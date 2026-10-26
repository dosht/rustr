//! `/compress-video` (`routes/compress-video.ts`): metadata and an inline
//! JPEG thumbnail on upload, a detached CRF/q:v re-encode with a fifteen-minute
//! ceiling, polling with a result object.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine;
use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};
use transgate_core::tools::{self, BAD_SESSION_ID, Family, Level, compression, compression_format, quality};

use super::{FileResponse, probe_float, probe_int, progress_path, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::js;
use crate::media::{self, Ended};
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::CompressVideo;
const FAILED: &str = "Something went wrong. Please try again.";
const EXPIRED: &str = "Session expired. Please upload your file again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Idle,
    Processing,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct CompressVideoSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    thumbnail_path: Option<PathBuf>,
    original_name: String,
    input_extension: String,
    compressed_size: Option<u64>,
    status: Status,
    error: Option<String>,
    output_filename: Option<String>,
    output_width: Option<f64>,
    output_height: Option<f64>,
    duration_seconds: Option<f64>,
    video_bitrate_kbps: Option<f64>,
    height: Option<f64>,
    created_at: SystemTime,
}

impl Session for CompressVideoSession {
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
        (&Method::POST, "compress") => compress(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query).await,
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

#[derive(Default)]
struct Metadata {
    duration_seconds: Option<f64>,
    video_bitrate_kbps: Option<f64>,
    audio_bitrate_kbps: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    fps: Option<f64>,
}

/// `m && !isNaN(m) ? m : null` without the falsy-zero shortcut of `probe_*`.
fn kbps_raw(value: Option<&Value>) -> Option<f64> {
    if !js::truthy(value) {
        return None;
    }
    Some(tools::js_round(js::parse_int(&js::string(value?)) / 1000.0))
}

fn parse_metadata(stdout: &str) -> Option<Metadata> {
    let parsed = super::parse_probe(stdout)?;
    let format = parsed.get("format");
    let streams = parsed.get("streams").and_then(Value::as_array).cloned().unwrap_or_default();
    let of_type = |t: &str| streams.iter().find(|s| s.get("codec_type").and_then(Value::as_str) == Some(t)).cloned();
    let video = of_type("video").unwrap_or(Value::Null);
    let audio = of_type("audio").unwrap_or(Value::Null);
    let total_kbps = kbps_raw(format.and_then(|f| f.get("bit_rate")));
    let fps = video.get("r_frame_rate").and_then(Value::as_str).filter(|r| !r.is_empty()).and_then(|r| {
        let parts: Vec<&str> = r.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        let (num, den) = (js::parse_int(parts[0]), js::parse_int(parts[1]));
        (!num.is_nan() && !den.is_nan() && den > 0.0).then(|| tools::js_round(num / den * 100.0) / 100.0)
    });
    let video_kbps = kbps_raw(video.get("bit_rate"));
    let audio_kbps = kbps_raw(audio.get("bit_rate"));
    // `videoBitrateKbps ?? (total && audio ? total - audio : total)`
    let final_video = match video_kbps {
        Some(v) => Some(v),
        None => match (total_kbps, audio_kbps) {
            (Some(t), Some(a)) if t != 0.0 && !t.is_nan() && a != 0.0 && !a.is_nan() => Some(t - a),
            (t, _) => t,
        },
    };
    Some(Metadata {
        duration_seconds: probe_float(format.and_then(|f| f.get("duration"))),
        video_bitrate_kbps: final_video.and_then(js::nonzero),
        audio_bitrate_kbps: audio_kbps.and_then(js::nonzero),
        width: probe_int(video.get("width")),
        height: probe_int(video.get("height")),
        fps: fps.and_then(js::nonzero),
    })
}

async fn metadata(p: &Processor, path: &Path) -> Metadata {
    let args: Vec<OsString> = vec![
        "-v".into(),
        "error".into(),
        "-show_entries".into(),
        "format=duration,bit_rate".into(),
        "-show_entries".into(),
        "stream=width,height,r_frame_rate,bit_rate,codec_type".into(),
        "-of".into(),
        "json".into(),
        path.into(),
    ];
    match media::output(&p.media.ffprobe, &args, 4 * 1024 * 1024, None).await {
        Ok(stdout) => parse_metadata(&stdout).unwrap_or_default(),
        Err(_) => Metadata::default(),
    }
}

/// A frame at one second, else the first frame, as JPEG (`-q:v 3`).
async fn extract_thumbnail(p: &Processor, input: &Path, thumb: &Path) -> bool {
    for at in ["00:00:01", "00:00:00"] {
        let args: Vec<OsString> = vec![
            "-ss".into(),
            at.into(),
            "-i".into(),
            input.into(),
            "-vframes".into(),
            "1".into(),
            "-q:v".into(),
            "3".into(),
            thumb.into(),
            "-y".into(),
        ];
        let ran = media::output(&p.media.ffmpeg, &args, 4 * 1024 * 1024, Some(Duration::from_secs(30))).await;
        if ran.is_ok() && tokio::fs::metadata(thumb).await.is_ok() {
            return true;
        }
    }
    false
}

async fn upload(p: &Arc<Processor>, inc: Incoming) -> Response {
    let uploaded = match receive_upload(p, FAMILY, inc).await {
        Ok(u) => u,
        Err(error) => return error.response(),
    };
    let (id, ext, name) = (uploaded.id, uploaded.accepted.extension, uploaded.accepted.safe_name);
    let meta = metadata(p, &uploaded.path).await;
    let thumb = p.work_dir().join(format!("{}{id}_thumb.jpg", FAMILY.file_prefix()));
    let extracted = extract_thumbnail(p, &uploaded.path, &thumb).await;
    let thumbnail_base64 = if extracted {
        tokio::fs::read(&thumb)
            .await
            .ok()
            .map(|data| format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(data)))
    } else {
        None
    };
    p.compress_video.store.create(
        id.clone(),
        CompressVideoSession {
            upload_path: uploaded.path,
            output_path: None,
            thumbnail_path: extracted.then_some(thumb),
            original_name: name.clone(),
            input_extension: ext.clone(),
            compressed_size: None,
            status: Status::Idle,
            error: None,
            output_filename: None,
            output_width: None,
            output_height: None,
            duration_seconds: meta.duration_seconds,
            video_bitrate_kbps: meta.video_bitrate_kbps,
            height: meta.height,
            created_at: SystemTime::now(),
        },
    );
    log::info(
        "Compress video file uploaded",
        json!({
            "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size),
            "durationSeconds": meta.duration_seconds, "videoBitrateKbps": meta.video_bitrate_kbps,
            "audioBitrateKbps": meta.audio_bitrate_kbps, "width": meta.width, "height": meta.height, "fps": meta.fps,
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
            "videoBitrateKbps" => meta.video_bitrate_kbps,
            "audioBitrateKbps" => meta.audio_bitrate_kbps,
            "width" => meta.width,
            "height" => meta.height,
            "fps" => meta.fps,
            "thumbnailBase64" => thumbnail_base64,
        },
    )
}

async fn thumbnail(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(thumb) = p.compress_video.session(id).and_then(|s| s.thumbnail_path) else {
        return http::error(404, "Thumbnail not found.");
    };
    send_file(
        FileResponse {
            path: &thumb,
            content_type: "image/jpeg",
            filename: None,
            cache_control: Some("private, max-age=3600"),
            missing: "Thumbnail not found or already deleted.",
            unreadable: "Failed to serve thumbnail.",
        },
        None,
    )
    .await
}

async fn compress(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let id = id.to_string();
    if !p.compress_video.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let level = match quality(http::arg(&body, "quality")) {
        Ok(level) => level,
        Err(refusal) => return http::refusal(refusal),
    };
    let claimed = p.compress_video.store.with(&id, |session| {
        let session = session.ok_or(tools::processing_expired(FAMILY))?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        compression_format(&session.input_extension)?;
        session.status = Status::Processing;
        session.error = None;
        Ok(session.clone())
    });
    let session = match claimed {
        Ok(session) => session,
        Err(refusal) => return http::refusal(refusal),
    };
    let processor = p.clone();
    let response = obj! { "accepted" => true, "sessionId" => id.clone() };
    let _ = p.compress_video.spawn(id.clone(), async move { run(processor, id, session, level).await });
    http::json(202, response)
}

async fn run(p: Arc<Processor>, id: String, session: CompressVideoSession, level: Level) {
    let format = session.input_extension.clone();
    let output = super::output_path(&p, FAMILY, &id, &format);
    let prog = progress_path(&p, FAMILY, &id);
    let plan = compression(&format, level, session.height, session.video_bitrate_kbps);
    let [t, n] = p.media.thread_args();
    let mut args = super::threaded_input(&p, &session.upload_path);
    args.extend([t.into(), n.into(), "-codec:v".into(), plan.video_codec.into()]);
    args.extend([plan.quality_flag.into(), plan.quality_value.to_string().into()]);
    if let Some(scale) = &plan.scale {
        args.extend(["-vf".into(), scale.into()]);
    }
    if let Some(preset) = plan.preset {
        args.extend(["-preset".into(), preset.into()]);
    }
    if let Some((maxrate, bufsize)) = plan.rate_cap_kbps {
        args.extend([
            "-maxrate".into(),
            format!("{}k", js::arg(maxrate)).into(),
            "-bufsize".into(),
            format!("{}k", js::arg(bufsize)).into(),
        ]);
    }
    args.extend([
        "-codec:a".into(),
        "aac".into(),
        "-b:a".into(),
        format!("{}k", plan.audio_bitrate_kbps).into(),
        "-ar".into(),
        plan.audio_sample_rate.to_string().into(),
        "-ac".into(),
        plan.audio_channels.to_string().into(),
        "-progress".into(),
        prog.into(),
        "-nostats".into(),
        "-y".into(),
        output.clone().into(),
    ]);
    // The source probes the duration here and does not use it.
    let _ = super::duration_us(&p, &session.upload_path).await;
    let ended = media::process(&p.media.ffmpeg, &args, FAMILY.processing_kill_after(), |_| {}, || async {}).await;
    let store = &p.compress_video.store;
    let failure = match ended {
        Ended::Success => match tokio::fs::metadata(&output).await {
            Ok(meta) => {
                let (width, height) = output_size(&p, &output).await;
                let filename = format!("{}_compressed.{format}", session.original_name);
                store.update(&id, |s| {
                    s.status = Status::Done;
                    s.error = None;
                    s.output_path = Some(output.clone());
                    s.compressed_size = Some(meta.len());
                    s.output_filename = Some(filename);
                    s.output_width = width;
                    s.output_height = height;
                });
                log::info(
                    "Compression complete",
                    json!({ "sessionId": id, "format": format, "quality": level.as_str(), "sizeBytes": meta.len() }),
                );
                return;
            }
            Err(e) => Some(e.to_string()),
        },
        Ended::TimedOut => None,
        Ended::Cancelled => return,
        Ended::Failed(code) => Some(format!("FFmpeg exited with code {code:?}")),
        Ended::SpawnFailed(e) => Some(e.to_string()),
    };
    let _ = tokio::fs::remove_file(&output).await;
    let message = match failure {
        None => {
            log::warn("Compression timed out — SIGKILL sent", json!({ "sessionId": id }));
            "Compression timed out. Try a shorter or smaller file."
        }
        Some(e) => {
            log::error("Compression failed", json!({ "sessionId": id }), &e);
            FAILED
        }
    };
    store.update(&id, |s| {
        s.status = Status::Failed;
        s.error = Some(message.into());
    });
}

/// Width and height of the first stream that has both; optional metadata.
async fn output_size(p: &Processor, path: &Path) -> (Option<f64>, Option<f64>) {
    let args: Vec<OsString> =
        vec!["-v".into(), "error".into(), "-show_entries".into(), "stream=width,height".into(), "-of".into(), "json".into(), path.into()];
    let Ok(stdout) = media::output(&p.media.ffprobe, &args, 1024 * 1024, None).await else { return (None, None) };
    let Some(parsed) = super::parse_probe(&stdout) else { return (None, None) };
    let streams = parsed.get("streams").and_then(Value::as_array).cloned().unwrap_or_default();
    let Some(stream) = streams.iter().find(|s| js::truthy(s.get("width")) && js::truthy(s.get("height"))) else {
        return (None, None);
    };
    let int = |v: Option<&Value>| v.map(|v| js::parse_int(&js::string(v))).and_then(js::nonzero);
    (int(stream.get("width")), int(stream.get("height")))
}

async fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.compress_video.session(id) else {
        return http::error(410, "Session expired.");
    };
    match session.status {
        Status::Failed => http::json(
            200,
            obj! { "status" => "error", "progress" => 0.0, "error" => session.error.as_deref().unwrap_or(FAILED) },
        ),
        Status::Done => http::json(
            200,
            obj! {
                "status" => "done",
                "progress" => 100.0,
                "result" => obj! {
                    "filename" => session.output_filename,
                    "extension" => session.input_extension,
                    "sizeBytes" => session.compressed_size,
                    "outputWidth" => session.output_width,
                    "outputHeight" => session.output_height,
                },
            },
        ),
        Status::Processing => {
            let total_us = session.duration_seconds.map(|d| tools::js_round(d * 1_000_000.0));
            let out = media::progress_file_value(&progress_path(p, FAMILY, id), "out_time_us=").await;
            let percent = tools::compress_percent(Some(out), total_us);
            http::json(200, obj! { "status" => "processing", "progress" => percent })
        }
        Status::Idle => http::json(200, obj! { "status" => "idle", "progress" => 0.0 }),
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.compress_video.session(id) else {
        return http::error(410, EXPIRED);
    };
    if session.status != Status::Done {
        return http::error(425, "Compression not complete yet.");
    }
    let (Some(path), Some(filename)) = (session.output_path.clone(), session.output_filename.clone()) else {
        return http::error(404, "File not found. Please compress your file first.");
    };
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.compress_video.cleanup(&id, processor.work_dir()).await;
            log::info("Compressed file downloaded — session cleaned up", json!({ "sessionId": id }));
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
    p.compress_video.cleanup(id, p.work_dir()).await;
    log::info("compress-video session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_bitrate_falls_back_to_total_minus_audio() {
        let json = r#"{"format":{"duration":"10.5","bit_rate":"2128000"},"streams":[
            {"codec_type":"video","width":1920,"height":1080,"r_frame_rate":"30000/1001"},
            {"codec_type":"audio","bit_rate":"128000"}]}"#;
        let m = parse_metadata(json).unwrap();
        assert_eq!((m.duration_seconds, m.video_bitrate_kbps, m.audio_bitrate_kbps), (Some(10.5), Some(2000.0), Some(128.0)));
        assert_eq!((m.width, m.height, m.fps), (Some(1920.0), Some(1080.0), Some(29.97)));
        let total_only = parse_metadata(r#"{"format":{"bit_rate":"900000"},"streams":[{"codec_type":"video"}]}"#).unwrap();
        assert_eq!((total_only.video_bitrate_kbps, total_only.width, total_only.fps), (Some(900.0), None, None));
    }
}
