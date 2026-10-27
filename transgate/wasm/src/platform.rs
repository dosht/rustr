//! Typed calls into the shell's `Platform` object (`transgate/web/src/lib/core/platform.ts`)
//! and the browser implementation of the shared driver's `Platform` port.
//! Every result becomes a core event; a throwing or missing capability is
//! reported as its failure value, never as success.
use billing_core::presentation::confirm::ConfirmPayload;
use js_sys::{Date, Function, Object, Promise, Reflect, Uint8Array};
use transgate_client::attribution::UTM_STORAGE_KEY;
use transgate_client::auth::AuthStorage;
use transgate_client::checkout::CheckoutForm;
use transgate_client::driver::{LocalFuture, Platform as ShellPlatform, ShareOutcome};
use transgate_core::jobs::app::CalendarContext;
use transgate_core::presentation::telemetry::{Prop, TelemetryEvent};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::browser::{self, BrowserStorage};
use crate::tasks::sleep;

#[wasm_bindgen]
extern "C" {
    pub type Platform;

    #[wasm_bindgen(method, catch)]
    fn measure(this: &Platform, handle: f64) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn put(this: &Platform, handle: f64, url: &str, on_progress: &Function) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method)]
    fn forget(this: &Platform, handle: f64);
    #[wasm_bindgen(method)]
    fn player(this: &Platform) -> JsValue;
    #[wasm_bindgen(method, catch)]
    fn copy(this: &Platform, text: &str, html: JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn save(this: &Platform, bytes: &Uint8Array, filename: &str, mime: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn preview(this: &Platform, bytes: &Uint8Array, filename: &str, mime: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn share(this: &Platform, bytes: &Uint8Array, filename: &str, mime: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method)]
    fn track(this: &Platform, event: &str, properties: &Object);
    #[wasm_bindgen(method, js_name = resetTelemetry)]
    fn reset_telemetry(this: &Platform);

    pub type PlayerPort;

    #[wasm_bindgen(method, catch)]
    fn play(this: &PlayerPort) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method)]
    fn pause(this: &PlayerPort);
    #[wasm_bindgen(method)]
    fn seek(this: &PlayerPort, seconds: f64);
    #[wasm_bindgen(method, js_name = isPlaying)]
    fn is_playing(this: &PlayerPort) -> bool;
}

/// File handles are small integers assigned by the shell; JS numbers carry them exactly.
fn handle(value: u64) -> f64 {
    value as f64
}

impl Platform {
    /// Seconds, or `None` when both measurement methods failed.
    pub async fn measured(&self, file: u64) -> Option<f64> {
        let promise = self.measure(handle(file)).ok()?;
        JsFuture::from(promise).await.ok()?.as_f64().filter(|s| s.is_finite() && *s > 0.0)
    }

    /// One PUT of the held File. `true` only when the shell saw HTTP 200.
    pub async fn upload(&self, file: u64, url: &str, mut progress: impl FnMut(u8) + 'static) -> bool {
        let callback = Closure::<dyn FnMut(f64)>::new(move |percent: f64| progress(percent.clamp(0.0, 100.0) as u8));
        let Ok(promise) = self.put(handle(file), url, callback.as_ref().unchecked_ref()) else { return false };
        let answer = JsFuture::from(promise).await;
        drop(callback);
        answer.ok().and_then(|v| Reflect::get(&v, &JsValue::from_str("ok")).ok()).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    pub fn release(&self, file: u64) {
        self.forget(handle(file));
    }

    fn mounted_player(&self) -> Option<PlayerPort> {
        let player = self.player();
        (!player.is_null() && !player.is_undefined()).then(|| player.unchecked_into())
    }

    /// Pauses the mounted player; answers whether it was playing before.
    pub fn pause_player(&self) -> bool {
        let Some(player) = self.mounted_player() else { return false };
        let was_playing = player.is_playing();
        player.pause();
        was_playing
    }

    pub async fn resume_player(&self) {
        if let Some(Ok(promise)) = self.mounted_player().map(|p| p.play()) {
            let _ = JsFuture::from(promise).await;
        }
    }

    pub fn seek_player(&self, seconds: f64) {
        if let Some(player) = self.mounted_player() {
            player.seek(seconds);
        }
    }

    pub async fn copied(&self, text: &str, html: Option<&str>) -> bool {
        let html = html.map(JsValue::from_str).unwrap_or(JsValue::NULL);
        let Ok(promise) = self.copy(text, html) else { return false };
        JsFuture::from(promise).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
    }

    /// Copies the bytes into a JS buffer the shell may keep after this call.
    pub fn download(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.save(&Uint8Array::from(bytes), filename, mime).is_ok()
    }

    /// Installs bytes in the invoice modal; never substitutes a download.
    pub fn preview_document(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.preview(&Uint8Array::from(bytes), filename, mime).is_ok()
    }

    pub async fn shared(&self, bytes: &[u8], filename: &str, mime: &str) -> ShareOutcome {
        let Ok(promise) = self.share(&Uint8Array::from(bytes), filename, mime) else { return ShareOutcome::Unsupported };
        match JsFuture::from(promise).await.ok().and_then(|v| v.as_string()).as_deref() {
            Some("shared") => ShareOutcome::Shared,
            Some("dismissed") => ShareOutcome::Dismissed,
            _ => ShareOutcome::Unsupported,
        }
    }

    /// Forwards a core analytics event; the shell drops it without cookie consent.
    pub fn analytics(&self, event: &TelemetryEvent) {
        let properties = Object::new();
        for (key, value) in &event.properties {
            let value = match value {
                Prop::Bool(b) => JsValue::from_bool(*b),
                Prop::Number(n) => JsValue::from_f64(*n),
                Prop::Text(t) => JsValue::from_str(t),
            };
            let _ = Reflect::set(&properties, &JsValue::from_str(key), &value);
        }
        self.track(&event.name, &properties);
    }

    /// Billing analytics whose properties billing-core builds as JSON; `null`
    /// stays `null`. The shell drops it without cookie consent.
    pub fn track_value(&self, event: &str, properties: &serde_json::Value) {
        let Ok(object) = serde::Serialize::serialize(properties, &serde_wasm_bindgen::Serializer::json_compatible()) else { return };
        self.track(event, object.unchecked_ref());
    }

    pub fn telemetry_reset(&self) {
        self.reset_telemetry();
    }
}

/// The browser as the shared driver's `Platform`: the JS capability object plus
/// the page mechanics `browser.rs` owns.
pub struct WebPlatform(pub Platform);

impl ShellPlatform for WebPlatform {
    fn spawn(&self, work: LocalFuture<'static, ()>) {
        wasm_bindgen_futures::spawn_local(work);
    }

    fn sleep(&self, ms: u64) -> LocalFuture<'static, ()> {
        Box::pin(sleep(ms))
    }

    fn now_ms(&self) -> f64 {
        Date::now()
    }

    fn calendar(&self) -> CalendarContext {
        browser::calendar()
    }

    fn navigate(&self, path: &str, replace: bool) {
        browser::navigate(path, replace);
    }

    fn leave(&self, url: &str) {
        browser::assign(url);
    }

    fn confirmation(&self) -> Option<ConfirmPayload> {
        browser::confirmation()
    }

    fn measure<'a>(&'a self, file: u64) -> LocalFuture<'a, Option<f64>> {
        Box::pin(self.0.measured(file))
    }

    fn upload<'a>(&'a self, file: u64, url: &'a str, progress: Box<dyn FnMut(u8)>) -> LocalFuture<'a, bool> {
        Box::pin(self.0.upload(file, url, progress))
    }

    fn release(&self, file: u64) {
        self.0.release(file);
    }

    fn pause_player(&self) -> bool {
        self.0.pause_player()
    }

    fn resume_player<'a>(&'a self) -> LocalFuture<'a, ()> {
        Box::pin(self.0.resume_player())
    }

    fn seek_player(&self, seconds: f64) {
        self.0.seek_player(seconds);
    }

    fn copy<'a>(&'a self, text: &'a str, html: Option<&'a str>) -> LocalFuture<'a, bool> {
        Box::pin(self.0.copied(text, html))
    }

    fn download(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.0.download(bytes, filename, mime)
    }

    fn preview_document(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.0.preview_document(bytes, filename, mime)
    }

    fn share<'a>(&'a self, bytes: &'a [u8], filename: &'a str, mime: &'a str) -> LocalFuture<'a, ShareOutcome> {
        Box::pin(self.0.shared(bytes, filename, mime))
    }

    fn submit_checkout(&self, form: &CheckoutForm) -> bool {
        browser::submit_checkout(form)
    }

    fn analytics(&self, event: &TelemetryEvent) {
        self.0.analytics(event);
    }

    fn track_value(&self, event: &str, properties: &serde_json::Value) {
        self.0.track_value(event, properties);
    }

    fn telemetry_reset(&self) {
        self.0.telemetry_reset();
    }

    fn stored_attribution(&self) -> Option<String> {
        BrowserStorage.get(UTM_STORAGE_KEY)
    }

    fn user_agent(&self) -> String {
        web_sys::window().and_then(|w| w.navigator().user_agent().ok()).unwrap_or_default()
    }
}
