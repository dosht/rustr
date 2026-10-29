//! What an Android or iOS shell provides, and the driver `Platform` built on it.
//!
//! `NativePorts` is a foreign-implemented interface: Kotlin and Swift implement it
//! once. Its methods are plain synchronous calls and the Rust side runs every
//! slow one (file measuring, PUT, clipboard, share) on a blocking thread, never on
//! the driver thread, so a slow shell cannot stall timers or other effects. A
//! failure is reported as its failure value (`false`, `None`); a foreign
//! exception is a bug in the shell.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use billing_core::presentation::confirm::ConfirmPayload;
use tokio::sync::mpsc;
use transgate_client::auth::{AuthStorage, LockGuard, SessionLock};
use transgate_client::checkout::CheckoutForm;
use transgate_client::driver::{LocalFuture, Platform, ShareOutcome};
use transgate_client::future::BoxFuture;
use transgate_core::jobs::app::CalendarContext;
use transgate_core::presentation::telemetry::TelemetryEvent;

/// The zone offsets the history window needs (minutes east of UTC): now, and at the
/// same local time one calendar month earlier.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarOffsets {
    pub now_minutes: i32,
    pub then_minutes: i32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareResult {
    Shared,
    Dismissed,
    Unsupported,
}

/// Upload progress reported by the shell while its PUT runs (0..=100).
#[derive(uniffi::Object)]
pub struct ProgressSink {
    tx: mpsc::UnboundedSender<u8>,
}

#[uniffi::export]
impl ProgressSink {
    pub fn report(&self, percent: u8) {
        let _ = self.tx.send(percent.min(100));
    }
}

#[uniffi::export(with_foreign)]
pub trait NativePorts: Send + Sync {
    // ---- session vault (Keystore-wrapped preferences, Keychain) ---------------------------
    fn storage_get(&self, key: String) -> Option<String>;
    fn storage_set(&self, key: String, value: String);
    fn storage_remove(&self, key: String);

    // ---- location ----------------------------------------------------------------------------
    /// In-app navigation requested by core (a history entry or a replacement).
    fn navigate(&self, path: String, replace: bool);
    /// Leave the app: Google authorization or a locale switch, in a Custom Tab or
    /// an authentication session.
    fn leave(&self, url: String);
    fn calendar(&self) -> CalendarOffsets;

    // ---- files and media -----------------------------------------------------------------
    /// Seconds, or `None` when the file could not be measured.
    fn measure(&self, file: u64) -> Option<f64>;
    /// One PUT of the held file; `true` only when the host answered 200.
    fn upload(&self, file: u64, url: String, progress: Arc<ProgressSink>) -> bool;
    fn release(&self, file: u64);
    /// Pauses the mounted player; whether it was playing before.
    fn pause_player(&self) -> bool;
    fn resume_player(&self);
    fn seek_player(&self, seconds: f64);

    // ---- output ------------------------------------------------------------------------------
    fn copy(&self, text: String, html: Option<String>) -> bool;
    fn save(&self, bytes: Vec<u8>, filename: String, mime: String) -> bool;
    fn preview(&self, bytes: Vec<u8>, filename: String, mime: String) -> bool;
    fn share(&self, bytes: Vec<u8>, filename: String, mime: String) -> ShareResult;

    // ---- bundled document assets (D10): fonts and the invoice logo, by file name ----------
    fn asset(&self, name: String) -> Option<Vec<u8>>;
}

/// Session vault over the shell's secure storage.
pub(crate) struct PortStorage(pub Arc<dyn NativePorts>);

impl AuthStorage for PortStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.0.storage_get(key.to_string())
    }
    fn set(&self, key: &str, value: &str) {
        self.0.storage_set(key.to_string(), value.to_string());
    }
    fn remove(&self, key: &str) {
        self.0.storage_remove(key.to_string());
    }
}

/// One process, one lock: refreshes of this process never overlap.
pub(crate) struct ProcessLock(Arc<tokio::sync::Mutex<()>>);

impl Default for ProcessLock {
    fn default() -> Self {
        static LOCK: std::sync::OnceLock<Arc<tokio::sync::Mutex<()>>> = std::sync::OnceLock::new();
        Self(LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))).clone())
    }
}

impl SessionLock for ProcessLock {
    fn acquire(&self) -> BoxFuture<'_, Result<LockGuard, String>> {
        let mutex = self.0.clone();
        Box::pin(async move {
            let held = mutex.lock_owned().await;
            Ok(LockGuard::new(move || drop(held)))
        })
    }
}

/// The driver's `Platform` for a native shell. Purchases, telemetry and the
/// provider return page do not exist natively (D6, D14): their ports do nothing.
pub(crate) struct NativePlatform {
    pub ports: Arc<dyn NativePorts>,
    pub user_agent: String,
}

/// Runs a shell call on a blocking thread and yields its answer, or `fallback` if the thread died.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, fallback: T) -> T {
    tokio::task::spawn_blocking(work).await.unwrap_or(fallback)
}

impl Platform for NativePlatform {
    fn spawn(&self, work: LocalFuture<'static, ()>) {
        tokio::task::spawn_local(work);
    }

    fn sleep(&self, ms: u64) -> LocalFuture<'static, ()> {
        Box::pin(tokio::time::sleep(Duration::from_millis(ms)))
    }

    fn now_ms(&self) -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
    }

    fn calendar(&self) -> CalendarContext {
        let offsets = self.ports.calendar();
        CalendarContext { offset_now_minutes: offsets.now_minutes, offset_then_minutes: offsets.then_minutes }
    }

    fn navigate(&self, path: &str, replace: bool) {
        self.ports.navigate(path.to_string(), replace);
    }

    fn leave(&self, url: &str) {
        self.ports.leave(url.to_string());
    }

    fn confirmation(&self) -> Option<ConfirmPayload> {
        None
    }

    fn measure<'a>(&'a self, file: u64) -> LocalFuture<'a, Option<f64>> {
        let ports = self.ports.clone();
        Box::pin(blocking(move || ports.measure(file), None))
    }

    fn upload<'a>(&'a self, file: u64, url: &'a str, mut progress: Box<dyn FnMut(u8)>) -> LocalFuture<'a, bool> {
        let ports = self.ports.clone();
        let url = url.to_string();
        Box::pin(async move {
            let (tx, mut ticks) = mpsc::unbounded_channel();
            let sink = Arc::new(ProgressSink { tx });
            let mut put = tokio::task::spawn_blocking(move || ports.upload(file, url, sink));
            let ok = loop {
                tokio::select! {
                    Some(percent) = ticks.recv() => progress(percent),
                    done = &mut put => break done.unwrap_or(false),
                }
            };
            // Ticks queued before the PUT returned still reach core, in order.
            while let Ok(percent) = ticks.try_recv() {
                progress(percent);
            }
            ok
        })
    }

    fn release(&self, file: u64) {
        self.ports.release(file);
    }

    fn pause_player(&self) -> bool {
        self.ports.pause_player()
    }

    fn resume_player<'a>(&'a self) -> LocalFuture<'a, ()> {
        let ports = self.ports.clone();
        Box::pin(blocking(move || ports.resume_player(), ()))
    }

    fn seek_player(&self, seconds: f64) {
        self.ports.seek_player(seconds);
    }

    fn copy<'a>(&'a self, text: &'a str, html: Option<&'a str>) -> LocalFuture<'a, bool> {
        let ports = self.ports.clone();
        let (text, html) = (text.to_string(), html.map(str::to_string));
        Box::pin(blocking(move || ports.copy(text, html), false))
    }

    fn download(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.ports.save(bytes.to_vec(), filename.to_string(), mime.to_string())
    }

    fn preview_document(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.ports.preview(bytes.to_vec(), filename.to_string(), mime.to_string())
    }

    fn share<'a>(&'a self, bytes: &'a [u8], filename: &'a str, mime: &'a str) -> LocalFuture<'a, ShareOutcome> {
        let ports = self.ports.clone();
        let (bytes, filename, mime) = (bytes.to_vec(), filename.to_string(), mime.to_string());
        Box::pin(async move {
            match blocking(move || ports.share(bytes, filename, mime), ShareResult::Unsupported).await {
                ShareResult::Shared => ShareOutcome::Shared,
                ShareResult::Dismissed => ShareOutcome::Dismissed,
                ShareResult::Unsupported => ShareOutcome::Unsupported,
            }
        })
    }

    /// No purchases on mobile (D6): nothing is ever submitted.
    fn submit_checkout(&self, _form: &CheckoutForm) -> bool {
        false
    }

    fn analytics(&self, _event: &TelemetryEvent) {}

    fn track_value(&self, _event: &str, _properties: &serde_json::Value) {}

    fn telemetry_reset(&self) {}

    fn stored_attribution(&self) -> Option<String> {
        None
    }

    fn user_agent(&self) -> String {
        self.user_agent.clone()
    }
}
