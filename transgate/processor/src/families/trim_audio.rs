//! `/trim-audio` (`routes/trim-audio.ts`): upload with waveform, a detached
//! two-part cut (keyframe-aware for video), progress from two `-progress`
//! files, per-part downloads and a stored ZIP. The audio-trimmer and
//! video-trimmer pages use this same router.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::http::{Method, header};
use axum::response::Response;
use serde_json::json;
use transgate_core::tools::{
    self, Family, VideoCut, can_copy_at_trim_point, trim_as_video, trim_audio_codec, trim_point_in_range,
    trim_request, trim_video_encoders,
};

use super::{FileResponse, receive_upload, send_file};
use crate::http::{self, Incoming, Query, QueryValue, Segment, Streamed};
use crate::js;
use crate::media::{self, CAPTURE_LIMIT};
use crate::store::Session;
use crate::{Processor, log, obj, zip};

const FAMILY: Family = Family::TrimAudio;
const WAVEFORM_SAMPLES: usize = 200;
const TRIM_FAILED: &str = "Something went wrong while trimming. Please try again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Processing,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct TrimSession {
    upload_path: PathBuf,
    part1_path: Option<PathBuf>,
    part2_path: Option<PathBuf>,
    original_name: String,
    extension: String,
    total_duration: f64,
    trim_point: f64,
    status: Status,
    error: Option<String>,
    part1_size: Option<u64>,
    part2_size: Option<u64>,
    created_at: SystemTime,
    downloaded_part1: bool,
    downloaded_part2: bool,
    downloaded_zip: bool,
}

impl Session for TrimSession {
    fn created_at(&self) -> SystemTime {
        self.created_at
    }

    fn files(&self, _id: &str, _dir: &Path) -> Vec<PathBuf> {
        [Some(self.upload_path.clone()), self.part1_path.clone(), self.part2_path.clone()].into_iter().flatten().collect()
    }
}

impl TrimSession {
    fn part_name(&self, part: u8) -> String {
        format!("{}_part{part}.{}", self.original_name, self.extension)
    }
}

fn part_path(p: &Processor, id: &str, part: u8, ext: &str) -> PathBuf {
    p.work_dir().join(format!("trim_{id}_part{part}.{ext}"))
}

fn prog_path(p: &Processor, id: &str, part: u8) -> PathBuf {
    p.work_dir().join(format!("trim_{id}_p{part}.prog"))
}

pub async fn route(p: &Arc<Processor>, action: &str, inc: Incoming) -> Option<Response> {
    Some(match (&inc.method, action) {
        (&Method::POST, "upload") => upload(p, inc).await,
        (&Method::POST, "trim") => trim(p, inc).await,
        (&Method::GET, "progress") => progress(p, &inc.query).await,
        (&Method::GET, "download") => download(p, &inc.query).await,
        (&Method::GET, "download-zip") => download_zip(p, &inc.query).await,
        (&Method::DELETE, "invalidate") => invalidate(p, inc).await,
        _ => return None,
    })
}

async fn upload(p: &Arc<Processor>, inc: Incoming) -> Response {
    let uploaded = match receive_upload(p, FAMILY, inc).await {
        Ok(u) => u,
        Err(error) => return error.response(),
    };
    let id = uploaded.id.clone();
    let (total_duration, waveform) = match probe_and_waveform(p, &uploaded.path).await {
        Ok(found) => found,
        Err(e) => {
            log::warn(&format!("Waveform generation failed: {e}"), json!({ "sessionId": id }));
            (0.0, vec![0.1; WAVEFORM_SAMPLES])
        }
    };
    let ext = uploaded.accepted.extension.clone();
    let name = uploaded.accepted.safe_name.clone();
    p.trim_audio.store.create(
        id.clone(),
        TrimSession {
            upload_path: uploaded.path,
            part1_path: None,
            part2_path: None,
            original_name: name.clone(),
            extension: ext.clone(),
            total_duration,
            trim_point: 0.0,
            status: Status::Idle,
            error: None,
            part1_size: None,
            part2_size: None,
            created_at: SystemTime::now(),
            downloaded_part1: false,
            downloaded_part2: false,
            downloaded_zip: false,
        },
    );
    log::info(
        "Trim audio file uploaded",
        json!({ "sessionId": id, "ext": ext, "sizeMb": super::size_mb(uploaded.size), "duration": total_duration }),
    );
    http::json(
        200,
        obj! {
            "sessionId" => id,
            "originalName" => name,
            "extension" => ext,
            "totalDuration" => total_duration,
            "waveform" => waveform,
        },
    )
}

async fn probe_and_waveform(p: &Processor, path: &Path) -> Result<(f64, Vec<f64>), media::RunError> {
    let duration = media::probe_duration(&p.media, path, CAPTURE_LIMIT).await?;
    let waveform = waveform(p, path).await?;
    Ok((duration, waveform))
}

/// Peak amplitude of 200 equal blocks of a 4 kHz mono decode.
async fn waveform(p: &Processor, path: &Path) -> Result<Vec<f64>, media::RunError> {
    let raw = p.work_dir().join(format!("trim_waveform_{}.raw", uuid::Uuid::new_v4()));
    let args: Vec<OsString> = vec![
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        path.into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "4000".into(),
        "-f".into(),
        "f32le".into(),
        "-y".into(),
        raw.clone().into(),
    ];
    let result = async {
        media::output(&p.media.ffmpeg, &args, CAPTURE_LIMIT, None).await?;
        let mut file = tokio::fs::File::open(&raw).await.map_err(media::RunError::Spawn)?;
        let len = file.metadata().await.map_err(media::RunError::Spawn)?.len();
        let mut peaks = Peaks::new(len, WAVEFORM_SAMPLES);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut file, &mut buf).await.map_err(media::RunError::Spawn)?;
            if n == 0 {
                break;
            }
            peaks.feed(&buf[..n]);
        }
        Ok(peaks.finish())
    }
    .await;
    let _ = tokio::fs::remove_file(&raw).await;
    result
}

/// Peak absolute amplitude of `samples` equal blocks of little-endian f32
/// samples, fed in pieces so a long decode is never held in memory. Block `i`
/// covers samples `[i*b, i*b+b)` with `b = floor(count / samples)`; samples
/// past the last block are ignored, as in the source.
struct Peaks {
    block: u64,
    index: u64,
    peaks: Vec<f64>,
    carry: [u8; 4],
    carried: usize,
}

impl Peaks {
    fn new(total_bytes: u64, samples: usize) -> Self {
        Peaks { block: total_bytes / 4 / samples as u64, index: 0, peaks: vec![0.0; samples], carry: [0; 4], carried: 0 }
    }

    fn feed(&mut self, mut bytes: &[u8]) {
        if self.carried > 0 {
            let take = (4 - self.carried).min(bytes.len());
            self.carry[self.carried..self.carried + take].copy_from_slice(&bytes[..take]);
            self.carried += take;
            bytes = &bytes[take..];
            if self.carried < 4 {
                // Still short of a whole sample: keep the partial bytes for the next piece.
                return;
            }
            self.carried = 0;
            self.sample(f32::from_le_bytes(self.carry));
        }
        let (floats, rest) = bytes.as_chunks::<4>();
        for bytes in floats {
            self.sample(f32::from_le_bytes(*bytes));
        }
        self.carry[..rest.len()].copy_from_slice(rest);
        self.carried = rest.len();
    }

    fn sample(&mut self, value: f32) {
        if let Some(at) = self.index.checked_div(self.block) && let Some(peak) = self.peaks.get_mut(at as usize) {
            let abs = f64::from(value).abs();
            if abs > *peak {
                *peak = abs;
            }
        }
        self.index += 1;
    }

    fn finish(self) -> Vec<f64> {
        self.peaks.into_iter().map(|p| p.min(1.0)).collect()
    }
}

async fn trim(p: &Arc<Processor>, inc: Incoming) -> Response {
    let (body, ip) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let session_arg = http::arg(&body, "sessionId");
    let trim_point = match trim_request(session_arg, http::arg(&body, "trimPoint")) {
        Ok(point) => point,
        Err(refusal) => return http::refusal(refusal),
    };
    if !p.trim_audio.allow(&ip) {
        return http::refusal(tools::allowance_refusal(FAMILY));
    }
    let tools::Arg::Text(id) = session_arg else {
        return http::refusal(tools::processing_expired(FAMILY));
    };
    let id = id.to_string();
    let claimed = p.trim_audio.store.with(&id, |session| {
        let Some(session) = session else { return Err(tools::processing_expired(FAMILY)) };
        trim_point_in_range(trim_point, session.total_duration)?;
        if session.status == Status::Processing {
            return Err(tools::already_processing(FAMILY));
        }
        session.trim_point = trim_point;
        session.status = Status::Processing;
        session.error = None;
        Ok(session.clone())
    });
    let session = match claimed {
        Ok(session) => session,
        Err(refusal) => return http::refusal(refusal),
    };
    log::info(
        "Starting trim",
        json!({ "sessionId": id, "trimPoint": trim_point, "totalDuration": session.total_duration, "extension": session.extension }),
    );
    let response = obj! {
        "accepted" => true,
        "sessionId" => id.clone(),
        "part1" => obj! { "filename" => session.part_name(1), "duration" => trim_point },
        "part2" => obj! { "filename" => session.part_name(2), "duration" => session.total_duration - trim_point },
    };
    // Detached: the client polls /progress.
    let processor = p.clone();
    let _ = p.trim_audio.spawn(id.clone(), async move { run_trim(processor, id, session, trim_point).await });
    http::json(202, response)
}

async fn run_trim(p: Arc<Processor>, id: String, session: TrimSession, trim_point: f64) {
    let ext = &session.extension;
    let part1 = part_path(&p, &id, 1, ext);
    let part2 = part_path(&p, &id, 2, ext);
    let prog1 = prog_path(&p, &id, 1);
    let prog2 = prog_path(&p, &id, 2);
    let result = async {
        trim_file(&p, &id, &session.upload_path, ext, trim_point, [&part1, &part2], [&prog1, &prog2]).await?;
        let size1 = tokio::fs::metadata(&part1).await.map_err(|e| e.to_string())?.len();
        let size2 = tokio::fs::metadata(&part2).await.map_err(|e| e.to_string())?.len();
        Ok::<_, String>((size1, size2))
    }
    .await;
    match result {
        Ok((size1, size2)) => {
            p.trim_audio.store.update(&id, |s| {
                s.part1_path = Some(part1);
                s.part2_path = Some(part2);
                s.part1_size = Some(size1);
                s.part2_size = Some(size2);
                s.status = Status::Done;
            });
            log::info("Trim complete", json!({ "sessionId": id, "part1Size": size1, "part2Size": size2 }));
        }
        Err(e) => {
            log::error("Trim failed", json!({ "sessionId": id }), &e);
            p.trim_audio.store.update(&id, |s| {
                s.status = Status::Failed;
                s.error = Some(TRIM_FAILED.into());
            });
        }
    }
    let _ = tokio::fs::remove_file(&prog1).await;
    let _ = tokio::fs::remove_file(&prog2).await;
}

fn s(x: impl Into<OsString>) -> OsString {
    x.into()
}

async fn trim_file(
    p: &Processor,
    id: &str,
    input: &Path,
    ext: &str,
    trim_point: f64,
    [part1, part2]: [&Path; 2],
    [prog1, prog2]: [&Path; 2],
) -> Result<(), String> {
    let (part1_args, part2_args): (Vec<OsString>, Vec<OsString>) = if trim_as_video(ext) {
        // `-ss` before `-i` with `-c copy` snaps back to the previous keyframe,
        // so copying is only used when the cut is on one.
        let keyframe = last_keyframe_at_or_before(p, input, trim_point).await;
        let on_keyframe = can_copy_at_trim_point(keyframe, trim_point);
        let seam = if on_keyframe { seam_row(p, input).await } else { None };
        if !on_keyframe {
            log::info(
                "Trim point is not on a keyframe, re-encoding for an exact cut",
                json!({ "sessionId": id, "trimPoint": trim_point, "nearestKeyframe": keyframe }),
            );
        }
        match plan_video_cut(trim_point, keyframe, seam.as_deref()) {
            VideoCut::Copy { part1_limit, part2_start } => (
                vec![
                    s("-loglevel"), s("error"), s("-i"), s(input), s("-t"), s(js::arg(part1_limit)), s("-c"), s("copy"),
                    s("-avoid_negative_ts"), s("make_zero"), s("-progress"), s(prog1), s("-nostats"), s("-y"), s(part1),
                ],
                vec![
                    s("-loglevel"), s("error"), s("-ss"), s(js::arg(part2_start)), s("-i"), s(input), s("-c"), s("copy"),
                    s("-avoid_negative_ts"), s("make_zero"), s("-progress"), s(prog2), s("-nostats"), s("-y"), s(part2),
                ],
            ),
            VideoCut::Reencode => {
                let (video, audio) = trim_video_encoders(ext);
                let [t, n] = p.media.thread_args();
                let point = js::arg(trim_point);
                (
                    vec![
                        s("-loglevel"), s("error"), s(&t), s(&n), s("-i"), s(input), s("-t"), s(&point), s(&t), s(&n),
                        s("-c:v"), s(video), s("-c:a"), s(audio), s("-progress"), s(prog1), s("-nostats"), s("-y"), s(part1),
                    ],
                    vec![
                        s("-loglevel"), s("error"), s(&t), s(&n), s("-ss"), s(&point), s("-i"), s(input), s(&t), s(&n),
                        s("-c:v"), s(video), s("-c:a"), s(audio), s("-progress"), s(prog2), s("-nostats"), s("-y"), s(part2),
                    ],
                )
            }
        }
    } else {
        let codec = detect_audio_codec(p, input).await.map_err(|e| e.to_string())?;
        let codec = trim_audio_codec(&codec, ext);
        let point = js::arg(trim_point);
        (
            vec![
                s("-loglevel"), s("error"), s("-i"), s(input), s("-t"), s(&point), s("-map"), s("0:a:0"), s("-c:a"), s(codec),
                s("-progress"), s(prog1), s("-nostats"), s("-y"), s(part1),
            ],
            vec![
                s("-loglevel"), s("error"), s("-ss"), s(&point), s("-i"), s(input), s("-map"), s("0:a:0"), s("-c:a"), s(codec),
                s("-progress"), s(prog2), s("-nostats"), s("-y"), s(part2),
            ],
        )
    };
    // A failed part stops the other at once instead of letting it run on.
    media::output_pair(&p.media.ffmpeg, &part1_args, &part2_args, CAPTURE_LIMIT).await.map_err(|e| e.to_string())
}

async fn detect_audio_codec(p: &Processor, input: &Path) -> Result<String, media::RunError> {
    let args = [
        s("-v"), s("error"), s("-select_streams"), s("a:0"), s("-show_entries"), s("stream=codec_name"), s("-of"),
        s("default=noprint_wrappers=1:nokey=1"), s(input),
    ];
    Ok(media::output(&p.media.ffprobe, &args, CAPTURE_LIMIT, None).await?.trim().to_string())
}

/// Timestamp of the last video keyframe at or before `t` (plus the copy
/// epsilon), reading packet flags up to the cut only. `None` when unknown.
async fn last_keyframe_at_or_before(p: &Processor, input: &Path, t: f64) -> Option<f64> {
    let window = format!("%+{}", js::arg(t.ceil() + 1.0));
    let args = [
        s("-v"), s("error"), s("-select_streams"), s("v:0"), s("-read_intervals"), s(window), s("-show_entries"),
        s("packet=pts_time,flags"), s("-of"), s("csv=p=0"), s(input),
    ];
    let stdout = media::output(&p.media.ffprobe, &args, CAPTURE_LIMIT, None).await.ok()?;
    keyframe_from_packets(&stdout, t)
}

fn keyframe_from_packets(csv: &str, t: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for line in csv.split('\n') {
        let mut fields = line.split(',');
        let (Some(time), Some(flags)) = (fields.next(), fields.next()) else { continue };
        if !flags.contains('K') {
            continue;
        }
        let seconds = js::number(time);
        if !seconds.is_finite() || seconds > t + tools::KEYFRAME_EPSILON_SECONDS {
            continue;
        }
        if best.is_none_or(|b| seconds > b) {
            best = Some(seconds);
        }
    }
    best
}

/// `parseFrameRate`: ffprobe's `num/den`; 0 when unreadable.
fn parse_frame_rate(raw: &str) -> f64 {
    let mut parts = raw.trim().split('/');
    let n = js::number(parts.next().unwrap_or(""));
    let d = parts.next().map_or(1.0, js::number);
    if !n.is_finite() || !d.is_finite() || d == 0.0 {
        return 0.0;
    }
    let fps = n / d;
    if fps.is_finite() && fps > 0.0 { fps } else { 0.0 }
}

/// `parseSeamTiming`: ffprobe emits `has_b_frames,r_frame_rate` in that order.
fn parse_seam_timing(row: &str) -> (f64, f64) {
    let mut fields = row.trim().split(',');
    let b_frames = js::number(fields.next().unwrap_or(""));
    let b_frames = if b_frames.is_nan() { 0.0 } else { b_frames };
    (b_frames, parse_frame_rate(fields.next().unwrap_or("")))
}

/// The `has_b_frames,r_frame_rate` row of the first video stream; `None`
/// when the probe fails.
async fn seam_row(p: &Processor, input: &Path) -> Option<String> {
    let args = [
        s("-v"), s("error"), s("-select_streams"), s("v:0"), s("-show_entries"), s("stream=has_b_frames,r_frame_rate"),
        s("-of"), s("csv=p=0"), s(input),
    ];
    media::output(&p.media.ffprobe, &args, 1024 * 1024, None).await.ok()
}

/// How a video is cut, from what ffprobe reported: the last keyframe at or
/// before the cut, and (only consulted on a keyframe) the seam row. A failed
/// or unreadable seam probe means a copy without seam correction.
fn plan_video_cut(trim_point: f64, keyframe: Option<f64>, seam_row: Option<&str>) -> VideoCut {
    tools::video_cut(trim_point, keyframe, || seam_row.map_or((0.0, 0.0), parse_seam_timing))
}

async fn progress(p: &Processor, query: &Query) -> Response {
    let Some(id) = query.text("sessionId").filter(|id| !id.is_empty()) else {
        return http::error(400, "sessionId is required");
    };
    let Some(session) = p.trim_audio.session(id) else {
        return http::json(200, obj! { "percent" => 0.0, "status" => "expired" });
    };
    // The session knows its duration; the query parameter serves older clients.
    let total = if session.total_duration != 0.0 && !session.total_duration.is_nan() {
        session.total_duration
    } else {
        match query.get("totalDuration") {
            Some(QueryValue::One(v)) => js::parse_float(v),
            Some(QueryValue::Many(all)) => js::parse_float(&all.join(",")),
            Some(QueryValue::Nested) | None => f64::NAN,
        }
    };
    if total == 0.0 || total.is_nan() || total <= 0.0 {
        return http::error(400, "totalDuration is required");
    }
    match session.status {
        Status::Done => {
            return http::json(
                200,
                obj! {
                    "percent" => 100.0,
                    "status" => "done",
                    "part1" => obj! {
                        "filename" => session.part_name(1),
                        "duration" => session.trim_point,
                        "sizeBytes" => session.part1_size.unwrap_or(0),
                    },
                    "part2" => obj! {
                        "filename" => session.part_name(2),
                        "duration" => total - session.trim_point,
                        "sizeBytes" => session.part2_size.unwrap_or(0),
                    },
                },
            );
        }
        Status::Failed => {
            return http::json(
                200,
                obj! { "percent" => 0.0, "status" => "failed", "error" => session.error.as_deref().unwrap_or(TRIM_FAILED) },
            );
        }
        Status::Idle | Status::Processing => {}
    }
    let p1 = media::progress_file_value(&prog_path(p, id, 1), "out_time_ms=").await;
    let p2 = media::progress_file_value(&prog_path(p, id, 2), "out_time_ms=").await;
    let percent = tools::trim_percent(p1, p2, session.trim_point, total).unwrap_or(0.0);
    http::json(200, obj! { "percent" => percent, "status" => "processing" })
}

/// `markDownloaded`: both parts, or the ZIP, end the session at once.
async fn mark_downloaded(p: &Processor, id: &str, part: &str) {
    let mut cleanup = false;
    let exists = p.trim_audio.store.update(id, |s| {
        match part {
            "part1" => s.downloaded_part1 = true,
            "part2" => s.downloaded_part2 = true,
            _ => s.downloaded_zip = true,
        }
        cleanup = s.downloaded_zip || (s.downloaded_part1 && s.downloaded_part2);
    });
    if exists && cleanup {
        log::info("All files downloaded — triggering immediate cleanup", json!({ "sessionId": id }));
        p.trim_audio.cleanup(id, p.work_dir()).await;
    }
}

const EXPIRED: &str = "Your session has expired. Please upload your file again.";

async fn download(p: &Arc<Processor>, query: &Query) -> Response {
    if !query.truthy("sessionId") || !query.truthy("part") {
        return http::error(400, "sessionId and part are required");
    }
    let part = match query.text("part") {
        Some("1") => 1u8,
        Some("2") => 2u8,
        _ => return http::error(400, "part must be 1 or 2"),
    };
    let Some((id, session)) = query.text("sessionId").and_then(|id| Some((id.to_string(), p.trim_audio.session(id)?)))
    else {
        return http::error(410, EXPIRED);
    };
    let path = if part == 1 { session.part1_path.clone() } else { session.part2_path.clone() };
    let Some(path) = path else {
        return http::error(404, "File not found. Please trim your file first.");
    };
    let filename = session.part_name(part);
    let processor = p.clone();
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            mark_downloaded(&processor, &id, if part == 1 { "part1" } else { "part2" }).await;
            log::info(&format!("Part {part} downloaded"), json!({ "sessionId": id }));
        });
    });
    send_file(
        FileResponse {
            path: &path,
            content_type: FAMILY.content_type(&session.extension),
            filename: Some(&filename),
            cache_control: None,
            missing: "File not found or already deleted.",
            unreadable: "Download failed.",
        },
        Some(on_end),
    )
    .await
}

const ZIP_FAILED: &str = "Something went wrong. Please try again.";

async fn download_zip(p: &Arc<Processor>, query: &Query) -> Response {
    if !query.truthy("sessionId") {
        return http::error(400, "sessionId is required");
    }
    let Some((id, session)) = query.text("sessionId").and_then(|id| Some((id.to_string(), p.trim_audio.session(id)?)))
    else {
        return http::error(410, EXPIRED);
    };
    let (Some(part1), Some(part2)) = (session.part1_path.clone(), session.part2_path.clone()) else {
        return http::error(404, "Files not found. Please trim your file first.");
    };
    let built = async {
        let mut entries = Vec::new();
        let mut files = Vec::new();
        for (n, path) in [(1, &part1), (2, &part2)] {
            files.push(tokio::fs::File::open(path).await?);
            let (crc, size) = zip::crc32_file(path).await?;
            entries.push(zip::Entry { name: session.part_name(n), crc, size });
        }
        let layout = zip::layout(&entries, SystemTime::now())
            .ok_or_else(|| std::io::Error::other("archive too large for a stored ZIP"))?;
        Ok::<_, std::io::Error>((layout, files))
    }
    .await;
    let (layout, files) = match built {
        Ok(built) => built,
        Err(e) => {
            log::error("ZIP download error", json!({ "sessionId": id }), &e);
            return http::error(500, ZIP_FAILED);
        }
    };
    let mut segments = Vec::new();
    for (header, file) in layout.local_headers.into_iter().zip(files) {
        segments.push(Segment::Bytes(header));
        segments.push(Segment::File(file));
    }
    segments.push(Segment::Bytes(layout.central_directory));
    let processor = p.clone();
    let session_id = id.clone();
    let on_end: Box<dyn FnOnce() + Send> = Box::new(move || {
        tokio::spawn(async move {
            mark_downloaded(&processor, &session_id, "zip").await;
            log::info("ZIP downloaded — session cleaned up", json!({ "sessionId": session_id }));
        });
    });
    http::stream(
        &[
            (header::CONTENT_TYPE, "application/zip".into()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}_trimmed.zip\"", session.original_name)),
        ],
        Streamed::new(layout.total_len, segments, Some(on_end)),
    )
}

async fn invalidate(p: &Processor, inc: Incoming) -> Response {
    let (body, _) = match super::json_body(p, inc).await {
        Ok(found) => found,
        Err(error) => return http::body_error(error),
    };
    let session_arg = http::arg(&body, "sessionId");
    if !session_arg.truthy() {
        return http::error(400, "sessionId is required");
    }
    if let tools::Arg::Text(id) = session_arg {
        p.trim_audio.cleanup(id, p.work_dir()).await;
    }
    log::info("Session invalidated by user", json!({ "sessionId": body.get("sessionId") }));
    http::json(200, obj! { "success" => true })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From ffprobe's own output to the cut: only usable B-frame metadata moves
    /// a copied part 1 back by the reorder delay (FT-QA-05); metadata read the
    /// wrong way round, unreadable or missing leaves the copy uncorrected, as
    /// in the source; a cut off a keyframe re-encodes without consulting it.
    #[test]
    fn probed_metadata_decides_the_copy_boundary_and_its_seam_correction() {
        let packets = "0.000000,K__\n0.033333,___\n8.333333,K__\n12.5,K__\nN/A,K__\n";
        let cut = |t: f64, seam: Option<&str>| plan_video_cut(t, keyframe_from_packets(packets, t), seam);
        let uncorrected = VideoCut::Copy { part1_limit: 8.333333, part2_start: 8.333333 };

        assert_eq!(
            cut(8.34, Some("2,30/1\n")),
            VideoCut::Copy { part1_limit: 8.333333 - 2.0 / 30.0, part2_start: 8.333333 },
            "a cut within the epsilon of a keyframe copies, backed off by two frames at 30 fps"
        );
        for seam in [Some("30/1,2"), Some("N/A,N/A"), Some(""), None] {
            assert_eq!(cut(8.333333, seam), uncorrected, "{seam:?}");
        }
        assert_eq!(cut(5.0, Some("2,30/1")), VideoCut::Reencode, "the previous keyframe is at 0 s");
        assert_eq!(plan_video_cut(5.0, keyframe_from_packets("", 5.0), Some("2,30/1")), VideoCut::Reencode);
    }

    #[test]
    fn waveform_peaks_are_absolute_maxima_of_equal_blocks_capped_at_one_however_the_bytes_arrive() {
        let samples: Vec<f32> = vec![0.25, -0.5, 2.0, 0.1, -0.05, 0.0, 0.9];
        let raw: Vec<u8> = samples.iter().flat_map(|f| f.to_le_bytes()).collect();
        for piece in [1, 3, 4, 5, raw.len()] {
            let mut peaks = Peaks::new(raw.len() as u64, 3);
            for chunk in raw.chunks(piece) {
                peaks.feed(chunk);
            }
            // Seven samples in three blocks of two: the seventh is past the last block.
            assert_eq!(peaks.finish(), vec![0.5, 1.0, 0.05000000074505806], "pieces of {piece}");
        }
        let mut few = Peaks::new(8, 3);
        few.feed(&raw[..8]);
        assert_eq!(few.finish(), vec![0.0, 0.0, 0.0], "fewer samples than blocks");
    }
}
