//! The seven tool routers. Each family owns its sessions, allowance and files;
//! this module holds what every router does the same way.

pub mod compress_video;
pub mod convert_audio;
pub mod convert_video;
pub mod reduce_audio;
pub mod remove_noise;
pub mod trim_audio;
pub mod video_to_audio;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use axum::http::header;
use axum::response::Response;
use serde_json::{Map, Value, json};
use transgate_core::tools::{AcceptedUpload, Family, ProcessingAllowance, UploadRefusal, UploadedFile};

use crate::Processor;
use crate::http::{self, BodyError, Incoming, Segment, Streamed};
use crate::log;
use crate::store::{Session, Store};
use crate::upload::{self, Received, UploadError};

/// Sessions and the processing allowance of one family.
pub struct Tool<S> {
    pub family: Family,
    pub store: Store<S>,
    allowance: Mutex<ProcessingAllowance>,
    workers: Arc<Mutex<HashMap<String, SessionWork>>>,
}

#[derive(Default)]
struct SessionWork {
    workers: Vec<Worker>,
    closing: Option<tokio::sync::watch::Receiver<bool>>,
}

struct Worker {
    cancel: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl<S: Session> Tool<S> {
    pub fn new(family: Family) -> Self {
        Tool {
            family,
            store: Store::default(),
            allowance: Mutex::new(ProcessingAllowance::new(family.processing_per_hour())),
            workers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn allow(&self, ip: &str) -> bool {
        self.allowance.lock().unwrap().check(ip, Instant::now())
    }

    /// Register before dispatch under the same lock as invalidation. A removed
    /// or closing session can never dispatch a late encode/thumbnail worker.
    pub(crate) fn spawn(&self, id: String, work: impl Future<Output = ()> + Send + 'static) -> bool {
        let mut sessions = self.workers.lock().unwrap();
        if !self.store.with(&id, |s| s.is_some()) || sessions.get(&id).is_some_and(|s| s.closing.is_some()) {
            return false;
        }
        let (cancel, cancelled) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(crate::media::with_cancellation(cancelled, work));
        sessions.entry(id).or_default().workers.push(Worker { cancel, task });
        true
    }

    /// Fence dispatch first, then cancel and reap every owned worker before
    /// deleting its files, including output paths not yet published in state.
    /// Cleanup itself is detached so a lost DELETE/download connection cannot
    /// abandon it. Concurrent cleanup callers await the same completion.
    pub async fn cleanup(&self, id: &str, work_dir: &Path) {
        let mut done = {
            let mut sessions = self.workers.lock().unwrap();
            if let Some(done) = sessions.get(id).and_then(|s| s.closing.clone()) {
                done
            } else {
                let snapshot = self.store.get(id);
                if snapshot.is_none() && !sessions.contains_key(id) {
                    return;
                }
                let session = sessions.entry(id.to_string()).or_default();
                let workers = std::mem::take(&mut session.workers);
                let (finished, done) = tokio::sync::watch::channel(false);
                session.closing = Some(done.clone());
                self.store.delete(id);
                let files = snapshot.map_or_else(Vec::new, |s| s.files(id, work_dir));
                let (registry, id, dir, prefix) = (self.workers.clone(), id.to_string(), work_dir.to_path_buf(), self.family.file_prefix());
                tokio::spawn(async move {
                    for worker in &workers {
                        worker.cancel.send_replace(true);
                    }
                    for worker in workers {
                        let _ = worker.task.await;
                    }
                    for file in files {
                        let _ = tokio::fs::remove_file(file).await;
                    }
                    // Names are minted by this session, not arbitrary uploaded
                    // basenames: trim parts, encodes, thumbnails and progress.
                    let stem = format!("{prefix}{id}_");
                    let progress = format!("{prefix}{id}.prog");
                    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                        while let Ok(Some(entry)) = entries.next_entry().await {
                            let name = entry.file_name();
                            let name = name.to_string_lossy();
                            if name.starts_with(&stem) || name == progress {
                                let _ = tokio::fs::remove_file(entry.path()).await;
                            }
                        }
                    }
                    let mut sessions = registry.lock().unwrap();
                    finished.send_replace(true);
                    sessions.remove(&id);
                });
                done
            }
        };
        let _ = done.wait_for(|finished| *finished).await;
    }

    /// `purgeExpiredSessions`.
    pub async fn purge_expired(&self, now: SystemTime, work_dir: &Path) -> usize {
        let expired = self.store.expired(now, self.family.session_ttl());
        for id in &expired {
            self.cleanup(id, work_dir).await;
        }
        self.allowance.lock().unwrap().forget_expired(Instant::now());
        expired.len()
    }

    pub fn session(&self, id: &str) -> Option<S> {
        self.store.get(id)
    }
}

/// `purgeOrphanedFiles`: files with the family prefix modified more than the
/// family TTL ago, whether or not a session still names them.
pub async fn purge_orphans(family: Family, now: SystemTime, work_dir: &Path) -> usize {
    let Ok(mut entries) = tokio::fs::read_dir(work_dir).await else { return 0 };
    let mut purged = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !entry.file_name().to_string_lossy().starts_with(family.file_prefix()) {
            continue;
        }
        let Ok(modified) = entry.metadata().await.and_then(|m| m.modified()) else { continue };
        if now.duration_since(modified).is_ok_and(|age| age > family.session_ttl())
            && tokio::fs::remove_file(entry.path()).await.is_ok()
        {
            purged += 1;
        }
    }
    purged
}

pub fn upload_path(p: &Processor, family: Family, id: &str, ext: &str) -> PathBuf {
    p.work_dir().join(format!("{}{id}_upload.{ext}", family.file_prefix()))
}

pub fn output_path(p: &Processor, family: Family, id: &str, ext: &str) -> PathBuf {
    p.work_dir().join(format!("{}{id}_output.{ext}", family.file_prefix()))
}

/// `/tmp/<prefix><id>.prog`.
pub fn progress_path(p: &Processor, family: Family, id: &str) -> PathBuf {
    p.work_dir().join(format!("{}{id}.prog", family.file_prefix()))
}

const UPLOAD_FAILED: &str = "Upload failed. Please check your connection and try again.";

/// An accepted upload, moved to the session's upload path.
pub struct Uploaded {
    pub id: String,
    pub accepted: AcceptedUpload,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug)]
pub enum UploadFailure {
    Timeout,
    Refused(u16, &'static str),
}

impl UploadFailure {
    pub fn response(self) -> Response {
        match self {
            Self::Timeout => http::request_timeout(),
            Self::Refused(status, message) => http::error(status, message),
        }
    }
}

/// The shared first half of every `POST /upload`: receive, check the format
/// and size, mint the session id and move the file into place.
pub async fn receive_upload(p: &Processor, family: Family, inc: Incoming) -> Result<Uploaded, UploadFailure> {
    let received = tokio::time::timeout(
        p.config.request_timeout,
        upload::receive(&inc.headers, inc.body, p.work_dir(), family.file_prefix(), family.max_upload_bytes()),
    )
    .await;
    let received: Received = match received {
        Err(_) => return Err(UploadFailure::Timeout),
        Ok(Err(UploadError::TooLarge)) => return Err(UploadFailure::Refused(400, family.upload_refusal(UploadRefusal::TooLarge))),
        Ok(Err(UploadError::Failed(e))) => {
            log::error("Upload handler error", json!({}), &e);
            return Err(UploadFailure::Refused(500, UPLOAD_FAILED));
        }
        Ok(Ok(None)) => return Err(UploadFailure::Refused(400, "No file provided")),
        Ok(Ok(Some(received))) => received,
    };
    let file = UploadedFile {
        original_filename: received.original_filename.as_deref(),
        mime: &received.mime,
        size: received.size,
    };
    let accepted = match family.accept_upload(&file) {
        Ok(accepted) => accepted,
        Err(refusal) => {
            let _ = tokio::fs::remove_file(&received.path).await;
            return Err(UploadFailure::Refused(400, family.upload_refusal(refusal)));
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let path = upload_path(p, family, &id, &accepted.extension);
    if let Err(e) = tokio::fs::rename(&received.path, &path).await {
        let _ = tokio::fs::remove_file(&received.path).await;
        log::error("Upload handler error", json!({}), &e);
        return Err(UploadFailure::Refused(500, UPLOAD_FAILED));
    }
    Ok(Uploaded { id, accepted, path, size: received.size })
}

/// `express.json()` within the request deadline.
pub async fn json_body(p: &Processor, inc: Incoming) -> Result<(Map<String, Value>, String), BodyError> {
    let ip = inc.ip;
    match tokio::time::timeout(p.config.request_timeout, http::json_body(&inc.headers, inc.body)).await {
        Ok(Ok(body)) => Ok((body, ip)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(BodyError::TimedOut),
    }
}

/// A file response; `missing` is the 404 message when the file is gone.
pub struct FileResponse<'a> {
    pub path: &'a Path,
    pub content_type: &'a str,
    /// `attachment; filename="..."`.
    pub filename: Option<&'a str>,
    pub cache_control: Option<&'a str>,
    pub missing: &'a str,
    /// The 500 message when the file cannot be opened.
    pub unreadable: &'a str,
}

pub async fn send_file(file: FileResponse<'_>, on_end: Option<Box<dyn FnOnce() + Send>>) -> Response {
    if tokio::fs::metadata(file.path).await.is_err() {
        return http::error(404, file.missing);
    }
    let opened = match tokio::fs::File::open(file.path).await {
        Ok(f) => f,
        Err(e) => {
            log::error("Download stream error", json!({}), &e);
            return http::error(500, file.unreadable);
        }
    };
    let len = match opened.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return http::error(500, file.unreadable),
    };
    let mut headers = vec![(header::CONTENT_TYPE, file.content_type.to_string())];
    if let Some(name) = file.filename {
        headers.push((header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")));
    }
    if let Some(cache) = file.cache_control {
        headers.push((header::CACHE_CONTROL, cache.to_string()));
    }
    http::stream(&headers, Streamed::new(len, vec![Segment::File(opened)], on_end))
}

/// The session id of a processing/progress/download request, for families
/// that validate it before anything else.
pub fn valid_id(value: Option<&str>) -> Option<&str> {
    value.filter(|id| transgate_core::tools::is_valid_session_id(id))
}

/// Megabytes as the source logs them.
pub fn size_mb(bytes: u64) -> f64 {
    transgate_core::tools::js_round(bytes as f64 / 1024.0 / 1024.0)
}

/// `ffprobe ... -of json` output parsed; `None` when it is not JSON.
pub fn parse_probe(stdout: &str) -> Option<Value> {
    serde_json::from_str(stdout).ok()
}

/// `value ? parseInt(value, 10) : null` then `x && !isNaN(x) ? x : null`.
pub fn probe_int(value: Option<&Value>) -> Option<f64> {
    if !crate::js::truthy(value) {
        return None;
    }
    crate::js::nonzero(crate::js::parse_int(&crate::js::string(value?)))
}

/// `value ? parseFloat(value) : null` then the same filter.
pub fn probe_float(value: Option<&Value>) -> Option<f64> {
    if !crate::js::truthy(value) {
        return None;
    }
    crate::js::nonzero(crate::js::parse_float(&crate::js::string(value?)))
}

/// `value ? Math.round(parseInt(value, 10) / 1000) : null` then the same filter.
pub fn probe_kbps(value: Option<&Value>) -> Option<f64> {
    if !crate::js::truthy(value) {
        return None;
    }
    let bps = crate::js::parse_int(&crate::js::string(value?));
    crate::js::nonzero(transgate_core::tools::js_round(bps / 1000.0))
}

/// `-loglevel error -i <in> -codec:a <codec> [-b:a <k>] -ar <rate> -ac <ch>
/// [-f <container>] [extra] -progress pipe:2 -y <out>` (convert and reduce).
pub fn audio_args(input: &Path, output: &Path, enc: &transgate_core::tools::AudioEncoding) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> =
        vec!["-loglevel".into(), "error".into(), "-i".into(), input.into(), "-codec:a".into(), enc.codec.into()];
    if let Some(kbps) = enc.bitrate_kbps {
        args.extend(["-b:a".into(), format!("{}k", crate::js::arg(kbps)).into()]);
    }
    args.extend(["-ar".into(), crate::js::arg(enc.sample_rate).into(), "-ac".into(), crate::js::arg(enc.channels).into()]);
    if let Some(container) = enc.container {
        args.extend(["-f".into(), container.into()]);
    }
    args.extend(enc.extra.iter().map(Into::into));
    args.extend(["-progress".into(), "pipe:2".into(), "-y".into(), output.into()]);
    args
}

/// `-loglevel error` output probe of the duration in microseconds, `None`
/// when it cannot be read (`getDurationUs`).
pub async fn duration_us(p: &Processor, path: &Path) -> Option<f64> {
    crate::media::probe_duration(&p.media, path, 1024 * 1024).await.ok().and_then(transgate_core::tools::duration_us)
}

/// Runs an encode that reports `out_time_us=` on stderr; `on_progress` sees
/// each increase and finally 100 on a clean exit.
pub async fn encode_with_stderr_progress(
    p: &Processor,
    args: &[std::ffi::OsString],
    total_us: Option<f64>,
    mut on_progress: impl FnMut(f64) + Send,
) -> Result<(), String> {
    let mut last = 0.0;
    let ended = crate::media::process(
        &p.media.ffmpeg,
        args,
        None,
        |line| {
            let Some(value) = line.trim().strip_prefix("out_time_us=") else { return };
            let value = crate::js::parse_int(value);
            if value.is_nan() {
                return;
            }
            if let Some(pct) = transgate_core::tools::stream_percent(value, total_us, last) {
                last = pct;
                on_progress(pct);
            }
        },
        || async {},
    )
    .await;
    match ended {
        crate::media::Ended::Success => {
            on_progress(100.0);
            Ok(())
        }
        crate::media::Ended::Failed(code) => Err(format!("FFmpeg exited with code {code:?}")),
        crate::media::Ended::SpawnFailed(e) => Err(e.to_string()),
        crate::media::Ended::TimedOut => Err("timed out".into()),
        crate::media::Ended::Cancelled => Err("session invalidated".into()),
    }
}

/// A detached FFmpeg child that writes a `-progress` file: on its ceiling it is
/// killed, its progress and output files are removed and `on_timeout` runs;
/// the progress file is always removed once it has ended.
pub async fn run_progress_file_child(
    p: &Processor,
    args: &[std::ffi::OsString],
    kill_after: Option<std::time::Duration>,
    prog: &Path,
    output: &Path,
    on_timeout: impl FnOnce() + Send,
) -> crate::media::Ended {
    let ended = crate::media::process(&p.media.ffmpeg, args, kill_after, |_| {}, || async move {
        let _ = tokio::fs::remove_file(prog).await;
        let _ = tokio::fs::remove_file(output).await;
        on_timeout();
    })
    .await;
    let _ = tokio::fs::remove_file(prog).await;
    ended
}

/// `-loglevel error -threads N -i <in>`, the start of every threaded command.
pub fn threaded_input(p: &Processor, input: &Path) -> Vec<std::ffi::OsString> {
    let [t, n] = p.media.thread_args();
    vec!["-loglevel".into(), "error".into(), t.into(), n.into(), "-i".into(), input.into()]
}

/// The first-frame thumbnail of video-to-audio and convert-video:
/// `-loglevel error -threads N -ss 00:00:01 -i <in> -vframes 1 -f image2 -y <png>`.
pub async fn png_thumbnail(p: &Processor, input: &Path, thumb: &Path) -> bool {
    let [t, n] = p.media.thread_args();
    let args: Vec<std::ffi::OsString> = vec![
        "-loglevel".into(),
        "error".into(),
        t.into(),
        n.into(),
        "-ss".into(),
        "00:00:01".into(),
        "-i".into(),
        input.into(),
        "-vframes".into(),
        "1".into(),
        "-f".into(),
        "image2".into(),
        "-y".into(),
        thumb.into(),
    ];
    crate::media::output(&p.media.ffmpeg, &args, usize::MAX, None).await.is_ok() && tokio::fs::metadata(thumb).await.is_ok()
}
