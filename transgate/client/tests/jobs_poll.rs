//! D11 list polling transport: the owned list is read newest-first, page by page,
//! down to and including the oldest id the tab has loaded, over the unchanged
//! `GET /api/jobs` route (REPAIR-P5-8).
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use serde_json::{Value, json};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::jobs::app::CalendarContext;
use transgate_core::JobId;
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::fixtures;
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

fn job(id: i64) -> Value {
    serde_json::to_value(fixtures::job(id, JobStatus::Completed, "a.mp3")).unwrap()
}
/// Ids 45..=1 newest first, 20 per page, like the host.
async fn host(requests: Arc<Mutex<Vec<Option<i64>>>>) -> String {
    async fn list(State(seen): State<Arc<Mutex<Vec<Option<i64>>>>>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
        let before = q.get("before").map(|b| b.parse::<i64>().unwrap());
        seen.lock().unwrap().push(before);
        let top = before.map_or(45, |b| b - 1);
        let jobs: Vec<Value> = (1..=top).rev().take(20).map(job).collect();
        let next = (jobs.len() == 20).then(|| top - 19);
        Json(json!({"jobs": jobs, "next_cursor": next}))
    }
    let routes = Router::new().route("/api/jobs", get(list)).with_state(requests);
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
async fn a_poll_without_loaded_rows_reads_only_the_first_page() {
    let seen = Arc::new(Mutex::new(vec![]));
    let api = api(&host(seen.clone()).await);
    let jobs = api.poll_jobs(USER, None, CalendarContext::default()).await.unwrap();
    assert_eq!(jobs.iter().map(|j| j.id.0).collect::<Vec<_>>(), (26..=45).rev().collect::<Vec<_>>());
    assert_eq!(*seen.lock().unwrap(), vec![None]);
}

#[tokio::test]
async fn a_poll_reads_pages_until_the_oldest_loaded_row_is_covered() {
    let seen = Arc::new(Mutex::new(vec![]));
    let api = api(&host(seen.clone()).await);
    // The tab loaded two pages (45..=6): the poll must reach id 6 and stop there.
    let jobs = api.poll_jobs(USER, Some(JobId(6)), CalendarContext::default()).await.unwrap();
    assert_eq!(jobs.first().map(|j| j.id.0), Some(45));
    assert!(jobs.iter().any(|j| j.id.0 == 6));
    assert_eq!(*seen.lock().unwrap(), vec![None, Some(26)], "no third page once 6 is on the second");
}
