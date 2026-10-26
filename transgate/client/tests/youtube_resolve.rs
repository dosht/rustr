//! `POST /api/youtube/resolve`: the host builds its body with
//! `transgate_api::resolve_response`, and the client must read exactly that
//! (REPAIR-P5-10). Bodies come from the real host function, so drift between host
//! and client fails here instead of in a browser.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::JobId;
use transgate_core::jobs::app::ResolveAnswer;
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

fn completed() -> ResolveAnswer {
    ResolveAnswer::Completed { job_id: JobId(11) }
}

fn failed() -> ResolveAnswer {
    ResolveAnswer::Failed { job_id: JobId(12), code: "private_video".into(), message: "This video is private".into() }
}

async fn host() -> String {
    let resolve = |Json(body): Json<Value>| async move {
        Json(match body["jobId"].as_i64() {
            Some(11) => transgate_api::resolve_response(&completed()),
            Some(12) => transgate_api::resolve_response(&failed()),
            Some(13) => json!({"status": "pending", "jobId": 13}),
            _ => json!({}),
        })
    };
    let routes = Router::new().route("/api/youtube/resolve", post(resolve));
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

#[tokio::test]
async fn a_completed_resolve_round_trips_the_hosts_envelope() {
    let api = api(&host().await);
    assert_eq!(api.youtube_resolve(USER, JobId(11)).await.unwrap(), completed());
}

#[tokio::test]
async fn a_failed_resolve_keeps_its_code_and_message() {
    let api = api(&host().await);
    assert_eq!(api.youtube_resolve(USER, JobId(12)).await.unwrap(), failed());
}

#[tokio::test]
async fn an_unknown_status_or_empty_body_is_a_decode_error_not_a_guess() {
    let api = api(&host().await);
    assert!(api.youtube_resolve(USER, JobId(13)).await.is_err());
    assert!(api.youtube_resolve(USER, JobId(14)).await.is_err());
}

/// The decoded answer drives the real `AppState` to the job page, as the driver
/// maps it (`Completed` -> `YouTubeResolved { completed: true }`).
#[tokio::test]
async fn a_completed_resolve_navigates_to_the_job() {
    use transgate_core::Micros;
    use transgate_core::presentation::app::{AppConfig, AppEffect, AppEvent, AppIntent, AppState, AuthEvent, SessionFacts};
    use transgate_core::presentation::fixtures::{NOW, session_profile};
    use transgate_core::presentation::layout::Viewport;
    use transgate_core::presentation::upload_queue::Event;

    let api = api(&host().await);
    let ResolveAnswer::Completed { job_id } = api.youtube_resolve(USER, JobId(11)).await.unwrap() else { panic!("completed expected") };

    let mut s = AppState::new(AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false });
    s.update(AppEvent::User(AppIntent::Started { url: "/home".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
    let profile = session_profile();
    let facts = SessionFacts { user_id: profile.account.0, email: profile.email.clone(), expires_at: Micros(NOW.0 + 3_600_000_000) };
    s.update(AppEvent::Auth { epoch: s.epoch(), event: AuthEvent::Restored { facts: Some(facts) } }, NOW);
    s.update(AppEvent::SessionLoaded { epoch: s.epoch(), result: Ok(profile) }, NOW);
    let effects = s.update(AppEvent::Upload { epoch: s.epoch(), event: Event::YouTubeResolved { job: job_id, completed: true, code: None } }, NOW);
    assert!(effects.contains(&AppEffect::Navigate { path: "/home/11".into(), replace: false }), "{effects:?}");
}
