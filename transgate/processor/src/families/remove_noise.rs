//! `/remove-noise` (`routes/remove-noise.ts`): afftdn over audio or the audio
//! of a video (video stream copied), detached with a ten-minute ceiling.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::Method;
use axum::response::Response;
use serde_json::json;
use transgate_core::tools::{self, BAD_SESSION_ID, Family, Level, denoise, noise_level};

use super::{FileResponse, progress_path, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::media::{self, Ended};
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::RemoveNoise;
const FAILED: &str = "Something went wrong during noise removal. Please try again.";
const EXPIRED: &str = "Session expired. Please upload your file again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Idle,
    Processing,
    Done,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Processing => "processing",
            Status::Done => "done",
            Status::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoveNoiseSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    original_name: String,
    input_extension: String,
    cleaned_size: Option<u64>,
    duration_seconds: Option<f64>,
    status: Status,
    error: Option<String>,
    created_at: SystemTime,
}

impl Session for RemoveNoiseSession {
    fn created_at(&self) -> SystemTime {
        self.created_at
    }

    fn files(&self, id: &str, dir: &Path) -> Vec<PathBuf> {
        let prog = dir.join(format!("{}{id}.prog", FAMILY.file_prefix()));
        [Some(self.upload_path.clone()), self.output_path.clone(), Some(prog)].into_iter().flatten().collect()
    }
}

impl RemoveNoiseSession {
    fn output_filename(&self) -> String {
        format!("{}_clean.{}", self.original_name, self.input_extension)
    }
}

pub async fn route(p: &Arc<Processor>, action: &str, inc: Incoming) -> Option<Response> {
    Some(match (&inc.method, action) {
        (&Method::POST, "upload") => upload(p, inc).await,
        (&Method::POST, "denoise") => denoise_request(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query).await,
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
    let duration = media::probe_duration(&p.media, &uploaded.path, 1024 * 1024)
        .await
        .ok()
        .filter(|s| !s.is_nan() && *s > 0.0);
    p.remove_noise.store.create(
        id.clone(),
        RemoveNoiseSession {
            upload_path: uploaded.path,
            output_path: None,
            original_name: name.clone(),
            input_extension: ext.clone(),
            cleaned_size: None,
            duration_seconds: duration,
            status: Status::Idle,
            error: None,
            created_at: SystemTime::now(),
        },
    );
    log::info(
        "Remove noise file uploaded",
        json!({ "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size), "durationSeconds": duration }),
    );
    http::json(
        200,
        obj! {
            "sessionId" => id,
            "originalName" => name,
            "inputExtension" => ext,
            "fileSizeBytes" => uploaded.size,
            "durationSeconds" => duration,
        },
    )
}

async fn denoise_request(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let id = id.to_string();
    if !p.remove_noise.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let level = match noise_level(http::arg(&body, "level")) {
        Ok(level) => level,
        Err(refusal) => return http::refusal(refusal),
    };
    let claimed = p.remove_noise.store.with(&id, |session| {
        let session = session.ok_or(tools::processing_expired(FAMILY))?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        session.status = Status::Processing;
        session.error = None;
        Ok(session.clone())
    });
    let session = match claimed {
        Ok(session) => session,
        Err(refusal) => return http::refusal(refusal),
    };
    log::info(
        "Accepted remove-noise request",
        json!({ "sessionId": id, "level": level.as_str(), "ext": session.input_extension }),
    );
    let response = obj! { "accepted" => true, "sessionId" => id.clone(), "outputFilename" => session.output_filename() };
    let processor = p.clone();
    let _ = p.remove_noise.spawn(id.clone(), async move { run(processor, id, session, level).await });
    http::json(202, response)
}

async fn run(p: Arc<Processor>, id: String, session: RemoveNoiseSession, level: Level) {
    let ext = &session.input_extension;
    let output = super::output_path(&p, FAMILY, &id, ext);
    let prog = progress_path(&p, FAMILY, &id);
    let plan = denoise(ext, level);
    let mut args: Vec<OsString> = super::threaded_input(&p, &session.upload_path);
    if plan.copy_video {
        args.extend(["-c:v".into(), "copy".into()]);
    }
    args.extend(["-af".into(), plan.filter.into(), "-codec:a".into(), plan.codec.into()]);
    args.extend(plan.extra.iter().map(Into::into));
    args.extend(["-progress".into(), prog.clone().into(), "-nostats".into(), "-y".into(), output.clone().into()]);
    let store = &p.remove_noise.store;
    let ended = super::run_progress_file_child(&p, &args, FAMILY.processing_kill_after(), &prog, &output, || {
        log::warn("FFmpeg denoise timed out — killed", json!({ "sessionId": id }));
        store.update(&id, |s| {
            s.status = Status::Failed;
            s.error = Some("Noise removal timed out. Try a shorter or smaller file.".into());
        });
    })
    .await;
    let fail = || {
        store.update(&id, |s| {
            s.status = Status::Failed;
            s.error = Some(FAILED.into());
        });
    };
    match ended {
        Ended::TimedOut | Ended::Cancelled => {}
        Ended::Failed(code) => {
            log::error("FFmpeg exited with non-zero code", json!({ "sessionId": id }), &format!("FFmpeg exit code: {code:?}"));
            fail();
            let _ = tokio::fs::remove_file(&output).await;
        }
        Ended::SpawnFailed(e) => {
            log::error("FFmpeg process error", json!({ "sessionId": id }), &e);
            fail();
        }
        Ended::Success => match tokio::fs::metadata(&output).await {
            Ok(meta) => {
                store.update(&id, |s| {
                    s.output_path = Some(output.clone());
                    s.cleaned_size = Some(meta.len());
                    s.status = Status::Done;
                    s.error = None;
                });
                log::info("FFmpeg noise removal complete", json!({ "sessionId": id, "sizeBytes": meta.len() }));
            }
            Err(e) => {
                log::error("Failed to stat output file", json!({ "sessionId": id }), &e);
                fail();
            }
        },
    }
}

async fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.remove_noise.session(id) else {
        return http::json(200, obj! { "percent" => 0.0, "status" => "expired" });
    };
    match session.status {
        Status::Done => http::json(
            200,
            obj! {
                "percent" => 100.0,
                "status" => "done",
                "outputSizeBytes" => session.cleaned_size,
                "outputExtension" => session.input_extension.clone(),
                "outputFilename" => session.output_filename(),
            },
        ),
        Status::Failed => http::json(
            200,
            obj! { "percent" => 0.0, "status" => "error", "error" => session.error.as_deref().unwrap_or(FAILED) },
        ),
        Status::Idle => http::json(200, obj! { "status" => Status::Idle.as_str(), "percent" => 0.0 }),
        Status::Processing => {
            let out = media::progress_file_value(&progress_path(p, FAMILY, id), "out_time_ms=").await;
            let percent = tools::file_percent(out, session.duration_seconds).unwrap_or(0.0);
            http::json(200, obj! { "percent" => percent, "status" => "denoising" })
        }
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.remove_noise.session(id) else {
        return http::error(410, EXPIRED);
    };
    if session.status != Status::Done {
        return http::error(425, "Noise removal not complete yet.");
    }
    let Some(path) = session.output_path.clone() else {
        return http::error(404, "File not ready. Please wait for noise removal to complete.");
    };
    let filename = session.output_filename();
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.remove_noise.cleanup(&id, processor.work_dir()).await;
            log::info("Cleaned file downloaded — session cleaned up", json!({ "sessionId": id }));
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
    p.remove_noise.cleanup(id, p.work_dir()).await;
    log::info("remove-noise session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}
