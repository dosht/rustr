//! The `NativePorts` a terminal can provide. Ports carry capabilities only: the
//! keyring, a measured duration, one PUT, a saved file, a URL handed to the
//! user. Which effect runs and what its answer means stay in core and the driver.
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use transgate_core::presentation::upload_queue::PickedFile;
use transgate_ffi::{CalendarOffsets, NativePorts, ProgressSink, ShareResult};

use crate::shell::Signal;
use crate::vault::Vault;

/// Receives the URL core asks the user to open (Google authorization, a locale page).
pub type OpenUrl = Box<dyn Fn(&str) + Send + Sync>;

struct Held {
    path: PathBuf,
    mime: String,
}

pub struct TerminalPorts {
    pub(crate) vault: Arc<Vault>,
    held: Mutex<HashMap<u64, Held>>,
    next_handle: AtomicU64,
    out_dir: PathBuf,
    open_url: OpenUrl,
    saved: Mutex<Vec<PathBuf>>,
    save_error: Mutex<Option<String>>,
    signal: Arc<Signal>,
}

impl TerminalPorts {
    pub fn new(vault: Arc<Vault>, out_dir: impl Into<PathBuf>, open_url: OpenUrl, signal: Arc<Signal>) -> TerminalPorts {
        TerminalPorts {
            vault,
            held: Mutex::default(),
            next_handle: AtomicU64::new(1),
            out_dir: out_dir.into(),
            open_url,
            saved: Mutex::default(),
            save_error: Mutex::default(),
            signal,
        }
    }

    /// Holds a file for measuring and uploading, as a picker would.
    pub fn pick(&self, path: &Path) -> std::io::Result<PickedFile> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a file"));
        }
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mime = mime_of(path).to_string();
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.held.lock().expect("the file lock is never poisoned").insert(handle, Held { path: path.to_path_buf(), mime: mime.clone() });
        Ok(PickedFile { handle, name, size: i64::try_from(metadata.len()).unwrap_or(i64::MAX), mime })
    }

    /// Files the driver saved, in order.
    pub fn saved(&self) -> Vec<PathBuf> {
        self.saved.lock().expect("the saved lock is never poisoned").clone()
    }

    pub fn take_save_error(&self) -> Option<String> {
        self.save_error.lock().expect("the save lock is never poisoned").take()
    }

    fn path_of(&self, file: u64) -> Option<(PathBuf, String)> {
        self.held.lock().expect("the file lock is never poisoned").get(&file).map(|h| (h.path.clone(), h.mime.clone()))
    }
}

/// The type a PUT declares: the browser sends the file's type, or the generic one.
fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("ogg" | "oga") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

/// Seconds of media, from `ffprobe` when it is installed; `None` otherwise (the
/// deferred-charge path of core handles an unknown duration).
fn probe_seconds(path: &Path) -> Option<f64> {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()?.trim().parse::<f64>().ok().filter(|s| s.is_finite() && *s >= 0.0)
}

/// Reads the file and reports whole-percent progress as the PUT consumes it.
struct Progressive {
    file: std::fs::File,
    total: u64,
    sent: u64,
    last: u8,
    sink: Arc<ProgressSink>,
}

impl Read for Progressive {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buf)?;
        self.sent += n as u64;
        let percent = (self.sent.saturating_mul(100) / self.total.max(1)).min(100) as u8;
        if percent != self.last {
            self.last = percent;
            self.sink.report(percent);
        }
        Ok(n)
    }
}

fn put(path: &Path, mime: &str, url: &str, sink: Arc<ProgressSink>) -> Result<bool, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();
    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let body = reqwest::blocking::Body::sized(Progressive { file, total, sent: 0, last: 0, sink }, total);
    let response = client.put(url).header(reqwest::header::CONTENT_TYPE, mime).body(body).send()?;
    Ok(response.status().as_u16() == 200)
}

/// The zone offsets the history window needs (minutes east of UTC): now, and at the
/// same local time one calendar month earlier.
#[cfg(unix)]
pub fn local_offsets() -> CalendarOffsets {
    // SAFETY: `localtime_r` and `mktime` only read and write the structs passed here; `tm` is
    // plain data and a zeroed one is a valid input to both.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return CalendarOffsets { now_minutes: 0, then_minutes: 0 };
        }
        let now_minutes = (tm.tm_gmtoff / 60) as i32;
        let mut earlier = tm;
        earlier.tm_mon -= 1;
        earlier.tm_isdst = -1;
        let then = libc::mktime(&mut earlier);
        let mut then_tm: libc::tm = std::mem::zeroed();
        if then == -1 || libc::localtime_r(&then, &mut then_tm).is_null() {
            return CalendarOffsets { now_minutes, then_minutes: now_minutes };
        }
        CalendarOffsets { now_minutes, then_minutes: (then_tm.tm_gmtoff / 60) as i32 }
    }
}

#[cfg(not(unix))]
pub fn local_offsets() -> CalendarOffsets {
    CalendarOffsets { now_minutes: 0, then_minutes: 0 }
}

impl NativePorts for TerminalPorts {
    fn storage_get(&self, key: String) -> Option<String> {
        self.vault.get(&key)
    }

    fn storage_set(&self, key: String, value: String) {
        self.vault.set(&key, &value);
    }

    fn storage_remove(&self, key: String) {
        self.vault.remove(&key);
    }

    fn navigate(&self, _path: String, _replace: bool) {}

    fn leave(&self, url: String) {
        (self.open_url)(&url);
    }

    fn calendar(&self) -> CalendarOffsets {
        local_offsets()
    }

    fn measure(&self, file: u64) -> Option<f64> {
        probe_seconds(&self.path_of(file)?.0)
    }

    fn upload(&self, file: u64, url: String, progress: Arc<ProgressSink>) -> bool {
        let Some((path, mime)) = self.path_of(file) else { return false };
        put(&path, &mime, &url, progress).unwrap_or(false)
    }

    fn release(&self, file: u64) {
        self.held.lock().expect("the file lock is never poisoned").remove(&file);
    }

    fn pause_player(&self) -> bool {
        false
    }

    fn resume_player(&self) {}

    fn seek_player(&self, _seconds: f64) {}

    fn copy(&self, _text: String, _html: Option<String>) -> bool {
        false
    }

    fn save(&self, bytes: Vec<u8>, filename: String, _mime: String) -> bool {
        // A file name never selects a directory: only its last component is used.
        let Some(name) = Path::new(&filename).file_name() else { return false };
        let target = self.out_dir.join(name);
        let result = std::fs::create_dir_all(&self.out_dir).and_then(|()| std::fs::write(&target, bytes));
        match result {
            Ok(()) => {
                self.saved.lock().expect("the saved lock is never poisoned").push(target);
                self.signal.notify();
                true
            }
            Err(error) => {
                *self.save_error.lock().expect("the save lock is never poisoned") = Some(format!("{}: {error}", target.display()));
                self.signal.notify();
                false
            }
        }
    }

    fn preview(&self, _bytes: Vec<u8>, _filename: String, _mime: String) -> bool {
        false
    }

    fn share(&self, _bytes: Vec<u8>, _filename: String, _mime: String) -> ShareResult {
        ShareResult::Unsupported
    }

    fn asset(&self, _name: String) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(dir: &Path) -> TerminalPorts {
        TerminalPorts::new(Arc::new(Vault::file(dir.join("s.json"))), dir.join("out"), Box::new(|_| {}), Arc::new(Signal::default()))
    }

    #[test]
    fn save_keeps_to_the_out_dir_and_reports_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let ports = ports(dir.path());
        assert!(ports.save(b"zip".to_vec(), "../../escape.zip".into(), "application/zip".into()));
        assert_eq!(ports.saved(), vec![dir.path().join("out/escape.zip")]);
        assert_eq!(std::fs::read(dir.path().join("out/escape.zip")).unwrap(), b"zip");
        assert!(!ports.save(vec![], "..".into(), String::new()), "a name without a file component saves nothing");
    }

    #[test]
    fn pick_describes_the_file_and_release_forgets_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Talk.M4A");
        std::fs::write(&path, b"abc").unwrap();
        let ports = ports(dir.path());
        let picked = ports.pick(&path).unwrap();
        assert_eq!((picked.name.as_str(), picked.size, picked.mime.as_str()), ("Talk.M4A", 3, "audio/mp4"));
        assert!(ports.pick(dir.path()).is_err(), "a directory is not a file");
        ports.release(picked.handle);
        assert_eq!(ports.measure(picked.handle), None, "a released handle measures nothing");
    }
}
