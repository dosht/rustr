//! The presenter handle end to end, as a Kotlin or Swift shell would drive it: fake
//! `NativePorts` and `ViewListener` (the foreign side), the real `AppDriver`,
//! `HttpApi` and `GoTrue` on the driver thread, and a loopback host.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::app::{AppIntent, AppVm};
use transgate_core::presentation::fixtures::{self, session_profile};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::msg::common;
use transgate_ffi::{CalendarOffsets, FfiError, NativePorts, PresenterConfig, ProgressSink, ShareResult, TransgatePresenter, ViewListener, translate};

#[derive(Default)]
struct Ports {
    storage: Mutex<HashMap<String, String>>,
    navigations: Mutex<Vec<(String, bool)>>,
}

impl NativePorts for Ports {
    fn storage_get(&self, key: String) -> Option<String> { self.storage.lock().unwrap().get(&key).cloned() }
    fn storage_set(&self, key: String, value: String) { self.storage.lock().unwrap().insert(key, value); }
    fn storage_remove(&self, key: String) { self.storage.lock().unwrap().remove(&key); }
    fn navigate(&self, path: String, replace: bool) { self.navigations.lock().unwrap().push((path, replace)); }
    fn leave(&self, _url: String) {}
    fn calendar(&self) -> CalendarOffsets { CalendarOffsets { now_minutes: 180, then_minutes: 180 } }
    fn measure(&self, _file: u64) -> Option<f64> { None }
    fn upload(&self, _file: u64, _url: String, _progress: Arc<ProgressSink>) -> bool { false }
    fn release(&self, _file: u64) {}
    fn pause_player(&self) -> bool { false }
    fn resume_player(&self) {}
    fn seek_player(&self, _seconds: f64) {}
    fn copy(&self, _text: String, _html: Option<String>) -> bool { false }
    fn save(&self, _bytes: Vec<u8>, _filename: String, _mime: String) -> bool { false }
    fn preview(&self, _bytes: Vec<u8>, _filename: String, _mime: String) -> bool { false }
    fn share(&self, _bytes: Vec<u8>, _filename: String, _mime: String) -> ShareResult { ShareResult::Unsupported }
    fn asset(&self, _name: String) -> Option<Vec<u8>> { None }
}

/// Collects every view; the first one calls back into the presenter, which must not deadlock.
#[derive(Default)]
struct Listener {
    views: Mutex<Vec<AppVm>>,
    presenter: OnceLock<Arc<TransgatePresenter>>,
    reentered: Mutex<Option<bool>>,
}

impl ViewListener for Listener {
    fn view_changed(&self, view: AppVm) {
        let first = self.views.lock().unwrap().is_empty();
        self.views.lock().unwrap().push(view);
        if first && let Some(presenter) = self.presenter.get() {
            *self.reentered.lock().unwrap() = Some(presenter.current().is_ok());
        }
    }
}

fn host() -> String {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let base = runtime.block_on(async {
        let routes = Router::new()
            .route("/api/session", get(|| async { Json(session_profile()) }))
            .route("/api/jobs", get(|| async {
                let jobs: Vec<Value> = [3, 1].iter().map(|id| serde_json::to_value(fixtures::job(*id, JobStatus::Completed, "talk.mp3")).unwrap()).collect();
                Json(json!({"jobs": jobs, "next_cursor": null}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        base
    });
    // The runtime must outlive the test; it ends with the process.
    std::mem::forget(runtime);
    base
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

fn rows(view: &AppVm) -> Vec<i64> {
    match view {
        AppVm::SignedIn(vm) => vm.jobs.rows.iter().map(|r| r.id.0).collect(),
        _ => vec![],
    }
}

#[test]
fn a_stored_session_signs_in_and_lists_jobs_through_the_handle() {
    let base = host();
    let ports = Arc::new(Ports::default());
    let profile = session_profile();
    ports.storage_set(
        "transgate.session".into(),
        json!({"access_token": "t", "refresh_token": "r", "expires_at": 4_000_000_000i64, "user_id": profile.account.0.to_string(), "email": profile.email}).to_string(),
    );
    let listener = Arc::new(Listener::default());
    let config = PresenterConfig {
        api_url: base.clone(),
        auth_url: "http://127.0.0.1:9/auth/v1".into(),
        auth_key: None,
        site_url: "transgate://app".into(),
        captcha: false,
        user_agent: "ffi-test".into(),
    };
    let presenter = TransgatePresenter::new(config, ports.clone(), listener.clone()).expect("the driver thread starts");
    listener.presenter.set(presenter.clone()).ok();

    let first = presenter
        .dispatch(AppIntent::Started { url: "/home?view=all-transcripts".into(), viewport: Viewport { touch: true, width: 412 }, utc_offset_minutes: 180 })
        .expect("dispatch answers");
    assert!(matches!(first, AppVm::Restoring { .. }), "the stored session is still being restored: {first:?}");

    until("the owned list through the handle", || listener.views.lock().unwrap().iter().any(|v| rows(v) == vec![3, 1]));
    assert_eq!(*listener.reentered.lock().unwrap(), Some(true), "a listener may call back into the presenter");
    assert!(matches!(presenter.current().unwrap(), AppVm::SignedIn(_)));

    presenter.dispose();
    presenter.dispose();
    assert_eq!(presenter.current(), Err(FfiError::Disposed));
    assert_eq!(presenter.dispatch(AppIntent::SignOutRequested), Err(FfiError::Disposed));
}

#[test]
fn translate_is_strict_and_uses_the_web_catalogs() {
    assert_eq!(translate("en".into(), common("Cancel")).unwrap(), "Cancel");
    assert!(matches!(translate("tr".into(), common("no such key")), Err(FfiError::Translate { .. })));
    assert!(matches!(translate("tr-TR".into(), common("Cancel")), Err(FfiError::Translate { .. })));
}
