//! `/convert-audio` (`routes/convert-audio.ts`): the conversion runs inside
//! the request and answers 200 when done; progress is kept in the session.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::Method;
use axum::response::Response;
use serde_json::json;
use transgate_core::tools::{self, BAD_SESSION_ID, Family, convert_audio_differs, convert_audio_options};

use super::{FileResponse, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::ConvertAudio;
const EXPIRED: &str = "Your session has expired. Please upload your file again.";

#[derive(Clone, Debug)]
pub struct ConvertAudioSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    output_extension: Option<String>,
    original_name: String,
    input_extension: String,
    created_at: SystemTime,
    progress: f64,
}

impl Session for ConvertAudioSession {
    fn created_at(&self) -> SystemTime {
        self.created_at
    }

    fn files(&self, _id: &str, _dir: &Path) -> Vec<PathBuf> {
        [Some(self.upload_path.clone()), self.output_path.clone()].into_iter().flatten().collect()
    }
}

pub async fn route(p: &Arc<Processor>, action: &str, inc: Incoming) -> Option<Response> {
    Some(match (&inc.method, action) {
        (&Method::POST, "upload") => upload(p, inc).await,
        (&Method::POST, "convert") => convert(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query),
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

async fn upload(p: &Arc<Processor>, inc: Incoming) -> Response {
    let uploaded = match receive_upload(p, FAMILY, inc).await {
        Ok(u) => u,
        Err(error) => return error.response(),
    };
    let (id, ext, name) = (uploaded.id, uploaded.accepted.extension, uploaded.accepted.safe_name);
    p.convert_audio.store.create(
        id.clone(),
        ConvertAudioSession {
            upload_path: uploaded.path,
            output_path: None,
            output_extension: None,
            original_name: name.clone(),
            input_extension: ext.clone(),
            created_at: SystemTime::now(),
            progress: 0.0,
        },
    );
    log::info(
        "Convert audio file uploaded",
        json!({ "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size) }),
    );
    http::json(200, obj! { "sessionId" => id, "originalName" => name, "inputExtension" => ext })
}

async fn convert(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let id = id.to_string();
    if !p.convert_audio.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let encoding = match convert_audio_options(
        http::arg(&body, "outputFormat"),
        http::arg(&body, "bitrate"),
        http::arg(&body, "sampleRate"),
        http::arg(&body, "channels"),
    ) {
        Ok(e) => e,
        Err(refusal) => return http::refusal(refusal),
    };
    let Some(session) = p.convert_audio.session(&id) else {
        return http::error(410, EXPIRED);
    };
    if let Err(refusal) = convert_audio_differs(&session.input_extension, &encoding) {
        return http::refusal(refusal);
    }
    let output = super::output_path(p, FAMILY, &id, encoding.output_ext);
    log::info(
        "Starting audio conversion",
        json!({
            "sessionId": id,
            "outputFormat": body.get("outputFormat"),
            "bitrate": body.get("bitrate"),
            "sampleRate": encoding.sample_rate,
            "channels": encoding.channels,
        }),
    );
    p.convert_audio.store.update(&id, |s| s.progress = 0.0);
    // The client waits for this response, but a dropped connection must not
    // stop the conversion or leave the session half-updated.
    let processor = p.clone();
    let worker_id = id.clone();
    let operation = async move {
        let p = processor;
        let total_us = super::duration_us(&p, &session.upload_path).await;
        let args = super::audio_args(&session.upload_path, &output, &encoding);
        let store = &p.convert_audio.store;
        super::encode_with_stderr_progress(&p, &args, total_us, |pct| {
            store.update(&id, |s| s.progress = pct);
        })
        .await?;
        let size = tokio::fs::metadata(&output).await.map_err(|e| e.to_string())?.len();
        store.update(&id, |s| {
            s.output_path = Some(output.clone());
            s.output_extension = Some(encoding.output_ext.to_string());
            s.progress = 100.0;
        });
        log::info(
            "Conversion complete",
            json!({ "sessionId": id, "outputExt": encoding.output_ext, "sizeBytes": size, "effectiveSampleRate": encoding.sample_rate }),
        );
        Ok::<_, String>(obj! {
            "success" => true,
            "filename" => format!("{}.{}", session.original_name, encoding.output_ext),
            "extension" => encoding.output_ext,
            "sizeBytes" => size,
            "effectiveSampleRate" => encoding.sample_rate,
        })
    };
    let (reply, result) = tokio::sync::oneshot::channel();
    if !p.convert_audio.spawn(worker_id, async move { let _ = reply.send(operation.await); }) {
        return http::error(410, EXPIRED);
    }
    match result.await {
        Ok(Ok(result)) => http::json(200, result),
        Ok(Err(e)) => {
            log::error("Conversion failed", json!({ "sessionId": body.get("sessionId") }), &e);
            http::error(500, "Something went wrong during conversion. Please try again.")
        }
        Err(e) => {
            log::error("Conversion failed", json!({ "sessionId": body.get("sessionId") }), &e);
            http::error(500, "Something went wrong during conversion. Please try again.")
        }
    }
}

fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    match p.convert_audio.session(id) {
        Some(session) => http::json(200, obj! { "progress" => session.progress }),
        None => http::error(410, "Session not found or expired."),
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.convert_audio.session(id) else {
        return http::error(410, EXPIRED);
    };
    let (Some(path), Some(ext)) = (session.output_path.clone(), session.output_extension.clone()) else {
        return http::error(404, "File not found. Please convert your file first.");
    };
    let filename = format!("{}.{ext}", session.original_name);
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.convert_audio.cleanup(&id, processor.work_dir()).await;
            log::info("Converted file downloaded — session cleaned up", json!({ "sessionId": id }));
        });
    });
    send_file(
        FileResponse {
            path: &path,
            content_type: FAMILY.content_type(&ext),
            filename: Some(&filename),
            cache_control: None,
            missing: "File not found or already deleted.",
            unreadable: "Download failed.",
        },
        Some(on_end),
    )
    .await
}

async fn invalidate(p: &Processor, inc: Incoming) -> Response {
    let (body, _) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    p.convert_audio.cleanup(id, p.work_dir()).await;
    log::info("convert-audio session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}
