//! `/video-to-audio` (`routes/video-to-audio.ts`): an upload is refused (422)
//! without an audio stream; a PNG thumbnail; detached extraction with a
//! ten-minute ceiling.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};
use transgate_core::tools::{self, BAD_SESSION_ID, Extraction, Family, extraction_options};

use super::{FileResponse, probe_float, probe_int, probe_kbps, progress_path, receive_upload, send_file, valid_id};
use crate::http::{self, Incoming, Query};
use crate::media::{self, Ended};
use crate::store::Session;
use crate::{Processor, log, obj};

const FAMILY: Family = Family::VideoToAudio;
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
pub struct VideoToAudioSession {
    upload_path: PathBuf,
    output_path: Option<PathBuf>,
    thumbnail_path: Option<PathBuf>,
    original_name: String,
    output_format: String,
    status: Status,
    error: Option<String>,
    created_at: SystemTime,
    duration: f64,
    output_size: Option<u64>,
}

impl Session for VideoToAudioSession {
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

fn thumbnail_path(p: &Processor, id: &str) -> PathBuf {
    p.work_dir().join(format!("{}{id}_thumb.png", FAMILY.file_prefix()))
}

pub async fn route(p: &Arc<Processor>, action: &str, inc: Incoming) -> Option<Response> {
    Some(match (&inc.method, action) {
        (&Method::POST, "upload") => upload(p, inc).await,
        (&Method::GET, "thumbnail") => thumbnail(p, &inc.query).await,
        (&Method::POST, "extract") => extract(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query).await,
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

struct AudioStream {
    codec: Option<String>,
    bitrate_kbps: Option<f64>,
    sample_rate: Option<f64>,
    channels: Option<f64>,
    duration_seconds: Option<f64>,
}

/// `parseAudioStream`: `None` when there is no audio stream (or no JSON).
fn parse_audio_stream(stdout: &str) -> Option<AudioStream> {
    let parsed = super::parse_probe(stdout)?;
    let stream = parsed
        .get("streams")
        .and_then(Value::as_array)?
        .iter()
        .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio"))?;
    let format = parsed.get("format");
    Some(AudioStream {
        codec: stream.get("codec_name").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string),
        bitrate_kbps: probe_kbps(stream.get("bit_rate")),
        sample_rate: probe_int(stream.get("sample_rate")),
        channels: stream.get("channels").and_then(Value::as_f64),
        duration_seconds: probe_float(format.and_then(|f| f.get("duration"))),
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
        "a:0".into(),
        "-show_entries".into(),
        "stream=codec_type,codec_name,bit_rate,sample_rate,channels:format=duration,size".into(),
        "-of".into(),
        "json".into(),
        uploaded.path.clone().into(),
    ];
    let stdout = match media::output(&p.media.ffprobe, &args, usize::MAX, None).await {
        Ok(stdout) => stdout,
        Err(e) => {
            let _ = tokio::fs::remove_file(&uploaded.path).await;
            log::error("ffprobe failed on upload", json!({ "sessionId": id }), &e);
            return http::error(422, "Could not read video metadata. Please try a different file.");
        }
    };
    let Some(audio) = parse_audio_stream(&stdout) else {
        let _ = tokio::fs::remove_file(&uploaded.path).await;
        log::info("No audio stream detected — rejecting upload", json!({ "sessionId": id }));
        return http::error(422, "This video has no audio track. Nothing to extract.");
    };
    let duration = audio.duration_seconds.unwrap_or(0.0);
    p.video_to_audio.store.create(
        id.clone(),
        VideoToAudioSession {
            upload_path: uploaded.path.clone(),
            output_path: None,
            thumbnail_path: None,
            original_name: name.clone(),
            output_format: String::new(),
            status: Status::Idle,
            error: None,
            created_at: SystemTime::now(),
            duration,
            output_size: None,
        },
    );
    // A thumbnail failure does not fail the upload.
    let thumb = thumbnail_path(p, &id);
    let has_thumbnail = super::png_thumbnail(p, &uploaded.path, &thumb).await;
    if has_thumbnail {
        p.video_to_audio.store.update(&id, |s| s.thumbnail_path = Some(thumb));
    }
    log::info(
        "Video-to-audio file uploaded",
        json!({
            "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size),
            "duration": audio.duration_seconds, "audioCodec": audio.codec,
        }),
    );
    http::json(
        200,
        obj! {
            "sessionId" => id,
            "originalName" => name,
            "inputExtension" => ext,
            "duration" => duration,
            "audioCodec" => audio.codec,
            "audioBitrateKbps" => audio.bitrate_kbps,
            "sampleRate" => audio.sample_rate,
            "channels" => audio.channels,
            "sizeBytes" => uploaded.size,
            "hasThumbnail" => has_thumbnail,
        },
    )
}

async fn thumbnail(p: &Processor, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.video_to_audio.session(id) else {
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

async fn extract(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let Some(id) = valid_id(body.get("sessionId").and_then(|v| v.as_str())) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let id = id.to_string();
    if !p.video_to_audio.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let extraction = match extraction_options(
        http::arg(&body, "outputFormat"),
        http::arg(&body, "bitrate"),
        http::arg(&body, "sampleRate"),
        http::arg(&body, "channels"),
    ) {
        Ok(e) => e,
        Err(refusal) => return http::refusal(refusal),
    };
    let claimed = p.video_to_audio.store.with(&id, |session| {
        let session = session.ok_or(tools::processing_expired(FAMILY))?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        session.output_format = extraction.output_ext.to_string();
        session.status = Status::Processing;
        Ok(session.clone())
    });
    let session = match claimed {
        Ok(session) => session,
        Err(refusal) => return http::refusal(refusal),
    };
    log::info(
        "Accepted video-to-audio extraction request",
        json!({ "sessionId": id, "outputFormat": extraction.output_ext, "bitrate": extraction.bitrate_kbps }),
    );
    let response = obj! {
        "accepted" => true,
        "sessionId" => id.clone(),
        "outputFilename" => format!("{}.{}", session.original_name, extraction.output_ext),
    };
    let processor = p.clone();
    let _ = p.video_to_audio.spawn(id.clone(), async move { run(processor, id, session.upload_path, extraction).await });
    http::json(202, response)
}

async fn run(p: Arc<Processor>, id: String, input: PathBuf, extraction: Extraction) {
    let output = super::output_path(&p, FAMILY, &id, extraction.output_ext);
    let prog = progress_path(&p, FAMILY, &id);
    let mut args = super::threaded_input(&p, &input);
    args.extend(["-vn".into(), "-af".into(), "asetpts=PTS-STARTPTS".into(), "-codec:a".into(), extraction.codec.into()]);
    if let Some(kbps) = extraction.bitrate_kbps {
        args.extend(["-b:a".into(), format!("{}k", crate::js::arg(kbps)).into()]);
    }
    args.extend([
        "-ar".into(),
        crate::js::arg(extraction.sample_rate).into(),
        "-ac".into(),
        crate::js::arg(extraction.channels).into(),
    ]);
    match extraction.output_ext {
        "ogg" => args.extend(["-strict".into(), "-2".into()]),
        "opus" => args.extend(["-f".into(), "opus".into()]),
        _ => {}
    }
    args.extend([
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        "-progress".into(),
        prog.clone().into(),
        "-nostats".into(),
        "-y".into(),
        output.clone().into(),
    ]);
    let store = &p.video_to_audio.store;
    let ended = super::run_progress_file_child(&p, &args, FAMILY.processing_kill_after(), &prog, &output, || {
        log::warn("FFmpeg extraction timed out — killed", json!({ "sessionId": id }));
        store.update(&id, |s| {
            s.status = Status::Failed;
            s.error = Some("Extraction timed out. Try a shorter or smaller file.".into());
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
                    s.output_format = extraction.output_ext.to_string();
                    s.output_size = Some(meta.len());
                    s.status = Status::Done;
                    s.error = None;
                });
                log::info("FFmpeg audio extraction complete", json!({ "sessionId": id, "sizeBytes": meta.len() }));
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
    let Some(session) = p.video_to_audio.session(id) else {
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
            },
        ),
        Status::Failed => http::json(
            200,
            obj! { "percent" => 0.0, "status" => "error", "error" => session.error.as_deref().unwrap_or(FAILED) },
        ),
        Status::Idle => http::json(200, obj! { "status" => "idle", "percent" => 0.0 }),
        Status::Processing => {
            let out = media::progress_file_value(&progress_path(p, FAMILY, id), "out_time_ms=").await;
            let percent = tools::file_percent(out, Some(session.duration)).unwrap_or(0.0);
            http::json(200, obj! { "percent" => percent, "status" => "converting" })
        }
    }
}

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    let Some(id) = valid_id(query.text("sessionId")) else {
        return http::refusal(BAD_SESSION_ID);
    };
    let Some(session) = p.video_to_audio.session(id) else {
        return http::error(410, EXPIRED);
    };
    if session.status != Status::Done {
        return http::error(425, "Extraction not complete yet.");
    }
    let (Some(path), false) = (session.output_path.clone(), session.output_format.is_empty()) else {
        return http::error(404, "File not ready. Please wait for extraction to complete.");
    };
    let filename = format!("{}.{}", session.original_name, session.output_format);
    let (processor, id) = (p.clone(), id.to_string());
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            processor.video_to_audio.cleanup(&id, processor.work_dir()).await;
            log::info("Extracted audio downloaded — session cleaned up", json!({ "sessionId": id }));
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
    p.video_to_audio.cleanup(id, p.work_dir()).await;
    log::info("video-to-audio session invalidated by user", json!({ "sessionId": id }));
    http::json(200, obj! { "success" => true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_audio_stream_is_read_as_the_source_reads_it() {
        let json = r#"{"streams":[{"codec_type":"audio","codec_name":"aac","bit_rate":"128049","sample_rate":"44100","channels":2}],"format":{"duration":"3.000000","size":"1"}}"#;
        let a = parse_audio_stream(json).unwrap();
        assert_eq!((a.codec.as_deref(), a.bitrate_kbps, a.sample_rate, a.channels, a.duration_seconds),
            (Some("aac"), Some(128.0), Some(44100.0), Some(2.0), Some(3.0)));
        assert!(parse_audio_stream(r#"{"streams":[],"format":{}}"#).is_none());
        assert!(parse_audio_stream("not json").is_none());
        let sparse = parse_audio_stream(r#"{"streams":[{"codec_type":"audio","codec_name":""}]}"#).unwrap();
        assert_eq!((sparse.codec, sparse.bitrate_kbps, sparse.duration_seconds), (None, None, None));
    }
}
