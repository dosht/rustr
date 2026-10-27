//! D11 through the real client and the real `AppState`: a job created elsewhere
//! (or just started) appears in the list without a reload, only while the page is
//! visible and the user is signed in (REPAIR-P5-8). The driver's timer is played by
//! feeding `AppEvent::Timer` for the effect core asked for.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde_json::{Value, json};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::jobs::app::CalendarContext;
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::app::{
    AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEvent, SessionFacts, Timer,
};
use transgate_core::presentation::fixtures::{self, NOW};
use transgate_core::presentation::job_list::{Effect, Event};
use transgate_core::presentation::layout::Viewport;
use transgate_core::{JobId, Micros};
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

const USER: Uuid = Uuid::from_u128(1);

/// The host's first page: the ids currently stored, newest first.
async fn host(stored: Arc<Mutex<Vec<i64>>>) -> String {
    async fn list(State(stored): State<Arc<Mutex<Vec<i64>>>>) -> Json<Value> {
        let jobs: Vec<Value> = stored
            .lock()
            .unwrap()
            .iter()
            .map(|id| serde_json::to_value(fixtures::job(*id, JobStatus::Completed, "a.mp3")).unwrap())
            .collect();
        Json(json!({"jobs": jobs, "next_cursor": null}))
    }
    let routes = Router::new().route("/api/jobs", get(list)).with_state(stored);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    base
}

fn api(base: &str) -> HttpApi {
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &json!({"access_token": "t", "refresh_token": "r", "expires_at": 4_000_000_000i64,
                                    "user_id": USER, "email": "ada@example.test"}).to_string());
    let ports = AuthPorts { storage, random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    HttpApi::new(base, Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports)))
}

fn signed_in() -> AppState {
    let mut s = AppState::new(AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false });
    s.update(AppEvent::User(AppIntent::Started { url: "/home?view=all-transcripts".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
    let profile = fixtures::session_profile();
    let facts = SessionFacts { user_id: profile.account.0, email: profile.email.clone(), expires_at: Micros(NOW.0 + 3_600_000_000) };
    s.update(AppEvent::Auth { epoch: s.epoch(), event: AuthEvent::Restored { facts: Some(facts) } }, NOW);
    s.update(AppEvent::SessionLoaded { epoch: s.epoch(), result: Ok(profile) }, NOW);
    s.update(AppEvent::Jobs { epoch: s.epoch(), event: Event::PageLoaded { jobs: vec![fixtures::job(1, JobStatus::Completed, "a.mp3")], next_cursor: None, replace: true } }, NOW);
    s
}

fn rows(s: &AppState) -> Vec<i64> {
    match s.view(NOW) {
        AppVm::SignedIn(vm) => vm.jobs.rows.iter().map(|r| r.id.0).collect(),
        other => panic!("{other:?}"),
    }
}

fn poll_timer(effects: &[AppEffect]) -> Option<(transgate_core::presentation::app::Epoch, Timer)> {
    effects.iter().find_map(|e| match e {
        AppEffect::Timer { epoch, timer: timer @ Timer::JobsPoll { .. }, .. } => Some((*epoch, *timer)),
        _ => None,
    })
}

#[tokio::test]
async fn a_job_created_elsewhere_appears_while_visible_and_polling_stops_when_hidden() {
    let stored = Arc::new(Mutex::new(vec![1]));
    let api = api(&host(stored.clone()).await);
    let mut s = signed_in();

    let armed = s.update(AppEvent::User(AppIntent::VisibilityChanged { visible: true }), NOW);
    let (epoch, timer) = poll_timer(&armed).expect("a visible, signed-in tab arms the list poll");
    assert_eq!(rows(&s), vec![1]);

    // Another tab starts job 2; this tab's timer fires.
    stored.lock().unwrap().insert(0, 2);
    let fired = s.update(AppEvent::Timer { epoch, timer }, NOW);
    let through = fired.iter().find_map(|e| match e {
        AppEffect::Jobs { effect: Effect::PollPage { through }, .. } => Some(*through),
        _ => None,
    });
    assert_eq!(through, Some(Some(JobId(1))), "polls down to the oldest loaded row: {fired:?}");
    let jobs = api.poll_jobs(USER, Some(JobId(1)), CalendarContext::default()).await.unwrap();
    let after = s.update(AppEvent::Jobs { epoch, event: Event::Polled { jobs } }, NOW);
    assert_eq!(rows(&s), vec![2, 1], "the new job shows without a reload");
    let (_, next) = poll_timer(&after).or_else(|| poll_timer(&fired)).expect("the chain re-arms itself");

    // Hidden: the stale timer does nothing and none is re-armed.
    s.update(AppEvent::User(AppIntent::VisibilityChanged { visible: false }), NOW);
    let stale = s.update(AppEvent::Timer { epoch, timer: next }, NOW);
    assert!(stale.iter().all(|e| !matches!(e, AppEffect::Jobs { effect: Effect::PollPage { .. }, .. })), "{stale:?}");
    assert!(poll_timer(&stale).is_none());
}

#[tokio::test]
async fn a_failed_poll_keeps_every_row_and_signing_out_ends_the_chain() {
    let mut s = signed_in();
    let armed = s.update(AppEvent::User(AppIntent::VisibilityChanged { visible: true }), NOW);
    let (epoch, timer) = poll_timer(&armed).unwrap();
    s.update(AppEvent::Timer { epoch, timer }, NOW);
    s.update(AppEvent::Jobs { epoch, event: Event::PollFailed }, NOW);
    assert_eq!(rows(&s), vec![1]);
    match s.view(NOW) {
        AppVm::SignedIn(vm) => assert_eq!(vm.jobs.error, None, "no banner for a failed poll"),
        other => panic!("{other:?}"),
    }

    s.update(AppEvent::Auth { epoch, event: AuthEvent::SignedOut }, NOW);
    let late = s.update(AppEvent::Timer { epoch, timer }, NOW);
    assert!(late.iter().all(|e| !matches!(e, AppEffect::Jobs { .. })), "a signed-out tab never polls: {late:?}");
}
