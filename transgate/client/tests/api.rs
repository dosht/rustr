//! Session API regressions over a loopback host that records what it received.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use transgate_client::api::{ApiError, HttpApi, disposition_filename};
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::JobId;
use transgate_core::export::ExportLabels;
use transgate_core::jobs::app::{DraftOptions, ExportKind, ExportRequest, ExportSurface};
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

/// One recorded request: path, bearer token, JSON body.
type Recorded = (String, Option<String>, Value);

#[derive(Clone, Default)]
struct Host(Arc<Mutex<Vec<Recorded>>>);

async fn options(State(h): State<Host>, headers: HeaderMap, body: Bytes) -> Json<Value> {
    let bearer = headers.get("authorization").map(|v| v.to_str().unwrap().to_string());
    h.0.lock().unwrap().push(("/api/jobs/options".into(), bearer, serde_json::from_slice(&body).unwrap()));
    Json(json!([]))
}
async fn export() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/zip"),
         (header::CONTENT_DISPOSITION, "attachment; filename=\"_ner_ m_lakat.zip\"; filename*=UTF-8''%C4%B0ner%20m%C3%BClakat.zip")],
        vec![0x50, 0x4b, 0x03, 0x04],
    )
}
async fn denied() -> (StatusCode, Json<Value>) {
    (StatusCode::UNAUTHORIZED, Json(json!("Not authorised")))
}

async fn host() -> (Host, String) {
    let recorded = Host::default();
    let routes = Router::new()
        .route("/api/jobs/options", patch(options))
        .route("/api/jobs/{id}/export", post(export))
        .route("/api/init-upload", post(denied))
        .with_state(recorded.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    (recorded, base)
}

fn signed_in(base: &str, user: Uuid) -> HttpApi {
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &json!({"access_token": "token-of-user", "refresh_token": "r", "expires_at": 4_000_000_000i64,
                                    "user_id": user, "email": "ada@example.test"}).to_string());
    let ports = AuthPorts { storage, random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    let auth = GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports);
    HttpApi::new(base, Arc::new(auth))
}

#[tokio::test]
async fn a_request_for_another_account_is_never_sent_with_this_sessions_token() {
    let (recorded, base) = host().await;
    let api = signed_in(&base, Uuid::from_u128(1));
    let other = Uuid::from_u128(2);
    let error = api.set_draft_options(other, &[JobId(7)], &DraftOptions::default()).await.unwrap_err();
    assert_eq!(error, ApiError::SignedOut);
    assert!(recorded.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn draft_options_keep_absent_versus_auto_detect() {
    let (recorded, base) = host().await;
    let user = Uuid::from_u128(1);
    let api = signed_in(&base, user);
    api.set_draft_options(user, &[JobId(7)], &DraftOptions { language: Some(None), translate_to: None, speakers: Some(true) }).await.unwrap();
    api.set_draft_options(user, &[JobId(7)], &DraftOptions::default()).await.unwrap();
    let calls = recorded.0.lock().unwrap();
    assert_eq!(calls[0].1.as_deref(), Some("Bearer token-of-user"));
    assert_eq!(calls[0].2, json!({"jobIds": [7], "language": null, "speakers": true}));
    assert_eq!(calls[1].2, json!({"jobIds": [7]}), "absent options must not be sent as null");
}

#[tokio::test]
async fn export_keeps_the_hosts_utf8_file_name_and_bytes() {
    let (_recorded, base) = host().await;
    let user = Uuid::from_u128(1);
    let api = signed_in(&base, user);
    let request = ExportRequest { kind: ExportKind::AllFormats, surface: ExportSurface::TranscriptPage, labels: ExportLabels::default() };
    let download = api.export(user, JobId(7), &request).await.unwrap();
    assert_eq!(download.filename, "İner mülakat.zip");
    assert_eq!(download.bytes, vec![0x50, 0x4b, 0x03, 0x04]);
}

#[tokio::test]
async fn a_source_text_401_keeps_its_message_and_status() {
    let (_recorded, base) = host().await;
    let user = Uuid::from_u128(1);
    let api = signed_in(&base, user);
    let ApiError::Http { status, code, message, .. } = api.init_upload(user, JobId(7), "a.wav").await.unwrap_err() else { panic!() };
    assert_eq!((status, code, message.as_deref()), (401, None, Some("Not authorised")));
}

#[test]
fn disposition_prefers_the_exact_utf8_name() {
    assert_eq!(disposition_filename("attachment; filename=\"a_b.zip\"; filename*=UTF-8''a%C3%A7b.zip").as_deref(), Some("açb.zip"));
    assert_eq!(disposition_filename("attachment; filename=\"plain.zip\"").as_deref(), Some("plain.zip"));
}
