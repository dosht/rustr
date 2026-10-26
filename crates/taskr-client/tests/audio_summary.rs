//! Consumer-visible shell regressions. The lead runs these after coherent integration.
use std::sync::{Arc, Mutex};

use axum::{Json, Router, routing::{get, post}};
use serde_json::{Value, json};
use taskr_client::{HttpApi, audio_summary::SummaryController};
use taskr_core::session::app::{Session, SessionStore};

#[derive(Default)]
struct Sessions(Mutex<Option<Session>>);
impl SessionStore for Sessions {
    fn load(&self) -> Option<Session> { self.0.lock().unwrap().clone() }
    fn save(&self, session: &Session) { *self.0.lock().unwrap() = Some(session.clone()); }
    fn clear(&self) { *self.0.lock().unwrap() = None; }
}
fn signed_in() -> Arc<Sessions> {
    let sessions = Arc::new(Sessions::default());
    sessions.save(&Session { access_token: "owner-token".into(), refresh_token: None, email: "owner@example.test".into(), expires_at: None });
    sessions
}
fn work(id: &str, status: &str, transcript: Option<&str>) -> Value {
    json!({"id": id, "name": "Research notes", "status": status, "transcript": transcript})
}
async fn serve(routes: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap(); });
    (base, task)
}

#[tokio::test]
async fn unavailable_transcript_cannot_be_submitted_and_requires_explicit_disclosure() {
    let id = "11111111-1111-4111-8111-111111111111";
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let routes = Router::new()
        .route("/api/audio-work", get(move || async move { Json(json!({"works": [work(id, "completed", Some("A completed transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null})) }))
        .route("/api/audio-work/{id}/summary/generations", post(move || { count.fetch_add(1, std::sync::atomic::Ordering::SeqCst); async { Json(json!({})) } }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(id.to_string()).await;
    controller.start("22222222-2222-4222-8222-222222222222".into(), false).await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(controller.view().error.unwrap().contains("external"));
    server.abort();
}

#[tokio::test]
async fn changing_session_during_detail_fetch_drops_previous_owners_content() {
    let id = "11111111-1111-4111-8111-111111111111";
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (arrived, unblock) = (entered.clone(), release.clone());
    let routes = Router::new()
        .route("/api/audio-work", get(move || async move { Json(json!({"works": [work(id, "completed", Some("private transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(move || { let entered = arrived.clone(); let release = unblock.clone(); async move {
            entered.notify_one(); release.notified().await;
            Json(json!({"summary": {"generation_id": "33333333-3333-4333-8333-333333333333", "work_id": id, "source_revision": 1, "text": "private saved summary", "saved_at_us": 1}}))
        } }));
    let (base, server) = serve(routes).await;
    let sessions = signed_in();
    let controller = Arc::new(SummaryController::new(Arc::new(HttpApi::new(base, sessions.clone()))));
    controller.refresh().await;
    let selected = controller.clone();
    let pending = tokio::spawn(async move { selected.select(id.into()).await; });
    entered.notified().await;
    sessions.clear();
    release.notify_one();
    pending.await.unwrap();
    let view = controller.view();
    assert_eq!(view.text, "");
    assert!(view.works.is_empty());
    assert!(!view.can_start);
    server.abort();
}

#[tokio::test]
async fn selecting_unfinished_work_does_not_enable_summary_action() {
    let id = "11111111-1111-4111-8111-111111111111";
    let routes = Router::new()
        .route("/api/audio-work", get(move || async move { Json(json!({"works": [work(id, "processing", None)]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null})) }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(id.into()).await;
    assert!(!controller.view().can_start);
    assert_eq!(controller.view().status_label, "A completed transcript is required.");
    server.abort();
}

#[tokio::test]
async fn progressive_reconnect_replaces_sanitized_text_without_restarting_generation() {
    use llm_core::{ApplicationFrame, Limits, Persistence, StreamIdentity, StreamStatus, TextEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    const WORK: &str = "11111111-1111-4111-8111-111111111111";
    const GENERATION: &str = "33333333-3333-4333-8333-333333333333";
    const EXECUTION: &str = "44444444-4444-4444-8444-444444444444";
    let starts = Arc::new(AtomicUsize::new(0));
    let watches = Arc::new(AtomicUsize::new(0));
    let started = starts.clone();
    let watched = watches.clone();
    let snapshot = json!({"generation_id": GENERATION, "execution_id": EXECUTION, "work_id": WORK, "source_revision": 1, "status": "running", "sequence": 0, "text": "", "durable": false, "remote_outcome_unknown": true, "error": null});
    let routes = Router::new()
        .route("/api/audio-work", get(|| async { Json(json!({"works": [work(WORK, "completed", Some("Completed transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null, "generation": null})) }))
        .route("/api/audio-work/{id}/summary/generations", post(move || { started.fetch_add(1, Ordering::SeqCst); let snapshot = snapshot.clone(); async { Json(snapshot) } }))
        .route("/api/audio-work/{id}/summary/generations/{generation}/watch", get(move || {
            let number = watched.fetch_add(1, Ordering::SeqCst);
            async move {
                let identity = StreamIdentity { generation: GENERATION.into(), content_revision: 1, consent_epoch: 0, execution: EXECUTION.into() };
                let events = if number == 0 {
                    vec![(1, TextEvent::Snapshot { text: "Hello ".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional })]
                } else {
                    vec![
                        (1, TextEvent::Snapshot { text: "Hello".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }),
                        (3, TextEvent::Snapshot { text: "Hello world".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }),
                        (4, TextEvent::Final { text: "Authoritative saved summary.".into(), persistence: Persistence::Saved }),
                    ]
                };
                let wire: String = events.into_iter().map(|(seq, event)| llm_http::encode_frame(&ApplicationFrame { identity: identity.clone(), seq, event }, Limits::default()).unwrap()).collect();
                let chunks: Vec<_> = wire.as_bytes().chunks(7).map(|chunk| Ok::<_, std::convert::Infallible>(chunk.to_vec())).collect();
                axum::response::Response::builder().header("content-type", "text/event-stream")
                    .body(axum::body::Body::from_stream(futures_util::stream::iter(chunks))).unwrap()
            }
        }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(WORK.into()).await;
    controller.start("22222222-2222-4222-8222-222222222222".into(), true).await;
    let seen = Mutex::new(Vec::new());
    controller.watch(|view| {
        if view.text == "Hello " {
            seen.lock().unwrap().push(view.text);
            // Re-entry from a foreign listener must not deadlock the state mutex.
            controller.suspend();
        }
    }).await;
    assert!(controller.view().can_resume);
    controller.watch(|view| { seen.lock().unwrap().push(view.text); }).await;
    let seen = seen.into_inner().unwrap();
    assert!(seen.iter().any(|text| text == "Hello "));
    assert!(seen.iter().any(|text| text == "Hello world"));
    assert!(!seen.iter().any(|text| text == "Helloworld"));
    assert_eq!(controller.view().text, "Authoritative saved summary.");
    assert!(controller.view().durable);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(watches.load(Ordering::SeqCst), 2);
    controller.dispose();
    controller.refresh().await;
    assert!(controller.view().works.is_empty());
    assert!(controller.view().text.is_empty());
    server.abort();
}

#[tokio::test]
async fn lost_start_response_reuses_request_identity_instead_of_new_generation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    const WORK: &str = "11111111-1111-4111-8111-111111111111";
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = ids.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let routes = Router::new()
        .route("/api/audio-work", get(|| async { Json(json!({"works": [work(WORK, "completed", Some("Completed transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null, "generation": null})) }))
        .route("/api/audio-work/{id}/summary/generations", post(move |Json(body): Json<Value>| {
            captured.lock().unwrap().push(body["request_id"].as_str().unwrap().into());
            let first = count.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if first {
                    (axum::http::StatusCode::ACCEPTED, "{".to_string())
                } else {
                    (axum::http::StatusCode::OK, json!({"generation_id": "33333333-3333-4333-8333-333333333333", "execution_id": "44444444-4444-4444-8444-444444444444", "work_id": WORK, "source_revision": 1, "status": "running", "sequence": 0, "text": "", "durable": false, "remote_outcome_unknown": true, "error": null}).to_string())
                }
            }
        }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(WORK.into()).await;
    controller.start("22222222-2222-4222-8222-222222222222".into(), true).await;
    assert!(controller.view().error.is_some());
    controller.start("55555555-5555-4555-8555-555555555555".into(), true).await;
    assert_eq!(*ids.lock().unwrap(), vec!["22222222-2222-4222-8222-222222222222", "22222222-2222-4222-8222-222222222222"]);
    assert_eq!(controller.view().generation_id.as_deref(), Some("33333333-3333-4333-8333-333333333333"));
    server.abort();
}

#[tokio::test]
async fn transient_outage_resumes_from_durable_snapshot_not_control_sequence() {
    use llm_core::{ApplicationFrame, Limits, Persistence, PublicError, StreamIdentity, StreamStatus, TextEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    const WORK: &str = "11111111-1111-4111-8111-111111111111";
    const GENERATION: &str = "33333333-3333-4333-8333-333333333333";
    const EXECUTION: &str = "44444444-4444-4444-8444-444444444444";
    let watches = Arc::new(AtomicUsize::new(0));
    let watched = watches.clone();
    let routes = Router::new()
        .route("/api/audio-work", get(|| async { Json(json!({"works": [work(WORK, "completed", Some("Completed transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null, "generation": {"generation_id": GENERATION, "execution_id": EXECUTION, "work_id": WORK, "source_revision": 1, "status": "running", "sequence": 0, "text": "", "durable": true, "remote_outcome_unknown": true, "error": null}})) }))
        .route("/api/audio-work/{id}/summary/generations/{generation}/watch", get(move |axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, u64>>| {
            let number = watched.fetch_add(1, Ordering::SeqCst);
            async move {
                assert_eq!(query.get("after").copied(), Some(if number == 0 { 0 } else { 1 }));
                let identity = StreamIdentity { generation: GENERATION.into(), content_revision: 1, consent_epoch: 0, execution: EXECUTION.into() };
                let events = if number == 0 {
                    vec![
                        (1, TextEvent::Snapshot { text: "Hello".into(), status: StreamStatus::Streaming, persistence: Persistence::Saved }),
                        (2, TextEvent::Interrupted(PublicError { code: "unavailable".into(), message: "Reconnect to reconcile the saved generation.".into() })),
                    ]
                } else {
                    vec![(1, TextEvent::Snapshot { text: "Hello".into(), status: StreamStatus::Streaming, persistence: Persistence::Saved }),
                         (3, TextEvent::Final { text: "Hello world".into(), persistence: Persistence::Saved })]
                };
                let wire: String = events.into_iter().map(|(seq, event)| llm_http::encode_frame(&ApplicationFrame { identity: identity.clone(), seq, event }, Limits::default()).unwrap()).collect();
                axum::response::Response::builder().header("content-type", "text/event-stream").body(axum::body::Body::from(wire)).unwrap()
            }
        }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(WORK.into()).await;
    controller.watch(|_| {}).await;
    assert!(controller.view().can_resume);
    assert!(!controller.view().can_start);
    controller.watch(|_| {}).await;
    assert_eq!(controller.view().text, "Hello world");
    assert!(controller.view().durable);
    assert_eq!(watches.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn watch_limit_refusal_stays_resumable_without_new_generation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    const WORK: &str = "11111111-1111-4111-8111-111111111111";
    let starts = Arc::new(AtomicUsize::new(0));
    let started = starts.clone();
    let routes = Router::new()
        .route("/api/audio-work", get(|| async { Json(json!({"works": [work(WORK, "completed", Some("Completed transcript"))]})) }))
        .route("/api/audio-work/{id}/summary", get(|| async { Json(json!({"summary": null, "generation": {"generation_id": "33333333-3333-4333-8333-333333333333", "execution_id": "44444444-4444-4444-8444-444444444444", "work_id": WORK, "source_revision": 1, "status": "running", "sequence": 1, "text": "Hello", "durable": true, "remote_outcome_unknown": true, "error": null}})) }))
        .route("/api/audio-work/{id}/summary/generations", post(move || { started.fetch_add(1, Ordering::SeqCst); async { axum::http::StatusCode::INTERNAL_SERVER_ERROR } }))
        .route("/api/audio-work/{id}/summary/generations/{generation}/watch", get(|| async {
            (axum::http::StatusCode::TOO_MANY_REQUESTS, Json(json!({"error": {"code": "watch_limit", "message": "Too many live summary views are attached. Close one, then resume."}})))
        }));
    let (base, server) = serve(routes).await;
    let controller = SummaryController::new(Arc::new(HttpApi::new(base, signed_in())));
    controller.refresh().await;
    controller.select(WORK.into()).await;
    controller.watch(|_| {}).await;
    let view = controller.view();
    assert_eq!(view.error.as_deref(), Some("Too many live summary views are attached. Close one, then resume."));
    assert!(view.can_resume);
    assert!(!view.can_start);
    assert_eq!(view.text, "Hello");
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    server.abort();
}
