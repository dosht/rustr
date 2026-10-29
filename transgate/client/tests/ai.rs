//! AI start/watch transport regressions over a loopback host shaped like
//! `transgate/http/src/ai.rs`.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use llm_core::{ApplicationFrame, Limits, Persistence, StreamIdentity, StreamStatus, TextEvent};
use serde_json::{Value, json};
use tokio::sync::Notify;
use transgate_client::ai::{StartOutcome, WatchItem};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::JobId;
use transgate_core::ai::GenerationId;
use transgate_core::ai::analysis::AnalysisType;
use transgate_core::presentation::ai::{AnalysisWire, WatchFailure, WatchKind};
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
    fn acquire(&self) -> BoxFuture<'_, Result<LockGuard, String>> { Box::pin(async { Ok(LockGuard::new(|| {})) }) }
}
struct Fixed(AtomicI64);
impl Clock for Fixed {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}

const USER: Uuid = Uuid::from_u128(1);
const GENERATION: Uuid = Uuid::from_u128(77);

/// Notifies when the response body it lives in is dropped by the server.
struct Closed(Arc<Notify>);
impl Drop for Closed {
    fn drop(&mut self) { self.0.notify_one(); }
}

fn sse(chunks: Vec<String>) -> Body {
    Body::from_stream(stream::iter(chunks.into_iter().map(|c| Ok::<_, std::io::Error>(Bytes::from(c)))))
}

fn split(text: String, every: usize) -> Vec<String> {
    text.as_bytes().chunks(every).map(|c| String::from_utf8_lossy(c).into_owned()).collect()
}

async fn host(closed: Arc<Notify>) -> String {
    let analyze = move |_: HeaderMap, body: Bytes| {
        let closed = closed.clone();
        async move {
            let body: Value = serde_json::from_slice(&body).unwrap();
            if body["jobId"] == 2 {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": "Daily limit reached", "code": "question_limit",
                    "current": 10, "max": 10, "resetDate": "2026-10-26T00:00:00.000Z", "resetPeriod": "daily"}))).into_response();
            }
            if body["jobId"] == 3 {
                return (StatusCode::OK, "no header").into_response();
            }
            // A stream that never ends: the start must not wait for it.
            let guard = Closed(closed);
            let pending = stream::unfold(guard, |guard| async move {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Some((Ok::<_, std::io::Error>(Bytes::from_static(b": keep-alive\n\n")), guard))
            });
            let mut response = Response::new(Body::from_stream(pending));
            response.headers_mut().insert("content-type", "text/event-stream".parse().unwrap());
            response.headers_mut().insert("x-generation-id", GENERATION.to_string().parse().unwrap());
            response
        }
    };
    let watch = |Path(id): Path<Uuid>| async move {
        if id == GENERATION {
            let events = [
                "event: sectionCount\ndata: {\"total\":2}\n\n",
                "event: typeDetected\ndata: {\"type\":\"meeting\",\"confidence\":0.9}\n\n",
                "event: sectionReady\ndata: {\"sectionType\":\"summary\",\"data\":{\"text\":\"Hi\"}}\n\n",
                "event: complete\ndata: {\"tokensUsed\":12,\"processingTime\":340}\n\n",
            ]
            .concat();
            return sse(split(events, 7)).into_response();
        }
        let identity = StreamIdentity { generation: id.to_string(), content_revision: 1, consent_epoch: 1, execution: "e".into() };
        let frame = ApplicationFrame { identity, seq: 1, event: TextEvent::Snapshot { text: "Hel".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional } };
        sse(split(llm_http::encode_frame(&frame, Limits::default()).unwrap(), 5)).into_response()
    };
    let routes = Router::new().route("/api/v1/ai/analyze", post(analyze)).route("/api/ai/generations/{id}/watch", get(watch));
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
async fn a_start_reads_the_generation_header_and_drops_the_stream_body() {
    let closed = Arc::new(Notify::new());
    let api = api(&host(closed.clone()).await);
    let outcome = tokio::time::timeout(Duration::from_secs(5), api.start_analysis(USER, JobId(1), Uuid::from_u128(9))).await;
    assert_eq!(outcome.expect("the start must not wait for the stream to end"), StartOutcome::Accepted(GenerationId(GENERATION)));
    tokio::time::timeout(Duration::from_secs(5), closed.notified()).await.expect("dropping the answer closes the POST attachment");
}

#[tokio::test]
async fn a_quota_refusal_keeps_its_counters_and_reset() {
    let api = api(&host(Arc::new(Notify::new())).await);
    let StartOutcome::Rejected { status, rejection } = api.start_analysis(USER, JobId(2), Uuid::from_u128(9)).await else { panic!() };
    assert_eq!(status, 429);
    assert_eq!((rejection.code.as_str(), rejection.current, rejection.max), ("question_limit", Some(10), Some(10)));
    assert_eq!(rejection.reset_date.as_deref(), Some("2026-10-26T00:00:00.000Z"));
}

#[tokio::test]
async fn a_success_without_a_generation_id_is_unknown_not_accepted() {
    let api = api(&host(Arc::new(Notify::new())).await);
    assert_eq!(api.start_analysis(USER, JobId(3), Uuid::from_u128(9)).await, StartOutcome::Unknown);
}

#[tokio::test]
async fn analysis_events_split_across_chunks_decode_in_order_and_end_at_complete() {
    let api = api(&host(Arc::new(Notify::new())).await);
    let mut items = Vec::new();
    api.watch(USER, GenerationId(GENERATION), WatchKind::Analysis, None, |item| items.push(item)).await.unwrap();
    assert_eq!(
        items,
        vec![
            WatchItem::Analysis(AnalysisWire::SectionCount { total: 2 }),
            WatchItem::Analysis(AnalysisWire::TypeDetected { kind: AnalysisType::Meeting, confidence: 0.9 }),
            WatchItem::Analysis(AnalysisWire::SectionReady { section_type: "summary".into(), data: json!({"text": "Hi"}) }),
            WatchItem::Analysis(AnalysisWire::Complete { tokens_used: 12, processing_time_ms: 340 }),
        ]
    );
}

#[tokio::test]
async fn a_text_watch_that_ends_before_a_terminal_frame_is_a_disconnect() {
    let api = api(&host(Arc::new(Notify::new())).await);
    let mut frames = 0;
    let ended = api.watch(USER, GenerationId(Uuid::from_u128(5)), WatchKind::Text, Some(0), |_| frames += 1).await;
    assert_eq!(frames, 1, "the provisional snapshot is delivered before the end");
    assert_eq!(ended, Err(WatchFailure::Disconnected), "reattach, never treat as complete");
}
