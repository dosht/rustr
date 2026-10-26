//! Additive audio-work summary routes. The shared application codec is the only SSE envelope.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{Router, body::{Body, Bytes}, extract::{Path, Query, State}, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}};
use billing_http::{AuthError, BillingIdentity, Caller};
use futures_util::stream;
use llm_core::{ApplicationFrame, Limits, PublicError, StreamIdentity, TextEvent};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::app::{FENCE_POLL, SummaryService, WatchPermit, frame, stream_identity};
use super::error::SummaryError;

pub struct SummaryHttp { pub service: Arc<SummaryService>, pub identity: Arc<dyn BillingIdentity> }

pub fn router(state: Arc<SummaryHttp>) -> Router {
    Router::new()
        .route("/api/audio-work/{work}/summary", get(summary))
        .route("/api/audio-work/{work}/summary/generations", post(start))
        .route("/api/audio-work/{work}/summary/generations/{generation}", get(generation))
        .route("/api/audio-work/{work}/summary/generations/{generation}/watch", get(watch))
        .route("/api/audio-work/{work}/summary/generations/{generation}/cancel", post(cancel))
        .with_state(state)
}

/// A small error value; handlers turn it into a `Response` at the edge.
type HttpError = (StatusCode, axum::Json<Value>);

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> HttpError {
    (status, axum::Json(json!({"error": {"code": code, "message": message.into()}})))
}
fn failure(e: SummaryError) -> HttpError {
    let (status, code) = match &e {
        SummaryError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        SummaryError::NotEnrolled => (StatusCode::FORBIDDEN, "not_enrolled"),
        SummaryError::Erased => (StatusCode::GONE, "erased"),
        SummaryError::NotReady => (StatusCode::CONFLICT, "not_ready"),
        SummaryError::Conflict => (StatusCode::CONFLICT, "conflict"),
        SummaryError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid"),
        SummaryError::NotConfigured => (StatusCode::SERVICE_UNAVAILABLE, "not_configured"),
        SummaryError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    };
    if let SummaryError::Unavailable(_) = e {
        eprintln!("taskr summary: {e}");
        error(status, code, "Audio summary storage is unavailable.")
    } else { error(status, code, e.to_string()) }
}
async fn authenticate(state: &SummaryHttp, headers: &HeaderMap) -> Result<Caller, HttpError> {
    state.identity.authenticate(headers).await.map_err(|e| match e {
        AuthError::Unauthenticated(_) => error(StatusCode::UNAUTHORIZED, "unauthenticated", "Your session has ended. Sign in again."),
        AuthError::AccountErased(_) => failure(SummaryError::Erased),
        AuthError::Unavailable(_) => error(StatusCode::SERVICE_UNAVAILABLE, "identity_unavailable", "The sign-in service is not reachable."),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start { request_id: Uuid, external_processing_accepted: bool }
async fn start(State(state): State<Arc<SummaryHttp>>, headers: HeaderMap, Path(work): Path<Uuid>, body: Bytes) -> Response {
    let caller = match authenticate(&state, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let request: Start = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return failure(SummaryError::Invalid("Send request_id and external_processing_accepted.".into())).into_response(),
    };
    match state.service.start(caller.account.0, work, request.request_id, request.external_processing_accepted).await {
        Ok((g, created)) => (if created { StatusCode::ACCEPTED } else { StatusCode::OK }, axum::Json(g)).into_response(),
        Err(e) => failure(e).into_response(),
    }
}
async fn summary(State(state): State<Arc<SummaryHttp>>, headers: HeaderMap, Path(work): Path<Uuid>) -> Response {
    let caller = match authenticate(&state, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match state.service.summary(caller.account.0, work).await { Ok(s) => axum::Json(s).into_response(), Err(e) => failure(e).into_response() }
}
async fn generation(State(state): State<Arc<SummaryHttp>>, headers: HeaderMap, Path((work, id)): Path<(Uuid, Uuid)>) -> Response {
    let caller = match authenticate(&state, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match state.service.generation(caller.account.0, work, id).await { Ok(g) => axum::Json(g).into_response(), Err(e) => failure(e).into_response() }
}
async fn cancel(State(state): State<Arc<SummaryHttp>>, headers: HeaderMap, Path((work, id)): Path<(Uuid, Uuid)>) -> Response {
    let caller = match authenticate(&state, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match state.service.cancel(caller.account.0, work, id).await { Ok(g) => axum::Json(g).into_response(), Err(e) => failure(e).into_response() }
}

/// An idle attachment writes an SSE comment this often: hyper notices a closed HTTP/1 client only
/// on a write, and the comment releases that attachment's slot. Decoders ignore it; no seq moves.
const HEARTBEAT: Duration = Duration::from_secs(5);
const HEARTBEAT_BYTES: &[u8] = b": keep-alive\n\n";

#[derive(Deserialize)]
struct WatchQuery { after: Option<u64> }
struct Attachment {
    state: Arc<SummaryHttp>, headers: HeaderMap, account: Uuid, work: Uuid, id: Uuid,
    identity: StreamIdentity, sequence: u64, first: bool, done: bool, idle: Instant,
    /// Released when the response body is dropped: disconnect, terminal or control frame.
    _permit: WatchPermit,
}
async fn watch(State(state): State<Arc<SummaryHttp>>, headers: HeaderMap, Path((work, id)): Path<(Uuid, Uuid)>, Query(query): Query<WatchQuery>) -> Response {
    let caller = match authenticate(&state, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let account = caller.account.0;
    let first = match state.service.generation(account, work, id).await { Ok(g) => g, Err(e) => return failure(e).into_response() };
    if query.after.is_some_and(|cursor| cursor > first.sequence) {
        return failure(SummaryError::Invalid("The resume cursor is ahead of the saved generation.".into())).into_response();
    }
    // Immediate and explicit; the client keeps its view and may resume later. Never queued.
    let Some(permit) = state.service.admit_watch(account) else {
        return error(StatusCode::TOO_MANY_REQUESTS, "watch_limit", "Too many live summary views are attached. Close one, then resume.").into_response();
    };
    let attachment = Attachment {
        identity: stream_identity(&first), sequence: first.sequence, first: true, state, headers, account, work, id,
        done: false, idle: Instant::now(), _permit: permit,
    };
    // Backpressure is pull-based: no token log, unbounded subscriber channel or accumulating
    // snapshots. While a subscriber is stalled, no additional snapshot is loaded for it.
    let frames = stream::unfold(attachment, |mut a| async move {
        if a.done { return None; }
        // Even the first body pull rechecks authority: a response can sit behind backpressure
        // after its pre-stream validation. Keep identity/cursor only, not a stale content copy.
        let next = loop {
            if !a.first { tokio::time::sleep(FENCE_POLL).await; }
            let identity = match a.state.identity.authenticate(&a.headers).await {
                Ok(c) if c.account.0 == a.account => None,
                Err(AuthError::Unavailable(_)) => Some(TextEvent::Interrupted(PublicError { code: "identity_unavailable".into(), message: "The sign-in service is not reachable.".into() })),
                _ => Some(TextEvent::Revoked),
            };
            if let Some(event) = identity {
                a.done = true;
                break ApplicationFrame { identity: a.identity.clone(), seq: a.sequence + 1, event };
            }
            // Fresh barrier/ownership/source facts every poll; an unchanged generation is read
            // without the account or row locks, so polling never waits behind account work.
            match a.state.service.generation(a.account, a.work, a.id).await {
                Ok(g) if !a.first && g.sequence == a.sequence => {
                    if a.idle.elapsed() >= HEARTBEAT {
                        a.idle = Instant::now();
                        return Some((Ok::<_, Infallible>(Bytes::from_static(HEARTBEAT_BYTES)), a));
                    }
                    continue;
                }
                Ok(g) => {
                    a.first = false;
                    a.sequence = g.sequence;
                    a.identity = stream_identity(&g);
                    a.done = g.status.terminal();
                    break frame(g);
                }
                Err(SummaryError::Erased | SummaryError::NotFound | SummaryError::NotEnrolled) => {
                    a.done = true;
                    break ApplicationFrame { identity: a.identity.clone(), seq: a.sequence + 1, event: TextEvent::Revoked };
                }
                Err(_) => {
                    a.done = true;
                    break ApplicationFrame { identity: a.identity.clone(), seq: a.sequence + 1,
                        event: TextEvent::Interrupted(PublicError { code: "unavailable".into(), message: "The saved generation cannot be read. Reconnect to reconcile it.".into() }) };
                }
            }
        };
        a.idle = Instant::now();
        let encoded = match llm_http::encode_frame(&next, Limits::default()) { Ok(bytes) => bytes, Err(_) => return None };
        Some((Ok::<_, Infallible>(Bytes::from(encoded)), a))
    });
    ([("content-type", "text/event-stream"), ("cache-control", "no-store"), ("x-accel-buffering", "no")], Body::from_stream(frames)).into_response()
}

#[cfg(test)]
mod tests;
