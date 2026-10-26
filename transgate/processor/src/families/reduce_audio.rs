//! `/reduce-audio` (`routes/reduce-audio.ts`): metadata on upload, a detached
//! preset re-encode in the upload's own format (202), and polling.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::Method;
use axum::response::Response;
use serde_json::json;
use transgate_core::tools::{self, AudioEncoding, BAD_SESSION_ID, Family, quality, reduce_audio};

use super::{FileResponse, probe_float, probe_int, probe_kbps, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::media;
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::ReduceAudio;
const FAILED: &str = "Something went wrong during reduction. Please try again.";
const EXPIRED: &str = "Session expired. Please upload your file again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Idle,
    Processing,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ReduceAudioSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    original_name: String,
    input_extension: String,
    reduced_size: Option<u64>,
    created_at: SystemTime,
    progress: f64,
    status: Status,
    error: Option<String>,
    output_filename: Option<String>,
    output_extension: Option<String>,
}

impl Session for ReduceAudioSession {
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
        (&Method::POST, "reduce") => reduce(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query),
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

struct Metadata {
    duration_seconds: Option<f64>,
    bitrate_kbps: Option<f64>,
    sample_rate: Option<f64>,
    channels: Option<f64>,
}

/// `extractMetadata`: format duration/bit rate and the first stream's rate and channels.
async fn metadata(p: &Processor, path: &Path) -> Metadata {
    let args: [&std::ffi::OsStr; 9] = [
        "-v".as_ref(),
        "error".as_ref(),
        "-show_entries".as_ref(),
        "format=duration,bit_rate".as_ref(),
        "-show_entries".as_ref(),
        "stream=sample_rate,channels".as_ref(),
        "-of".as_ref(),
        "json".as_ref(),
        path.as_os_str(),
    ];
    let none = Metadata { duration_seconds: None, bitrate_kbps: None, sample_rate: None, channels: None };
    let Ok(stdout) = media::output(&p.media.ffprobe, &args, 2 * 1024 * 1024, None).await else { return none };
    let Some(parsed) = super::parse_probe(&stdout) else { return none };
    let format = parsed.get("format");
    let stream = parsed.get("streams").and_then(|s| s.get(0));
    Metadata {
        duration_seconds: probe_float(format.and_then(|f| f.get("duration"))),
        bitrate_kbps: probe_kbps(format.and_then(|f| f.get("bit_rate"))),
        sample_rate: probe_int(stream.and_then(|s| s.get("sample_rate"))),
        channels: probe_int(stream.and_then(|s| s.get("channels"))),
    }
}

async fn upload(p: &Arc<Processor>, inc: Incoming) -> Response {
    let uploaded = match receive_upload(p, FAMILY, inc).await {
        Ok(u) => u,
        Err(error) => return error.response(),
    };
    let (id, ext, name) = (uploaded.id, uploaded.accepted.extension, uploaded.accepted.safe_name);
    let meta = metadata(p, &uploaded.path).await;
    p.reduce_audio.store.create(
        id.clone(),
        ReduceAudioSession {
            upload_path: uploaded.path,
            output_path: None,
            original_name: name.clone(),
            input_extension: ext.clone(),
            reduced_size: None,
            created_at: SystemTime::now(),
            progress: 0.0,
            status: Status::Idle,
            error: None,
            output_filename: None,
            output_extension: None,
        },
    );
    log::info(
        "Reduce audio file uploaded",
        json!({
            "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size),
            "durationSeconds": meta.duration_seconds, "bitrateKbps": meta.bitrate_kbps,
            "sampleRate": meta.sample_rate, "channels": meta.channels,
        }),
    );
    http::json(
        200,
        obj! {
            "sessionId" => id,
            "originalName" => name,
            "inputExtension" => ext,
            "fileSizeBytes" => uploaded.size,
            "durationSeconds" => meta.duration_seconds,
            "bitrateKbps" => meta.bitrate_kbps,
            "sampleRate" => meta.sample_rate,
            "channels" => meta.channels,
        },
    )
}

async fn reduce(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let id = id.to_string();
    if !p.reduce_audio.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let level = match quality(http::arg(&body, "quality")) {
        Ok(level) => level,
        Err(refusal) => return http::refusal(refusal),
    };
    let claimed = p.reduce_audio.store.with(&id, |session| {
        let session = session.ok_or(tools::processing_expired(FAMILY))?;
        let encoding = reduce_audio(&session.input_extension, level)?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        let filename = format!("{}_reduced.{}", session.original_name, encoding.output_ext);
        session.progress = 0.0;
        session.status = Status::Processing;
        session.error = None;
        session.output_filename = Some(filename.clone());
        session.output_extension = Some(encoding.output_ext.to_string());
        Ok((session.upload_path.clone(), encoding, filename))
    });
    let (input, encoding, filename) = match claimed {
        Ok(found) => found,
        Err(refusal) => return http::refusal(refusal),
    };
    log::info(
        "Starting audio reduction",
        json!({ "sessionId": id, "format": encoding.output_ext, "quality": level.as_str() }),
    );
    let response = obj! {
        "accepted" => true,
        "sessionId" => id.clone(),
        "filename" => filename,
        "extension" => encoding.output_ext,
    };
    let processor = p.clone();
    let _ = p.reduce_audio.spawn(id.clone(), async move { run(processor, id, input, encoding, level).await });
    http::json(202, response)
}

async fn run(p: Arc<Processor>, id: String, input: PathBuf, encoding: AudioEncoding, level: tools::Level) {
    let output = super::output_path(&p, FAMILY, &id, encoding.output_ext);
    let store = &p.reduce_audio.store;
    let result = async {
        let total_us = super::duration_us(&p, &input).await;
        let args = super::audio_args(&input, &output, &encoding);
        super::encode_with_stderr_progress(&p, &args, total_us, |pct| {
            store.update(&id, |s| s.progress = pct);
        })
        .await?;
        Ok::<_, String>(tokio::fs::metadata(&output).await.map_err(|e| e.to_string())?.len())
    }
    .await;
    match result {
        Ok(size) => {
            store.update(&id, |s| {
                s.output_path = Some(output.clone());
                s.reduced_size = Some(size);
                s.progress = 100.0;
                s.status = Status::Done;
            });
            log::info(
                "Reduction complete",
                json!({ "sessionId": id, "format": encoding.output_ext, "quality": level.as_str(), "sizeBytes": size }),
            );
        }
        Err(e) => {
            log::error("Reduction failed", json!({ "sessionId": id }), &e);
            store.update(&id, |s| {
                s.status = Status::Failed;
                s.error = Some(FAILED.into());
            });
        }
    }
}

fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.reduce_audio.session(id) else {
        return http::json(200, obj! { "progress" => 0.0, "percent" => 0.0, "status" => "expired" });
    };
    match session.status {
        Status::Done => http::json(
            200,
            obj! {
                "progress" => 100.0,
                "percent" => 100.0,
                "status" => "done",
                "filename" => session.output_filename,
                "extension" => session.output_extension,
                "reducedSizeBytes" => session.reduced_size,
            },
        ),
        Status::Failed => http::json(
            200,
            obj! {
                "progress" => 0.0,
                "percent" => 0.0,
                "status" => "failed",
                "error" => session.error.as_deref().unwrap_or(FAILED),
            },
        ),
        Status::Idle | Status::Processing => http::json(
            200,
            obj! { "progress" => session.progress, "percent" => session.progress, "status" => "processing" },
        ),
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.reduce_audio.session(id) else {
        return http::error(410, EXPIRED);
    };
    let Some(path) = session.output_path.clone() else {
        return http::error(404, "File not found. Please reduce your file first.");
    };
    let filename = format!("{}_reduced.{}", session.original_name, session.input_extension);
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.reduce_audio.cleanup(&id, processor.work_dir()).await;
            log::info("Reduced file downloaded — session cleaned up", json!({ "sessionId": id }));
        });
    });
    send_file(
        FileResponse {
            path: &path,
            content_type: FAMILY.content_type(&session.input_extension),
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
    p.reduce_audio.cleanup(id, p.work_dir()).await;
    log::info("reduce-audio session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}
