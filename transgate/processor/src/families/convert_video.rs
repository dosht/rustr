//! `/convert-video` (`routes/convert-video.ts`): metadata on upload (422 when
//! unreadable), a background PNG thumbnail, a detached re-encode with a
//! ten-minute ceiling and an out-of-memory diagnosis.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};
use transgate_core::tools::{self, BAD_SESSION_ID, Family, VideoConversion, video_conversion_options};

use super::{FileResponse, progress_path, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::js;
use crate::media::{self, Ended};
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::ConvertVideo;
const FAILED: &str = "Something went wrong during conversion. Please try again.";
const EXPIRED: &str = "Session expired. Please upload your file again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Idle,
    Processing,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ConvertVideoSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    thumbnail_path: Option<PathBuf>,
    original_name: String,
    output_format: String,
    status: Status,
    error: Option<String>,
    duration: f64,
    output_size: Option<u64>,
    output_resolution: Option<String>,
    output_fps: Option<f64>,
    created_at: SystemTime,
}

impl Session for ConvertVideoSession {
    fn created_at(&self) -> SystemTime {
        self.created_at
    }

    fn files(&self, id: &str, dir: &Path) -> Vec<PathBuf> {
        let prog = dir.join(format!("{}{id}.prog", FAMILY.file_prefix()));
        [Some(self.upload_path.clone()), self.output_path.clone(), self.thumbnail_path.clone(), Some(prog)]
            .into_iter()
            .flatten()
            .collect()
    }
}

pub async fn route(p: &Arc<Processor>, action: &str, inc: Incoming) -> Option<Response> {
    Some(match (&inc.method, action) {
        (&Method::POST, "upload") => upload(p, inc).await,
        (&Method::GET, "thumbnail") => thumbnail(p, &inc.query).await,
        (&Method::POST, "convert") => convert(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query).await,
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

/// `num/den` with `parseFloat`, or a bare `parseFloat` without exactly one `/`.
fn frame_rate(raw: &str, unreadable: Option<f64>) -> Option<f64> {
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() == 2 {
        let (num, den) = (js::parse_float(parts[0]), js::parse_float(parts[1]));
        return if den != 0.0 { Some(tools::js_round(num / den * 100.0) / 100.0) } else { unreadable };
    }
    let fps = js::parse_float(raw);
    if fps == 0.0 || fps.is_nan() { unreadable } else { Some(fps) }
}

struct Metadata {
    width: f64,
    height: f64,
    fps: f64,
    duration: f64,
}

fn parse_metadata(stdout: &str) -> Option<Metadata> {
    let parsed: Value = serde_json::from_str(stdout).ok()?;
    let stream = parsed.get("streams").and_then(|s| s.get(0)).cloned().unwrap_or(Value::Null);
    let int_or_zero = |v: Option<&Value>| {
        let n = v.map_or(f64::NAN, |v| js::parse_int(&js::string(v)));
        if n.is_nan() { 0.0 } else { n }
    };
    let rate = stream.get("r_frame_rate").filter(|v| js::truthy(Some(*v))).map_or("0/1".to_string(), js::string);
    let duration = parsed.get("format").and_then(|f| f.get("duration")).map_or(f64::NAN, |v| js::parse_float(&js::string(v)));
    Some(Metadata {
        width: int_or_zero(stream.get("width")),
        height: int_or_zero(stream.get("height")),
        fps: frame_rate(&rate, Some(0.0)).unwrap_or(f64::NAN),
        duration: if duration == 0.0 || duration.is_nan() { 0.0 } else { duration },
    })
}

async fn upload(p: &Arc<Processor>, inc: Incoming) -> Response {
    let uploaded = match receive_upload(p, FAMILY, inc).await {
        Ok(u) => u,
        Err(error) => return error.response(),
    };
    let (id, ext, name) = (uploaded.id, uploaded.accepted.extension, uploaded.accepted.safe_name);
    let args: Vec<OsString> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=width,height,r_frame_rate:format=duration".into(),
        "-of".into(),
        "json".into(),
        uploaded.path.clone().into(),
    ];
    let meta = match media::output(&p.media.ffprobe, &args, usize::MAX, None).await {
        Ok(stdout) => parse_metadata(&stdout).ok_or_else(|| "Unexpected ffprobe output".to_string()),
        Err(e) => Err(e.to_string()),
    };
    let meta = match meta {
        Ok(meta) => meta,
        Err(e) => {
            let _ = tokio::fs::remove_file(&uploaded.path).await;
            log::error("ffprobe failed during upload", json!({}), &e);
            return http::error(422, "Could not read video metadata. Please try a different file.");
        }
    };
    let resolution = if meta.width != 0.0 && meta.height != 0.0 {
        format!("{}x{}", js::arg(meta.width), js::arg(meta.height))
    } else {
        "unknown".into()
    };
    let input = uploaded.path.clone();
    p.convert_video.store.create(
        id.clone(),
        ConvertVideoSession {
            upload_path: uploaded.path,
            output_path: None,
            thumbnail_path: None,
            original_name: name.clone(),
            output_format: String::new(),
            status: Status::Idle,
            error: None,
            duration: meta.duration,
            output_size: None,
            output_resolution: None,
            output_fps: None,
            created_at: SystemTime::now(),
        },
    );
    // In the background: FFmpeg can hang on some files and the upload must not wait.
    let (processor, thumb_id) = (p.clone(), id.clone());
    let _ = p.convert_video.spawn(id.clone(), async move {
        let thumb = processor.work_dir().join(format!("{}{thumb_id}_thumb.png", FAMILY.file_prefix()));
        if super::png_thumbnail(&processor, &input, &thumb).await {
            processor.convert_video.store.update(&thumb_id, |s| s.thumbnail_path = Some(thumb));
        }
    });
    log::info(
        "Convert video file uploaded",
        json!({ "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size), "resolution": resolution, "fps": meta.fps }),
    );
    http::json(
        200,
        obj! {
            "sessionId" => id,
            "originalName" => name,
            "inputExtension" => ext,
            "resolution" => resolution,
            "fps" => meta.fps,
            "duration" => meta.duration,
            "sizeBytes" => uploaded.size,
            "hasThumbnail" => false,
        },
    )
}

async fn thumbnail(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.convert_video.session(id) else {
        return http::error(410, "Your session has expired. Please upload your file again.");
    };
    let Some(thumb) = session.thumbnail_path else {
        return http::error(404, "No thumbnail available.");
    };
    send_file(
        FileResponse {
            path: &thumb,
            content_type: "image/png",
            filename: None,
            cache_control: Some("private, max-age=3600"),
            missing: "Thumbnail not found.",
            unreadable: crate::http::SOMETHING_WENT_WRONG,
        },
        None,
    )
    .await
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
    if !p.convert_video.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let conversion = match video_conversion_options(
        http::arg(&body, "outputFormat"),
        http::arg(&body, "videoCodec"),
        http::arg(&body, "resolution"),
        http::arg(&body, "fps"),
        http::arg(&body, "videoBitrate"),
        http::arg(&body, "audioBitrate"),
        http::arg(&body, "sampleRate"),
        http::arg(&body, "channels"),
    ) {
        Ok(c) => c,
        Err(refusal) => return http::refusal(refusal),
    };
    let claimed = p.convert_video.store.with(&id, |session| {
        let session = session.ok_or(tools::processing_expired(FAMILY))?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        session.status = Status::Processing;
        session.output_format = conversion.output_ext.to_string();
        session.error = None;
        Ok(session.clone())
    });
    let session = match claimed {
        Ok(session) => session,
        Err(refusal) => return http::refusal(refusal),
    };
    log::info(
        "Accepted video conversion request",
        json!({ "sessionId": id, "outputFormat": conversion.output_ext, "videoCodec": conversion.video_codec }),
    );
    let response = obj! {
        "accepted" => true,
        "sessionId" => id.clone(),
        "outputFilename" => format!("{}.{}", session.original_name, conversion.output_ext),
    };
    let processor = p.clone();
    let _ = p.convert_video.spawn(id.clone(), async move { run(processor, id, session.upload_path, conversion).await });
    http::json(202, response)
}

async fn run(p: Arc<Processor>, id: String, input: PathBuf, c: VideoConversion) {
    let output = super::output_path(&p, FAMILY, &id, c.output_ext);
    let prog = progress_path(&p, FAMILY, &id);
    let [t, n] = p.media.thread_args();
    let mut args = super::threaded_input(&p, &input);
    args.extend([
        t.into(),
        n.into(),
        "-c:v".into(),
        c.video_codec.into(),
        "-b:v".into(),
        format!("{}k", js::arg(c.video_bitrate_kbps)).into(),
    ]);
    if let Some((w, h)) = c.scale {
        args.extend(["-vf".into(), format!("scale={w}:{h}").into()]);
    }
    if let Some(fps) = c.fps {
        args.extend(["-r".into(), js::arg(fps).into()]);
    }
    args.extend([
        "-c:a".into(),
        c.audio_codec.into(),
        "-b:a".into(),
        format!("{}k", js::arg(c.audio_bitrate_kbps)).into(),
        "-ar".into(),
        js::arg(c.sample_rate).into(),
        "-ac".into(),
        js::arg(c.channels).into(),
    ]);
    if let Some(container) = c.container {
        args.extend(["-f".into(), container.into()]);
    }
    args.extend(["-progress".into(), prog.clone().into(), "-nostats".into(), "-y".into(), output.clone().into()]);
    log::info(
        "Starting FFmpeg video conversion",
        json!({ "sessionId": id, "outputFormat": c.output_ext, "videoCodec": c.video_codec }),
    );
    let store = &p.convert_video.store;
    let ended = super::run_progress_file_child(&p, &args, FAMILY.processing_kill_after(), &prog, &output, || {
        log::error("FFmpeg video conversion timed out — killed", json!({ "sessionId": id }), &"SIGKILL timeout");
        store.update(&id, |s| {
            s.status = Status::Failed;
            s.error = Some("Conversion timed out. Try a shorter or smaller file.".into());
        });
    })
    .await;
    let fail = |message: &str| {
        store.update(&id, |s| {
            s.status = Status::Failed;
            s.error = Some(message.into());
        });
    };
    match ended {
        Ended::TimedOut | Ended::Cancelled => {}
        Ended::Failed(code) => {
            log::error(
                "FFmpeg video exited with non-zero code",
                json!({ "sessionId": id, "outOfMemory": code.is_none() }),
                &format!("FFmpeg exit code: {code:?}"),
            );
            let _ = tokio::fs::remove_file(&output).await;
            fail(tools::conversion_failure(code));
        }
        Ended::SpawnFailed(e) => {
            log::error("FFmpeg video process error", json!({ "sessionId": id }), &e);
            fail(FAILED);
        }
        Ended::Success => {
            let (meta, (resolution, fps)) = tokio::join!(tokio::fs::metadata(&output), probe_output(&p, &output));
            match meta {
                Ok(meta) => {
                    store.update(&id, |s| {
                        s.output_path = Some(output.clone());
                        s.output_format = c.output_ext.to_string();
                        s.output_size = Some(meta.len());
                        s.output_resolution = resolution.clone();
                        s.output_fps = fps;
                        s.status = Status::Done;
                        s.error = None;
                    });
                    log::info(
                        "FFmpeg video conversion complete",
                        json!({ "sessionId": id, "sizeBytes": meta.len(), "outputResolution": resolution }),
                    );
                }
                Err(e) => {
                    log::error("Failed to stat output file after encode", json!({ "sessionId": id }), &e);
                    fail(FAILED);
                }
            }
        }
    }
}

/// The encoded file's resolution and frame rate; optional metadata.
async fn probe_output(p: &Processor, path: &Path) -> (Option<String>, Option<f64>) {
    let args: Vec<OsString> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=width,height,r_frame_rate".into(),
        "-of".into(),
        "json".into(),
        path.into(),
    ];
    let Ok(stdout) = media::output(&p.media.ffprobe, &args, usize::MAX, None).await else { return (None, None) };
    let Some(stream) = super::parse_probe(&stdout).and_then(|v| v.get("streams").and_then(|s| s.get(0)).cloned()) else {
        return (None, None);
    };
    let resolution = match (stream.get("width"), stream.get("height")) {
        (w, h) if js::truthy(w) && js::truthy(h) => Some(format!("{}x{}", js::string(w.unwrap()), js::string(h.unwrap()))),
        _ => None,
    };
    let fps = stream
        .get("r_frame_rate")
        .filter(|v| js::truthy(Some(*v)))
        .and_then(|v| frame_rate(&js::string(v), None))
        .filter(|f| !f.is_nan());
    (resolution, fps)
}

async fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.convert_video.session(id) else {
        return http::json(200, obj! { "percent" => 0.0, "status" => "expired" });
    };
    match session.status {
        Status::Done => http::json(
            200,
            obj! {
                "percent" => 100.0,
                "status" => "done",
                "outputSizeBytes" => session.output_size,
                "outputExtension" => session.output_format.clone(),
                "outputFilename" => format!("{}.{}", session.original_name, session.output_format),
                "outputResolution" => session.output_resolution,
                "outputFps" => session.output_fps,
            },
        ),
        Status::Failed => http::json(
            200,
            obj! { "percent" => 0.0, "status" => "failed", "error" => session.error.as_deref().unwrap_or(FAILED) },
        ),
        Status::Idle | Status::Processing => {
            let out = media::progress_file_value(&progress_path(p, FAMILY, id), "out_time_ms=").await;
            let percent = tools::file_percent(out, Some(session.duration)).unwrap_or(0.0);
            http::json(200, obj! { "percent" => percent, "status" => "processing" })
        }
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.convert_video.session(id) else {
        return http::error(410, EXPIRED);
    };
    if session.status != Status::Done {
        return http::error(425, "Conversion not complete yet.");
    }
    let Some(path) = session.output_path.clone() else {
        return http::error(404, "File not ready. Please wait for conversion to complete.");
    };
    let filename = format!("{}.{}", session.original_name, session.output_format);
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.convert_video.cleanup(&id, processor.work_dir()).await;
            log::info("Converted video downloaded — session cleaned up", json!({ "sessionId": id }));
        });
    });
    send_file(
        FileResponse {
            path: &path,
            content_type: FAMILY.content_type(&session.output_format),
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
    p.convert_video.cleanup(id, p.work_dir()).await;
    log::info("convert-video session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_metadata_defaults_like_the_source() {
        let m = parse_metadata(r#"{"streams":[{"width":640,"height":360,"r_frame_rate":"30000/1001"}],"format":{"duration":"2.5"}}"#).unwrap();
        assert_eq!((m.width, m.height, m.fps, m.duration), (640.0, 360.0, 29.97, 2.5));
        let empty = parse_metadata(r#"{"streams":[],"format":{}}"#).unwrap();
        assert_eq!((empty.width, empty.height, empty.fps, empty.duration), (0.0, 0.0, 0.0, 0.0));
        assert!(parse_metadata("{oops").is_none());
        assert_eq!(frame_rate("25", None), Some(25.0));
        assert_eq!(frame_rate("1/0", None), None);
    }
}
