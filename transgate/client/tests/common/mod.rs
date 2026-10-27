//! The shared rig of the driver tests: the real `AppDriver`, `HttpApi` and `GoTrue`
//! against a loopback host, and a recording `Platform` whose timers the test fires
//! by hand and whose capabilities answer as the test configures them.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use billing_core::presentation::confirm::ConfirmPayload;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::checkout::CheckoutForm;
use transgate_client::driver::{AppDriver, LocalFuture, Platform, ShareOutcome};
use transgate_client::export::{DocumentAssetSource, DocumentFonts};
use transgate_client::future::BoxFuture;
use transgate_core::jobs::app::{CalendarContext, JobDetail};
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::app::{AppConfig, AppIntent, AppVm};
use transgate_core::presentation::fixtures::{self, session_profile};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::telemetry::TelemetryEvent;
use transgate_core::privacy::erasure::ErasureView;
use transgate_core::privacy::settings::UserSettings;
use transgate_export::documents::{DocumentAssets, FallbackFont};
use uuid::Uuid;

#[derive(Default)]
pub struct Memory(Mutex<HashMap<String, String>>);
impl AuthStorage for Memory {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().unwrap().get(key).cloned() }
    fn set(&self, key: &str, value: &str) { self.0.lock().unwrap().insert(key.into(), value.into()); }
    fn remove(&self, key: &str) { self.0.lock().unwrap().remove(key); }
}
struct NoContention;
impl SessionLock for NoContention {
    fn acquire(&self) -> BoxFuture<'_, LockGuard> { Box::pin(async { LockGuard::new(|| {}) }) }
}
struct Fixed(AtomicI64);
impl Clock for Fixed {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}
struct NoAssets;
impl DocumentAssetSource for NoAssets {
    fn base(&self) -> BoxFuture<'_, Result<DocumentAssets, String>> { Box::pin(async { Err("no assets in this test".to_string()) }) }
    fn fallback(&self, _: FallbackFont) -> BoxFuture<'_, Result<Vec<u8>, String>> { Box::pin(async { Err("no assets in this test".to_string()) }) }
}

pub const ADA: Uuid = Uuid::from_u128(1);
pub const BOB: Uuid = Uuid::from_u128(2);

/// What the platform was asked to do, and how it answers.
pub struct Calls {
    pub sleeps: Vec<(u64, oneshot::Sender<()>)>,
    pub navigations: Vec<(String, bool)>,
    /// What measuring a file answers (seconds).
    pub measured: Option<f64>,
    /// Progress the PUT reports before it answers, and its answer (`true` = HTTP 200).
    pub upload_ticks: Vec<u8>,
    pub upload_ok: bool,
    pub uploads: Vec<(u64, String)>,
    pub share_outcome: ShareOutcome,
    pub shares: usize,
    /// `(bytes, file name, mime)` of every save.
    pub downloads: Vec<(usize, String, String)>,
    pub analytics: Vec<String>,
    pub resets: usize,
}

impl Default for Calls {
    fn default() -> Self {
        Calls {
            sleeps: vec![],
            navigations: vec![],
            measured: Some(90.0),
            upload_ticks: vec![],
            upload_ok: true,
            uploads: vec![],
            share_outcome: ShareOutcome::Unsupported,
            shares: 0,
            downloads: vec![],
            analytics: vec![],
            resets: 0,
        }
    }
}

pub struct TestPlatform(pub Rc<RefCell<Calls>>);

impl Platform for TestPlatform {
    fn spawn(&self, work: LocalFuture<'static, ()>) {
        tokio::task::spawn_local(work);
    }
    fn sleep(&self, ms: u64) -> LocalFuture<'static, ()> {
        let (fire, fired) = oneshot::channel();
        self.0.borrow_mut().sleeps.push((ms, fire));
        Box::pin(async move {
            // A dropped (aborted) timer never fires: pend forever, as a real aborted task is dropped.
            if fired.await.is_err() {
                std::future::pending::<()>().await;
            }
        })
    }
    fn now_ms(&self) -> f64 { 1_800_000_000_000.0 }
    fn calendar(&self) -> CalendarContext { CalendarContext::default() }
    fn navigate(&self, path: &str, replace: bool) { self.0.borrow_mut().navigations.push((path.into(), replace)); }
    fn leave(&self, _: &str) {}
    fn confirmation(&self) -> Option<ConfirmPayload> { None }
    fn measure<'a>(&'a self, _: u64) -> LocalFuture<'a, Option<f64>> {
        let answer = self.0.borrow().measured;
        Box::pin(async move { answer })
    }
    fn upload<'a>(&'a self, file: u64, url: &'a str, mut progress: Box<dyn FnMut(u8)>) -> LocalFuture<'a, bool> {
        let (ticks, ok) = {
            let mut calls = self.0.borrow_mut();
            calls.uploads.push((file, url.to_string()));
            (calls.upload_ticks.clone(), calls.upload_ok)
        };
        Box::pin(async move {
            for tick in ticks {
                progress(tick);
            }
            ok
        })
    }
    fn release(&self, _: u64) {}
    fn pause_player(&self) -> bool { false }
    fn resume_player<'a>(&'a self) -> LocalFuture<'a, ()> { Box::pin(async {}) }
    fn seek_player(&self, _: f64) {}
    fn copy<'a>(&'a self, _: &'a str, _: Option<&'a str>) -> LocalFuture<'a, bool> { Box::pin(async { false }) }
    fn download(&self, bytes: &[u8], filename: &str, mime: &str) -> bool {
        self.0.borrow_mut().downloads.push((bytes.len(), filename.into(), mime.into()));
        true
    }
    fn preview_document(&self, _: &[u8], _: &str, _: &str) -> bool { false }
    fn share<'a>(&'a self, _: &'a [u8], _: &'a str, _: &'a str) -> LocalFuture<'a, ShareOutcome> {
        let outcome = {
            let mut calls = self.0.borrow_mut();
            calls.shares += 1;
            calls.share_outcome
        };
        Box::pin(async move { outcome })
    }
    fn submit_checkout(&self, _: &CheckoutForm) -> bool { false }
    fn analytics(&self, event: &TelemetryEvent) { self.0.borrow_mut().analytics.push(event.name.to_string()); }
    fn track_value(&self, _: &str, _: &Value) {}
    fn telemetry_reset(&self) { self.0.borrow_mut().resets += 1; }
    fn stored_attribution(&self) -> Option<String> { None }
    fn user_agent(&self) -> String { "driver-test".into() }
}

/// Every request the host saw: path and bearer token, in order.
pub type Seen = Arc<Mutex<Vec<(String, String)>>>;

pub struct Host {
    pub base: String,
    pub seen: Seen,
    pub jobs: Arc<Mutex<Vec<i64>>>,
}

pub fn session(user: Uuid, token: &str) -> String {
    json!({"access_token": token, "refresh_token": "r", "expires_at": 4_000_000_000i64, "user_id": user, "email": "ada@example.test"}).to_string()
}

pub async fn host() -> Host {
    let seen: Seen = Arc::default();
    let jobs = Arc::new(Mutex::new(vec![1i64]));
    async fn record(State(seen): State<Seen>, request: Request<Body>, next: Next) -> Response {
        let bearer = request.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").trim_start_matches("Bearer ").to_string();
        seen.lock().unwrap().push((request.uri().path().to_string(), bearer));
        next.run(request).await
    }
    let list = {
        let jobs = jobs.clone();
        move || {
            let rows: Vec<Value> = jobs.lock().unwrap().iter().map(|id| serde_json::to_value(fixtures::job(*id, JobStatus::Completed, "a.mp3")).unwrap()).collect();
            async move { Json(json!({"jobs": rows, "next_cursor": null})) }
        }
    };
    let routes = Router::new()
        .route("/api/session", get(|| async { Json(session_profile()) }))
        .route("/api/jobs", get(list).post(|| async {
            // A new draft for whatever file was picked: the host assigns the id.
            (StatusCode::CREATED, Json(fixtures::job(5, JobStatus::Draft, "talk.mp3")))
        }))
        .route("/api/jobs/drafts", get(|| async { Json(json!([])) }))
        .route("/api/jobs/options", patch(|| async { Json(json!([])) }))
        .route("/api/init-upload", post(|| async { (StatusCode::CREATED, Json("https://signed.example/put")) }))
        // The settings write is always refused (a 503), the read answers the stored value.
        .route("/api/settings", get(|| async { Json(UserSettings::defaults(session_profile().account)) }).patch(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        .route("/api/billing/overview", get(|| async { Json(json!({"balance": 75, "is_freemium": false, "special_client_alert": null})) }))
        // A completed job with no lines at all (EXP-015).
        .route("/api/jobs/{id}", get(|Path(id): Path<i64>| async move {
            Json(JobDetail { job: fixtures::job(id, JobStatus::Completed, "empty.mp3"), lines: vec![] })
        }))
        .route("/api/jobs/{id}/lines", get(|| async { Json(json!([])) }))
        .route("/api/jobs/{id}/export", post(|| async {
            (
                [(header::CONTENT_TYPE, "application/zip"), (header::CONTENT_DISPOSITION, "attachment; filename=\"empty.zip\"")],
                vec![0x50u8, 0x4b, 0x03, 0x04],
            )
                .into_response()
        }))
        .route("/api/delete-account", post(|| async { Json(ErasureView::FullyErased) }))
        .layer(middleware::from_fn_with_state(seen.clone(), record));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    Host { base, seen, jobs }
}

pub struct Rig {
    pub driver: Rc<AppDriver>,
    pub calls: Rc<RefCell<Calls>>,
    pub storage: Arc<Memory>,
    pub host: Host,
    /// Every view the driver published, in order.
    pub views: Rc<RefCell<Vec<AppVm>>>,
}

pub async fn rig() -> Rig {
    rig_with(Viewport::default(), Calls::default()).await
}

/// A driver with a stored session for ADA, started on the all-transcripts page.
pub async fn rig_with(viewport: Viewport, calls: Calls) -> Rig {
    let host = host().await;
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &session(ADA, "token-ada"));
    let ports = AuthPorts { storage: storage.clone(), random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    let auth = Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports));
    let api = Arc::new(HttpApi::new(host.base.clone(), auth.clone()));
    let calls = Rc::new(RefCell::new(calls));
    let driver = AppDriver::new(
        AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false },
        api,
        auth,
        DocumentFonts::new(Arc::new(NoAssets)),
        Box::new(TestPlatform(calls.clone())),
    );
    let views: Rc<RefCell<Vec<AppVm>>> = Rc::default();
    let record = views.clone();
    driver.set_listener(Some(Rc::new(move |view: &AppVm| record.borrow_mut().push(view.clone()))));
    driver.dispatch(AppIntent::Started { url: "/home?view=all-transcripts".into(), viewport, utc_offset_minutes: 0 });
    Rig { driver, calls, storage, host, views }
}

pub fn rows(driver: &AppDriver) -> Option<Vec<i64>> {
    match driver.current() {
        AppVm::SignedIn(vm) => Some(vm.jobs.rows.iter().map(|r| r.id.0).collect()),
        _ => None,
    }
}

pub async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Fires every timer the platform was asked for, as the browser's clock would.
pub fn fire_all(calls: &Rc<RefCell<Calls>>) -> usize {
    let due: Vec<_> = std::mem::take(&mut calls.borrow_mut().sleeps);
    let count = due.len();
    for (_, fire) in due {
        let _ = fire.send(());
    }
    count
}

/// Waits until the platform has stopped being asked for new timers, so a test
/// fires the whole armed set (balance, list poll, ...) and not a prefix of it.
pub async fn settle(calls: &Rc<RefCell<Calls>>) {
    let mut last = usize::MAX;
    for _ in 0..200 {
        let now = calls.borrow().sleeps.len();
        if now == last && now > 0 {
            return;
        }
        last = now;
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    panic!("timers never settled");
}

pub fn local<F: std::future::Future>(test: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, test)
}
