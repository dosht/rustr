//! The shared effect driver on a native, non-browser `Platform` (P8-IMPL-1, D1):
//! the real `AppDriver`, the real `HttpApi` and `GoTrue` against a loopback host,
//! and a recording platform whose timers the test fires by hand. These pin what
//! the web shell relies on without a browser: epoch and token fences, the keyed
//! timer chain, and dispose.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
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
use transgate_core::jobs::app::CalendarContext;
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::app::{AppConfig, AppIntent, AppVm, ManageDataIntent};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::presentation::fixtures::{self, session_profile};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::telemetry::TelemetryEvent;
use transgate_export::documents::{DocumentAssets, FallbackFont};
use uuid::Uuid;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);
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

const ADA: Uuid = Uuid::from_u128(1);
const BOB: Uuid = Uuid::from_u128(2);

/// What the platform was asked to do; the test fires timers by hand.
#[derive(Default)]
struct Calls {
    sleeps: Vec<(u64, oneshot::Sender<()>)>,
    navigations: Vec<(String, bool)>,
}

struct TestPlatform(Rc<RefCell<Calls>>);

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
    fn measure<'a>(&'a self, _: u64) -> LocalFuture<'a, Option<f64>> { Box::pin(async { None }) }
    fn upload<'a>(&'a self, _: u64, _: &'a str, _: Box<dyn FnMut(u8)>) -> LocalFuture<'a, bool> { Box::pin(async { false }) }
    fn release(&self, _: u64) {}
    fn pause_player(&self) -> bool { false }
    fn resume_player<'a>(&'a self) -> LocalFuture<'a, ()> { Box::pin(async {}) }
    fn seek_player(&self, _: f64) {}
    fn copy<'a>(&'a self, _: &'a str, _: Option<&'a str>) -> LocalFuture<'a, bool> { Box::pin(async { false }) }
    fn download(&self, _: &[u8], _: &str, _: &str) -> bool { false }
    fn preview_document(&self, _: &[u8], _: &str, _: &str) -> bool { false }
    fn share<'a>(&'a self, _: &'a [u8], _: &'a str, _: &'a str) -> LocalFuture<'a, ShareOutcome> { Box::pin(async { ShareOutcome::Unsupported }) }
    fn submit_checkout(&self, _: &CheckoutForm) -> bool { false }
    fn analytics(&self, _: &TelemetryEvent) {}
    fn track_value(&self, _: &str, _: &Value) {}
    fn telemetry_reset(&self) {}
    fn stored_attribution(&self) -> Option<String> { None }
    fn user_agent(&self) -> String { "driver-test".into() }
}

/// Every request the host saw: path and bearer token, in order.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

struct Host {
    base: String,
    seen: Seen,
    jobs: Arc<Mutex<Vec<i64>>>,
}

async fn host() -> Host {
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
    let settings = || async { Json(UserSettings::defaults(session_profile().account)) };
    let routes = Router::new()
        .route("/api/session", get(|| async { Json(session_profile()) }))
        .route("/api/jobs", get(list))
        // The settings write is always refused (a 503), the read answers the stored value.
        .route("/api/settings", get(settings).patch(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        .layer(middleware::from_fn_with_state(seen.clone(), record));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    Host { base, seen, jobs }
}

fn session(user: Uuid, token: &str) -> String {
    json!({"access_token": token, "refresh_token": "r", "expires_at": 4_000_000_000i64, "user_id": user, "email": "ada@example.test"}).to_string()
}

struct Rig {
    driver: Rc<AppDriver>,
    calls: Rc<RefCell<Calls>>,
    storage: Arc<Memory>,
    host: Host,
}

async fn rig() -> Rig {
    let host = host().await;
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &session(ADA, "token-ada"));
    let ports = AuthPorts { storage: storage.clone(), random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    let auth = Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports));
    let api = Arc::new(HttpApi::new(host.base.clone(), auth.clone()));
    let calls = Rc::new(RefCell::new(Calls::default()));
    let driver = AppDriver::new(
        AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false },
        api,
        auth,
        DocumentFonts::new(Arc::new(NoAssets)),
        Box::new(TestPlatform(calls.clone())),
    );
    driver.dispatch(AppIntent::Started { url: "/home?view=all-transcripts".into(), viewport: Viewport::default(), utc_offset_minutes: 0 });
    Rig { driver, calls, storage, host }
}

fn rows(driver: &AppDriver) -> Option<Vec<i64>> {
    match driver.current() {
        AppVm::SignedIn(vm) => Some(vm.jobs.rows.iter().map(|r| r.id.0).collect()),
        _ => None,
    }
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Fires every timer the platform was asked for, as the browser's clock would.
fn fire_all(calls: &Rc<RefCell<Calls>>) -> usize {
    let due: Vec<_> = std::mem::take(&mut calls.borrow_mut().sleeps);
    let count = due.len();
    for (_, fire) in due {
        let _ = fire.send(());
    }
    count
}

/// Waits until the platform has stopped being asked for new timers, so a test
/// fires the whole armed set (balance, list poll, ...) and not a prefix of it.
async fn settle(calls: &Rc<RefCell<Calls>>) {
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

fn local<F: std::future::Future>(test: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, test)
}

#[test]
fn a_stored_session_restores_and_the_owned_list_loads_through_the_driver() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let seen = rig.host.seen.lock().unwrap().clone();
        assert!(seen.iter().any(|(path, bearer)| path == "/api/session" && bearer == "token-ada"), "{seen:?}");
    });
}

/// The token fence: once another process signs in a different account, nothing is
/// sent with the first account's token, not even by effects that were already queued.
#[test]
fn after_an_account_switch_no_request_carries_the_old_accounts_token() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let before = rig.driver.epoch();

        rig.storage.set(SESSION_KEY, &session(BOB, "token-bob"));
        rig.driver.session_changed_elsewhere();
        assert_ne!(rig.driver.epoch().session, before.session, "core began a new session");
        let mark = rig.host.seen.lock().unwrap().len();

        until("requests of the new account", || rig.host.seen.lock().unwrap().len() > mark + 1).await;
        let after: Vec<_> = rig.host.seen.lock().unwrap()[mark..].to_vec();
        assert!(after.iter().all(|(_, bearer)| bearer == "token-bob"), "only the new account's token is used: {after:?}");
    });
}

/// D11 through the driver: the keyed `jobs.poll` timer asks the platform to sleep,
/// firing it polls the host, and a job created elsewhere shows without a reload.
#[test]
fn the_list_poll_chain_runs_on_platform_timers() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: true });
        // The page starts visible, so this arms the chain: the platform is asked for a timer.
        until("the platform to be asked for a timer", || !rig.calls.borrow().sleeps.is_empty()).await;
        settle(&rig.calls).await;
        rig.host.jobs.lock().unwrap().insert(0, 2);
        assert!(fire_all(&rig.calls) > 0);
        until("the job found by the first timer-driven poll", || rows(&rig.driver) == Some(vec![2, 1])).await;

        // The chain re-armed itself: another tick finds the next job.
        until("the platform to be asked for the next timer", || !rig.calls.borrow().sleeps.is_empty()).await;
        settle(&rig.calls).await;
        rig.host.jobs.lock().unwrap().insert(0, 3);
        assert!(fire_all(&rig.calls) > 0);
        until("the job found by the second poll", || rows(&rig.driver) == Some(vec![3, 2, 1])).await;
    });
}

/// Dispose aborts every keyed timer (their futures are dropped), silences the
/// listener, and a late timer fires into nothing.
#[test]
fn dispose_drops_timers_and_a_late_fire_changes_nothing() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: true });
        until("the platform to be asked for a timer", || !rig.calls.borrow().sleeps.is_empty()).await;

        let notified = Rc::new(Cell::new(0));
        let counter = notified.clone();
        rig.driver.set_listener(Some(Rc::new(move |_| counter.set(counter.get() + 1))));
        assert!(rig.driver.dispose());
        assert!(!rig.driver.dispose(), "dispose is once");

        let pending: Vec<_> = std::mem::take(&mut rig.calls.borrow_mut().sleeps);
        assert!(!pending.is_empty());
        let mark = rig.host.seen.lock().unwrap().len();
        rig.host.jobs.lock().unwrap().insert(0, 9);
        for (_, fire) in pending {
            let _ = fire.send(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(rig.host.seen.lock().unwrap().len(), mark, "no request after dispose");
        assert_eq!(notified.get(), 0, "no view after dispose");
        assert_eq!(rows(&rig.driver), Some(vec![1]), "state is frozen at dispose");
        // An intent after dispose returns the current view and runs nothing.
        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: false });
        assert_eq!(rig.host.seen.lock().unwrap().len(), mark);
    });
}

/// A refused settings write returns the switch to its stored value with the save
/// error shown (core's `SaveFailed`), and the driver does not reread the settings.
#[test]
fn a_refused_settings_write_answers_save_failed_without_a_reread() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let reads = || rig.host.seen.lock().unwrap().iter().filter(|(path, _)| path == "/api/settings").count();
        until("the settings to load", || reads() >= 1).await;
        let before = reads();

        rig.driver.dispatch(AppIntent::ManageData(ManageDataIntent::DeleteMonthlyToggled { on: false }));
        let settled = |driver: &AppDriver| match driver.current() {
            AppVm::SignedIn(vm) => (vm.manage_data.delete_monthly_on, vm.manage_data.save_error.is_some()),
            _ => (false, false),
        };
        until("the refused write to be answered", || settled(&rig.driver) == (true, true)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        // The PATCH hit the same path once; no GET followed the refusal.
        assert_eq!(reads(), before + 1, "one PATCH and no reread");
    });
}
