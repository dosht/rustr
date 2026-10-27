//! What the shared effect driver needs from the shell it runs in. The driver is
//! single threaded by construction (`Rc`, `RefCell`): every method below is called
//! on the one thread that owns the driver, and the futures it returns are polled
//! there. A shell with a threaded runtime keeps the driver on one thread (a
//! current-thread runtime with a `LocalSet`, as the browser's microtask queue is).
//!
//! Ports carry capabilities only. Which effect runs, when, for whom and what its
//! answer means stay in core and in the driver's fences.
use std::future::Future;
use std::pin::Pin;

use billing_core::presentation::confirm::ConfirmPayload;
use serde_json::Value;
use transgate_core::jobs::app::CalendarContext;
use transgate_core::presentation::telemetry::TelemetryEvent;

use crate::checkout::CheckoutForm;

/// A local (not `Send`) future, as the browser and a `LocalSet` poll them.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Outcome of the phone share sheet; `Dismissed` must not fall through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareOutcome {
    Shared,
    Dismissed,
    Unsupported,
}

pub trait Platform {
    // ---- execution ---------------------------------------------------------------------

    /// Runs `work` to completion on the driver's thread.
    fn spawn(&self, work: LocalFuture<'static, ()>);
    /// A delay of `ms` milliseconds.
    fn sleep(&self, ms: u64) -> LocalFuture<'static, ()>;
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> f64;
    /// The zone offsets the history window needs (east positive): now, and at the
    /// same local time one calendar month earlier.
    fn calendar(&self) -> CalendarContext;

    // ---- location ----------------------------------------------------------------------

    /// In-app navigation: one history entry, or a replacement.
    fn navigate(&self, path: &str, replace: bool);
    /// Leaves the app (Google authorization, a locale switch, the public root).
    fn leave(&self, url: &str);
    /// The host-embedded confirmation payload of the provider return page, if any.
    fn confirmation(&self) -> Option<ConfirmPayload>;

    // ---- files and media ---------------------------------------------------------------

    /// Seconds, or `None` when the file could not be measured.
    fn measure<'a>(&'a self, file: u64) -> LocalFuture<'a, Option<f64>>;
    /// One PUT of the held file. `true` only when the host answered 200.
    fn upload<'a>(&'a self, file: u64, url: &'a str, progress: Box<dyn FnMut(u8)>) -> LocalFuture<'a, bool>;
    /// Forgets a picked file.
    fn release(&self, file: u64);
    /// Pauses the mounted player; answers whether it was playing before.
    fn pause_player(&self) -> bool;
    fn resume_player<'a>(&'a self) -> LocalFuture<'a, ()>;
    fn seek_player(&self, seconds: f64);

    // ---- output ------------------------------------------------------------------------

    fn copy<'a>(&'a self, text: &'a str, html: Option<&'a str>) -> LocalFuture<'a, bool>;
    /// Saves bytes; `false` when nothing was saved.
    fn download(&self, bytes: &[u8], filename: &str, mime: &str) -> bool;
    /// Installs bytes in the invoice modal; never substitutes a download.
    fn preview_document(&self, bytes: &[u8], filename: &str, mime: &str) -> bool;
    fn share<'a>(&'a self, bytes: &'a [u8], filename: &'a str, mime: &'a str) -> LocalFuture<'a, ShareOutcome>;
    /// D17: posts the checkout form; `false` means nothing was submitted.
    fn submit_checkout(&self, form: &CheckoutForm) -> bool;

    // ---- telemetry and attribution ------------------------------------------------------

    /// A core analytics event; the shell drops it without consent.
    fn analytics(&self, event: &TelemetryEvent);
    /// A billing analytics event whose properties billing-core built as JSON.
    fn track_value(&self, event: &str, properties: &Value);
    fn telemetry_reset(&self);
    /// The stored first-touch record, if the shell keeps one.
    fn stored_attribution(&self) -> Option<String>;
    fn user_agent(&self) -> String;
}
