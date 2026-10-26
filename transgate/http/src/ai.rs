//! Session-only AI transport. Core owns admission, privacy, quotas, parsing,
//! checkpoint/final semantics and source event meanings. Attachments only
//! render freshly authorized snapshots; disconnect never cancels or dispatches.
use std::sync::Arc;
use std::time::Duration;

use axum::{Router, body::{Body, Bytes}, extract::{DefaultBodyLimit, Path, Query, State}, http::{HeaderMap, HeaderValue, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}};
use billing_http::{AuthError, BillingIdentity, Caller};
use futures_util::stream;
use llm_core::{ApplicationFrame, Limits, PublicError, StreamIdentity, TextEvent};
use serde::Deserialize;
use serde_json::{Value, json};
use transgate_core::{AccountId, JobId};
use transgate_core::ai::{Ai, AiError, GenerationId, GenerationKind, GenerationStatus, GenerationView, Started};
use transgate_core::ai::presentation::{AnalysisEvent, analysis_events, public_error};
use uuid::Uuid;

pub struct AiHttp { pub ai: Arc<Ai>, pub identity: Arc<dyn BillingIdentity> }

pub fn router(state: Arc<AiHttp>) -> Router {
    // Decoded input is bounded by core at16MiB; escaped JSON can be six times
    // larger. This is a wire bound, not another product/history truncation.
    Router::new()
        .route("/api/ai/consent", get(consent).post(record_consent).delete(revoke_consent))
        .route("/api/ai/chat-limits", get(limits))
        .route("/api/v1/ai/analyze", post(start_analysis))
        .route("/api/v1/ai/chat", post(start_chat))
        .route("/api/ai/jobs/{job}/analysis", get(analysis))
        .route("/api/ai/jobs/{job}/chat", get(history))
        .route("/api/ai/jobs/{job}/summary", get(summary))
        .route("/api/ai/generations/{id}", get(generation))
        .route("/api/ai/generations/{id}/watch", get(watch))
        .route("/api/ai/generations/{id}/cancel", post(cancel))
        .route("/api/ai/generations/{id}/retry", post(retry))
        .layer(DefaultBodyLimit::max(6 * Limits::default().input_bytes + 4096))
        .with_state(state)
}

async fn authenticate(state: &AiHttp, headers: &HeaderMap) -> Result<Caller, super::HttpError> {
    state.identity.authenticate(headers).await.map_err(|e| super::authentication_error(e, super::Denied::Json))
}

pub(super) fn failure(e: AiError) -> Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let AiError::Quota(q) = &e {
        return (status, axum::Json(json!({"error":q.message,"code":q.code,"current":q.current,"max":q.max,"resetDate":q.reset_at.map(transgate_api::iso8601),"resetPeriod":q.period}))).into_response();
    }
    if let AiError::Busy(id) = &e {
        return (status, axum::Json(json!({"error":e.message(),"code":"ai_busy","generationId":id}))).into_response();
    }
    let code = match &e {
        AiError::NotFound => "not_found",
        AiError::ConsentRequired => "consent_required",
        AiError::NoTranscript => "no_transcript",
        AiError::Disabled => "ai_disabled",
        AiError::ChatDisabled => "chat_disabled",
        AiError::NotConfigured => "not_configured",
        AiError::Invalid(_) => "invalid_request",
        AiError::Conflict => "request_conflict",
        AiError::WrongState(_) => "wrong_state",
        AiError::Erased => "account_deletion_in_progress",
        AiError::Capacity => "ai_capacity",
        AiError::Unavailable(_) => "unavailable",
        AiError::Quota(_) | AiError::Busy(_) => unreachable!(),
    };
    let message = if matches!(&e, AiError::Unavailable(_)) {
        eprintln!("transgate AI: {e}");
        "AI storage is unavailable".into()
    } else { e.message() };
    super::error(status, code, message)
}

fn request_key(headers: &HeaderMap, body: Option<Uuid>) -> Result<Uuid, super::HttpError> {
    let header = headers.get("idempotency-key").map(|h| h.to_str().ok().and_then(|s| Uuid::parse_str(s).ok()).ok_or_else(|| super::invalid_value("Idempotency-Key must be a UUID"))).transpose()?;
    if matches!((header, body), (Some(a), Some(b)) if a != b) { return Err(super::invalid_value("Idempotency-Key and requestId disagree")); }
    Ok(body.or(header).unwrap_or_else(Uuid::new_v4))
}

async fn consent(State(s): State<Arc<AiHttp>>, headers: HeaderMap) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.consent(c.account).await { Ok(v) => axum::Json(transgate_api::ai::consent_response(&v)).into_response(), Err(e) => failure(e) }
}
async fn record_consent(State(s): State<Arc<AiHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let decision = match transgate_api::ai::parse_consent(&body) { Ok(v) => v, Err(e) => return super::invalid(e) };
    match s.ai.record_consent(c.account, decision).await { Ok(v) => axum::Json(transgate_api::ai::consent_response(&v)).into_response(), Err(e) => failure(e) }
}
async fn revoke_consent(State(s): State<Arc<AiHttp>>, headers: HeaderMap) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.revoke_consent(c.account).await { Ok(v) => axum::Json(transgate_api::ai::consent_response(&v)).into_response(), Err(e) => failure(e) }
}
#[derive(Deserialize)]
struct LimitQuery { #[serde(rename = "jobId")] job: Option<i64> }
async fn limits(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Query(q): Query<LimitQuery>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let Some(job) = q.job.filter(|j| *j > 0) else { return super::invalid("Job ID is required") };
    match s.ai.limits(c.account, JobId(job)).await { Ok(v) => axum::Json(transgate_api::ai::limits_response(&v)).into_response(), Err(e) => failure(e) }
}

fn dispatch(s: &AiHttp, started: &Started) {
    if !started.created { return; }
    let ai = s.ai.clone();
    let id = started.generation.id;
    tokio::spawn(async move { if let Err(e) = ai.run(id).await { eprintln!("transgate AI generation {id}: {e}"); } });
}
async fn start_analysis(State(s): State<Arc<AiHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let input = match transgate_api::ai::parse_analysis(&body) { Ok(v) => v, Err(e) => return super::invalid(e) };
    let key = match request_key(&headers, input.request) { Ok(k) => k, Err(r) => return r.into_response() };
    let reservation = match s.ai.reserve_watch(c.account) { Ok(slot) => slot, Err(e) => return failure(e) };
    match s.ai.start_analysis(c.account, input.job, key).await {
        Ok(started) => {
            let response = attach(s.clone(), headers, c.account, started.generation.id, Mode::Analysis, None, Some(reservation)).await;
            dispatch(&s, &started);
            response
        }
        Err(e) => failure(e),
    }
}
async fn start_chat(State(s): State<Arc<AiHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let input = match transgate_api::ai::parse_chat(&body) { Ok(v) => v, Err(e) => return super::invalid(e) };
    let key = match request_key(&headers, input.request) { Ok(k) => k, Err(r) => return r.into_response() };
    let reservation = match s.ai.reserve_watch(c.account) { Ok(slot) => slot, Err(e) => return failure(e) };
    match s.ai.start_chat(c.account, input.job, key, input.chat).await {
        Ok(started) => {
            let response = attach(s.clone(), headers, c.account, started.generation.id, Mode::LegacyChat, None, Some(reservation)).await;
            dispatch(&s, &started);
            response
        }
        Err(e) => failure(e),
    }
}
async fn retry(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(id): Path<Uuid>, body: Bytes) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    let key = match transgate_api::ai::parse_retry(&body) { Ok(k) => k, Err(e) => return super::invalid(e) };
    let key = match request_key(&headers, Some(key)) { Ok(k) => k, Err(r) => return r.into_response() };
    match s.ai.retry(c.account, GenerationId(id), key).await {
        Ok(started) => { dispatch(&s, &started); super::ok_json(if started.created { StatusCode::ACCEPTED } else { StatusCode::OK }, &started) }
        Err(e) => failure(e),
    }
}
async fn generation(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.generation(c.account, GenerationId(id)).await { Ok(v) => super::ok_json(StatusCode::OK, &v), Err(e) => failure(e) }
}
async fn cancel(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.cancel(c.account, GenerationId(id)).await { Ok(v) => super::ok_json(StatusCode::OK, &v), Err(e) => failure(e) }
}
async fn analysis(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(job): Path<i64>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.analysis(c.account, JobId(job)).await { Ok(v) => super::ok_json(StatusCode::OK, &json!({"analysis":v})), Err(e) => failure(e) }
}
async fn history(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(job): Path<i64>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.chat_history(c.account, JobId(job)).await { Ok(v) => super::ok_json(StatusCode::OK, &json!({"messages":v})), Err(e) => failure(e) }
}
async fn summary(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(job): Path<i64>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    match s.ai.completion_summary(c.account, JobId(job)).await { Ok(v) => super::ok_json(StatusCode::OK, &json!({"summary":v})), Err(e) => failure(e) }
}
#[derive(Deserialize)]
struct WatchQuery { after: Option<u64> }
async fn watch(State(s): State<Arc<AiHttp>>, headers: HeaderMap, Path(id): Path<Uuid>, Query(q): Query<WatchQuery>) -> Response {
    let c = match authenticate(&s, &headers).await { Ok(c) => c, Err(r) => return r.into_response() };
    attach(s, headers, c.account, GenerationId(id), Mode::Shared, q.after, None).await
}

#[derive(Clone, Copy)]
enum Mode { Shared, Analysis, LegacyChat }
struct Attachment {
    state: Arc<AiHttp>, headers: HeaderMap, account: AccountId, id: GenerationId, mode: Mode,
    first: bool, sequence: i64, identity: Option<StreamIdentity>, analysis_previous: Option<GenerationView>, done: bool,
    // The core's private capacity guard must survive until this SSE body is dropped.
    _watch: transgate_core::ai::Watch,
}

fn named(name: &str, data: Value) -> String { format!("event: {name}\ndata: {data}\n\n") }
fn chat_event(data: Value) -> String { format!("data: {data}\n\n") }
fn analysis_bytes(previous: Option<&GenerationView>, v: &GenerationView) -> String {
    analysis_events(previous, v).into_iter().map(|e| match e {
        AnalysisEvent::SectionCount { total } => named("sectionCount", json!({"total":total})),
        AnalysisEvent::TypeDetected { kind, confidence } => named("typeDetected", json!({"type":kind,"confidence":confidence})),
        AnalysisEvent::SectionReady { section_type, data } => named("sectionReady", json!({"sectionType":section_type,"data":data})),
        AnalysisEvent::Complete { tokens_used, processing_time_ms } => named("complete", json!({"tokensUsed":tokens_used,"processingTime":processing_time_ms})),
        AnalysisEvent::Error { code, message } => named("error", json!({"code":code,"message":message})),
    }).collect()
}
fn legacy_chat_bytes(v: &GenerationView, frame: Option<ApplicationFrame>) -> String {
    let mut out = match frame.map(|f| f.event) {
        Some(TextEvent::Snapshot { text, .. } | TextEvent::Final { text, .. }) => chat_event(json!({"type":"snapshot","content":text,"metadata":{"generationId":v.id,"sequence":v.sequence,"durable":v.durable}})),
        _ => String::new(),
    };
    match v.status {
        GenerationStatus::Completed => out.push_str(&chat_event(json!({"type":"done","metadata":{"generationId":v.id,"processingTime":v.finished_at.map(|f|(f.0-v.created_at.0)/1000),"tokensUsed":v.tokens_used,"provider":"deepseek"}}))),
        GenerationStatus::Interrupted | GenerationStatus::Cancelled => {
            let e = v.interruption.as_ref().map(public_error).unwrap_or(PublicError { code: "cancelled".into(), message: "Cancelled".into() });
            out.push_str(&chat_event(json!({"type":"error","error":e.message,"metadata":{"errorCode":e.code,"generationId":v.id}})));
        }
        _ => {}
    }
    out
}

fn control(a: &Attachment, revoked: bool, e: PublicError) -> Result<String, std::io::Error> {
    match a.mode {
        Mode::Analysis => Ok(named("error", json!({"code":e.code,"message":e.message}))),
        Mode::LegacyChat => Ok(chat_event(json!({"type":"error","error":e.message,"metadata":{"errorCode":e.code}}))),
        Mode::Shared => {
            let identity = a.identity.clone().ok_or_else(|| std::io::Error::other("Generation has no stream execution; read its snapshot"))?;
            let event = if revoked { TextEvent::Revoked } else { TextEvent::Interrupted(e) };
            llm_http::encode_frame(&ApplicationFrame { identity, seq: u64::try_from(a.sequence).unwrap_or(0) + 1, event }, Limits::default()).map_err(std::io::Error::other)
        }
    }
}

async fn attach(s: Arc<AiHttp>, headers: HeaderMap, account: AccountId, id: GenerationId, mut mode: Mode, after: Option<u64>, reservation: Option<transgate_core::ai::WatchReservation>) -> Response {
    let watched = match reservation {
        Some(slot) => s.ai.attach(slot, account, id).await,
        None => s.ai.watch(account, id).await,
    };
    let mut watch = match watched { Ok(v) => v, Err(e) => return failure(e) };
    // Every attachment begins from a durable resnapshot. Discard the optional
    // raw subscription permanently, but keep Watch itself: it owns the live slot.
    drop(watch.frames.take());
    let v = &watch.snapshot;
    if v.kind == GenerationKind::Analysis { mode = Mode::Analysis; }
    if after.is_some_and(|n| n > u64::try_from(v.sequence).unwrap_or(0)) { return super::invalid("The resume cursor is ahead of the saved generation"); }
    if matches!(mode, Mode::Shared) && v.status.is_terminal() && v.execution.is_none() {
        return failure(AiError::WrongState("This generation did not dispatch; read its snapshot".into()));
    }
    let identity = v.execution.map(|execution| StreamIdentity {
        generation: v.id.to_string(), content_revision: u64::try_from(v.source_revision).unwrap_or(0),
        consent_epoch: u64::try_from(v.consent_epoch).unwrap_or(0), execution: execution.to_string(),
    });
    let sequence = v.sequence;
    // No stale payload is needed for the guard: every body pull fetches authorized state.
    watch.snapshot.draft = transgate_core::ai::Draft::Empty;
    watch.snapshot.final_content = None;
    let a = Attachment { state:s, headers, account, id, mode, first:true, sequence, identity, analysis_previous:None, done:false, _watch:watch };
    let body = stream::unfold(a, |mut a| async move {
        if a.done { return None; }
        loop {
            if !a.first { tokio::time::sleep(Duration::from_millis(100)).await; }
            let auth = match a.state.identity.authenticate(&a.headers).await {
                Ok(c) if c.account == a.account => None,
                Err(AuthError::Unavailable(_)) => Some((false, PublicError { code:"identity_unavailable".into(), message:"The sign-in service is not reachable.".into() })),
                _ => Some((true, PublicError { code:"revoked".into(), message:"This attachment is no longer authorized.".into() })),
            };
            if let Some((revoked, e)) = auth { a.done=true; return Some((control(&a, revoked, e).map(Bytes::from), a)); }
            let v = match a.state.ai.generation(a.account, a.id).await {
                Ok(v) => v,
                Err(e) => {
                    let revoked = matches!(e, AiError::ConsentRequired | AiError::Erased | AiError::NotFound);
                    let public = PublicError { code: if revoked {"revoked"} else {"unavailable"}.into(), message: if revoked {"This attachment is no longer authorized."} else {"The saved generation cannot be read. Reconnect to reconcile it."}.into() };
                    a.done=true;
                    return Some((control(&a, revoked, public).map(Bytes::from), a));
                }
            };
            if !a.first && v.sequence == a.sequence { continue; }
            a.first=false;
            a.sequence=v.sequence;
            a.done=v.status.is_terminal();
            let frame = v.frame();
            if let Some(f) = &frame { a.identity=Some(f.identity.clone()); }
            let bytes = match a.mode {
                Mode::Shared => match frame { Some(f) => llm_http::encode_frame(&f, Limits::default()).map_err(std::io::Error::other), None => Ok(String::new()) },
                Mode::Analysis => { let bytes=analysis_bytes(a.analysis_previous.as_ref(), &v); a.analysis_previous=Some(v); Ok(bytes) }
                Mode::LegacyChat => Ok(legacy_chat_bytes(&v, frame)),
            };
            match bytes {
                Ok(bytes) if bytes.is_empty() && a.done => return None,
                Ok(bytes) if bytes.is_empty() => continue,
                result => return Some((result.map(Bytes::from), a)),
            }
        }
    });
    let mut response = ([("content-type","text/event-stream"),("cache-control","no-store"),("x-accel-buffering","no")], Body::from_stream(body)).into_response();
    if let Ok(value)=HeaderValue::from_str(&id.to_string()) { response.headers_mut().insert("x-generation-id",value); }
    response
}
