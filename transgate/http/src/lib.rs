//! Thin Axum routes over the core use cases. A handler authenticates, parses
//! through `transgate-api`, calls exactly one use case and maps its answer or its
//! `JobsError` to HTTP. No business rule lives here: ownership, the erasure
//! barrier, the blocked flag, limits and every state change are core's.
//!
//! Mounted: the source's session routes (`/api/init-upload`, `/api/streaming-url`,
//! `/api/youtube/*`, `/api/delete-account`), `/api/jobs/start` in job-id mode,
//! the two provider callbacks, the two cron triggers, and the job, transcript,
//! export and settings routes the shells need. Not mounted: the legacy
//! `/api/transcripts` and public `/api/v1/transcriptions`, which need P7's
//! API-key authenticator.

pub mod account;
pub mod ai;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, patch, post};
use billing_core::types::{BoxFuture, Clock};
use billing_http::{AuthError, BillingIdentity, Caller};
use media_ports::{SecretString, Transcriber};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use transgate_api::Transcriptions;
use transgate_core::export::{DownloadEvent, ExportLabels};
use transgate_core::jobs::app::{CalendarContext, ExportRequest, IntakeAck, Jobs, JobsError, NewDraft};
use transgate_core::jobs::ports::{JobQueries, RateDecision};
use transgate_core::privacy::app::{OperatorApproval, Privacy};
use transgate_core::privacy::erasure::ErasureView;
use transgate_core::public_transcript::PublicTranscript;
use transgate_core::transcript::LineEdit;
use transgate_core::{AccountId, AttemptId, JobId, Micros};

/// Provider callbacks can carry a whole transcript.
const CALLBACK_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// Where `transcript_downloaded` (EXP-016) goes. Delivery is the host's analytics port; a
/// failure to record never fails the download.
pub trait DownloadEvents: Send + Sync {
    fn downloaded<'a>(&'a self, account: AccountId, event: &'a DownloadEvent) -> BoxFuture<'a, ()>;
}

pub struct TransgateHttp {
    pub jobs: Arc<Jobs>,
    pub privacy: Arc<Privacy>,
    pub ai: Option<Arc<transgate_core::ai::Ai>>,
    /// One per process: it holds the six-hour public caption cache.
    pub public_transcript: Arc<PublicTranscript>,
    /// Read-only: the callback route loads the attempt a callback names.
    pub queries: Arc<dyn JobQueries>,
    pub transcribers: Vec<Arc<dyn Transcriber>>,
    pub identity: Arc<dyn BillingIdentity>,
    pub clock: Arc<dyn Clock>,
    /// `CRON_SECRET`. `None` refuses every cron call: nothing matches an unset secret.
    pub cron_secret: Option<SecretString>,
    pub downloads: Option<Arc<dyn DownloadEvents>>,
}

pub fn router(state: Arc<TransgateHttp>) -> Router {
    let ai_router = state.ai.as_ref().map(|service| ai::router(Arc::new(ai::AiHttp { ai: service.clone(), identity: state.identity.clone() })));
    let app = Router::new()
        .route("/api/init-upload", post(init_upload))
        .route("/api/streaming-url", post(streaming_url))
        .route("/api/jobs/start", post(transcriptions))
        .route("/api/youtube/add", post(youtube_add))
        .route("/api/youtube/metadata", post(youtube_metadata))
        .route("/api/youtube/resolve", post(youtube_resolve))
        .route("/api/youtube/public-transcript", post(public_transcript))
        .route("/api/delete-account", post(delete_account).delete(delete_account))
        .route("/api/admin/erasures/{erasure}/items/{item}/exception", post(approve_erasure_exception))
        .route("/api/soniox/callback", post(callback_soniox).layer(DefaultBodyLimit::max(CALLBACK_BODY_LIMIT)))
        .route("/api/deepgram/callback", post(callback_deepgram).layer(DefaultBodyLimit::max(CALLBACK_BODY_LIMIT)))
        .route("/api/cron/process-deepgram-queue", any(cron_deepgram_queue))
        .route("/api/cron/process-soniox-recovery", any(cron_soniox_recovery))
        .route("/api/jobs", get(list_jobs).post(create_draft).delete(delete_all))
        .route("/api/jobs/drafts", get(drafts))
        .route("/api/jobs/options", patch(set_draft_options))
        .route("/api/jobs/lines", patch(edit_line))
        .route("/api/jobs/{id}", get(job_detail).delete(delete_job))
        .route("/api/jobs/{id}/draft", delete(remove_draft))
        .route("/api/jobs/{id}/lines", get(job_lines))
        .route("/api/jobs/{id}/export", post(export_job))
        .route("/api/languages/usage", get(language_usage))
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .with_state(state);
    match ai_router { Some(ai) => app.merge(ai), None => app }
}

// ---- answers -------------------------------------------------------------------

type HttpRejection = (StatusCode, &'static str, &'static str);

/// Keep helper failures compact; construct the full response only at the handler edge.
type HttpError = (StatusCode, axum::Json<Value>);

fn error_value(status: StatusCode, code: &str, message: impl Into<String>) -> HttpError {
    (status, axum::Json(json!({"error": message.into(), "code": code})))
}

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    error_value(status, code, message).into_response()
}

fn invalid_value(message: impl Into<String>) -> HttpError {
    error_value(StatusCode::BAD_REQUEST, "invalid_request", message)
}

/// A 400 in the source's words.
fn invalid(message: impl Into<String>) -> Response {
    invalid_value(message).into_response()
}

fn json_value<T: serde::Serialize>(value: &T) -> Result<Value, HttpRejection> {
    serde_json::to_value(value).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "An error occurred"))
}

fn ok_json<T: serde::Serialize>(status: StatusCode, value: &T) -> Response {
    match json_value(value) {
        Ok(body) => (status, axum::Json(body)).into_response(),
        Err((status, code, message)) => error(status, code, message),
    }
}

/// `X-RateLimit-*`: limit and remaining always, the reset in whole seconds only on a refusal.
fn rate_headers(response: &mut Response, decision: &RateDecision, now: Micros, with_reset: bool) {
    let headers = response.headers_mut();
    for (name, value) in [("x-ratelimit-limit", i64::from(decision.limit)), ("x-ratelimit-remaining", i64::from(decision.remaining))] {
        if let Ok(value) = HeaderValue::from_str(&value.to_string()) {
            headers.insert(name, value);
        }
    }
    if with_reset {
        let seconds = ((decision.reset_at.0 - now.0).max(0) as u64).div_ceil(1_000_000);
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            headers.insert("x-ratelimit-reset", value);
        }
    }
}

/// A use case's failure. Storage details never reach the client: they are printed, not sent.
/// A billing account that does not exist owns no jobs: for these owned-job routes that is the
/// same `NotFound` as a job that is not the caller's. Nothing is provisioned to say so, and
/// every other storage failure stays a 500.
fn jobs_error(state: &TransgateHttp, e: JobsError) -> Response {
    owned_error_at(state.clock.as_ref(), e)
}

fn owned_error_at(clock: &dyn Clock, e: JobsError) -> Response {
    let e = match e {
        JobsError::Store(billing_core::ports::StoreError::AccountMissing(_)) => JobsError::NotFound,
        other => other,
    };
    jobs_error_at(clock, e)
}

fn jobs_error_at(clock: &dyn Clock, e: JobsError) -> Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let message = if status.is_server_error() && !matches!(e, JobsError::NotConfigured(_) | JobsError::Upstream(_)) {
        eprintln!("transgate: {e}");
        "An error occurred".to_string()
    } else {
        e.message()
    };
    let mut response = error(status, e.code(), message);
    if let JobsError::RateLimited(decision) = &e {
        rate_headers(&mut response, decision, clock.now(), true);
    }
    response
}

/// How a route words its 401, as the source did: three routes answer a bare string.
#[derive(Clone, Copy)]
enum Denied {
    Text,
    Json,
}

fn authentication_error(e: AuthError, denied: Denied) -> HttpError {
    match (e, denied) {
        (AuthError::Unauthenticated(_), Denied::Text) => (StatusCode::UNAUTHORIZED, axum::Json(json!("Not authorised"))),
        (AuthError::Unauthenticated(_), Denied::Json) => error_value(StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized"),
        (AuthError::AccountErased(_), _) => error_value(StatusCode::GONE, "account_deletion_in_progress", "This account is unavailable due to account erasure."),
        (AuthError::Unavailable(_), _) => error_value(StatusCode::SERVICE_UNAVAILABLE, "identity_unavailable", "The sign-in service is not reachable."),
    }
}

async fn authenticate(state: &TransgateHttp, headers: &HeaderMap, denied: Denied) -> Result<Caller, (AuthError, Denied)> {
    state.identity.authenticate(headers).await.map_err(|e| (e, denied))
}

macro_rules! session {
    ($state:expr, $headers:expr, $denied:expr) => {
        match authenticate(&$state, &$headers, $denied).await {
            Ok(caller) => caller,
            Err((error, denied)) => return authentication_error(error, denied).into_response(),
        }
    };
}

// ---- upload and playback -------------------------------------------------------

async fn init_upload(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Text);
    let file = match transgate_api::parse_job_file(&body) {
        Ok(file) => file,
        Err(message) => return invalid(message),
    };
    match state.jobs.init_upload(caller.account, file.job, &file.file_name).await {
        Ok(access) => (StatusCode::CREATED, axum::Json(Value::String(access.url.expose().to_string()))).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

async fn streaming_url(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Text);
    let file = match transgate_api::parse_job_file(&body) {
        Ok(file) => file,
        Err(message) => return invalid(message),
    };
    match state.jobs.streaming_url(caller.account, file.job, &file.file_name).await {
        Ok(access) => (StatusCode::CREATED, axum::Json(Value::String(access.url.expose().to_string()))).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

// ---- start ---------------------------------------------------------------------

async fn transcriptions(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let started = state.clock.now();
    match transgate_api::parse_transcriptions(&body) {
        Ok(Transcriptions::JobIds(request)) => match state.jobs.start(caller.account, request).await {
            Ok(answer) => {
                let elapsed = ((state.clock.now().0 - started.0).max(0) / 1000) as u64;
                let id = uuid::Uuid::new_v4().to_string();
                (StatusCode::CREATED, axum::Json(transgate_api::start_response(&answer, &id, state.clock.now(), elapsed, "web"))).into_response()
            }
            Err(e) => jobs_error(&state, e),
        },
        // The URL mode is for API customers: it waits for P7's API-key authenticator.
        Ok(Transcriptions::AudioUrls(_)) => invalid("audioUrls requests are only available for API customers"),
        Err(message) => invalid(message),
    }
}

// ---- YouTube -------------------------------------------------------------------

async fn youtube_add(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let request = match transgate_api::parse_youtube_add(&body) {
        Ok(request) => request,
        Err(message) => return invalid(message),
    };
    match state.jobs.youtube_add(caller.account, &request.url, request.language.as_deref()).await {
        Ok((job, metadata)) => match (json_value(&job), json_value(&metadata)) {
            (Ok(job), Ok(metadata)) => axum::Json(json!({"job": job, "metadata": metadata})).into_response(),
            (Err((status, code, message)), _) | (_, Err((status, code, message))) => error(status, code, message),
        },
        Err(e) => jobs_error(&state, e),
    }
}

async fn youtube_metadata(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let input = transgate_api::parse_youtube_metadata(&body).unwrap_or_default();
    match state.jobs.youtube_metadata(caller.account, &input).await {
        Ok(metadata) => ok_json(StatusCode::OK, &metadata),
        Err(e) => jobs_error(&state, e),
    }
}

async fn youtube_resolve(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let job = match transgate_api::parse_youtube_resolve(&body) {
        Ok(job) => job,
        Err(message) => return invalid(message),
    };
    match state.jobs.youtube_resolve(caller.account, job).await {
        Ok(answer) => axum::Json(transgate_api::resolve_response(&answer)).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

/// The first `x-forwarded-for` entry, as the source trusts its proxy; the socket address otherwise.
fn client_ip(headers: &HeaderMap, peer: &SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|first| first.trim().to_string())
        .filter(|first| !first.is_empty())
        .unwrap_or_else(|| peer.ip().to_string())
}

/// Anonymous captions: nothing is billed and no job is made. Limiter, cache and formats are core's.
async fn public_transcript(State(state): State<Arc<TransgateHttp>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Response {
    let map = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    let text = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_string);
    let ip = client_ip(&headers, &peer);
    let reply = match state.public_transcript.fetch(&ip, text("url").as_deref().unwrap_or(""), text("format").as_deref(), text("lang").as_deref()).await {
        Ok(reply) => reply,
        Err(e) => return jobs_error(&state, JobsError::Store(e)),
    };
    let now = state.clock.now();
    let denied = reply.result.as_ref().err().is_some_and(|e| e.http_status() == 429);
    let mut response = match &reply.result {
        Ok(answer) => axum::Json(json!({
            "videoId": answer.video_id,
            "language": answer.language,
            "segments": answer.segments,
            "text": answer.text,
            "format": answer.format.as_str(),
        }))
        .into_response(),
        Err(e) => error(StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::BAD_GATEWAY), e.code(), e.message()),
    };
    rate_headers(&mut response, &reply.rate, now, denied);
    response
}

// ---- account erasure -----------------------------------------------------------

/// Starts the account's erasure saga. 200 only when everything is already gone; otherwise 202 with
/// the honest status (what remains and which exceptions are recorded).
async fn delete_account(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let account = match state.identity.authenticate_for_erasure(&headers).await {
        Ok(account) => account,
        Err(e) => return authentication_error(e, Denied::Text).into_response(),
    };
    match state.privacy.request_erasure(account).await {
        Ok(view) => {
            let status = if view == ErasureView::FullyErased { StatusCode::OK } else { StatusCode::ACCEPTED };
            ok_json(status, &view)
        }
        Err(e) => jobs_error(&state, e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExceptionApprovalBody {
    rationale: String,
}

/// D15: operator approval is privileged, never part of the owner-only self-erasure bypass.
/// Authenticate and authorize before parsing approval data or calling the core.
async fn approve_erasure_exception(
    State(state): State<Arc<TransgateHttp>>,
    headers: HeaderMap,
    Path((erasure, item)): Path<(uuid::Uuid, uuid::Uuid)>,
    body: Bytes,
) -> Response {
    let caller = session!(state, headers, Denied::Json);
    if !caller.is_admin {
        return error(StatusCode::FORBIDDEN, "forbidden", "This action needs an administrator.");
    }
    let approval: ExceptionApprovalBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return invalid("Send only the exception rationale."),
    };
    let approval = OperatorApproval { operator: caller.email, rationale: approval.rationale };
    match state.privacy.approve_exception(transgate_core::ErasureId(erasure), item, &approval).await {
        Ok(()) => axum::Json(json!({"ok": true})).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

// ---- provider callbacks --------------------------------------------------------

#[derive(Deserialize)]
struct CallbackQuery {
    job_id: i64,
    attempt: uuid::Uuid,
}

fn callback_headers(headers: &HeaderMap) -> media_ports::Headers {
    headers.iter().filter_map(|(name, value)| value.to_str().ok().map(|v| (name.as_str().to_string(), v.to_string()))).collect()
}

async fn callback_soniox(state: State<Arc<TransgateHttp>>, query: Query<CallbackQuery>, headers: HeaderMap, body: Bytes) -> Response {
    callback(state.0, media_ports::Provider::Soniox, query.0, headers, body).await
}

async fn callback_deepgram(state: State<Arc<TransgateHttp>>, query: Query<CallbackQuery>, headers: HeaderMap, body: Bytes) -> Response {
    callback(state.0, media_ports::Provider::Deepgram, query.0, headers, body).await
}

/// The stored binding of the attempt the query names authenticates the call before anything in
/// the body is trusted; the observation is durable before the 2xx. Nothing of the request is logged.
async fn callback(state: Arc<TransgateHttp>, transport: media_ports::Provider, query: CallbackQuery, headers: HeaderMap, body: Bytes) -> Response {
    let unavailable = || {
        let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
        response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
        response
    };
    let attempt = match state.queries.attempt(AttemptId(query.attempt)).await {
        Ok(Some(attempt)) => attempt,
        // An attempt nobody issued has no binding to prove anything with.
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return unavailable(),
    };
    if attempt.job != JobId(query.job_id) || attempt.provider.transport() != Some(transport) {
        return StatusCode::CONFLICT.into_response();
    }
    let (Some(binding), Some(transcriber)) = (attempt.binding(), state.transcribers.iter().find(|t| t.provider() == transport)) else {
        return unavailable();
    };
    let observation = match transcriber.verify_callback(&binding, &callback_headers(&headers), &body) {
        Ok(observation) => observation,
        Err(media_ports::CallbackError::Unauthorized) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(media_ports::CallbackError::IdentityMismatch) => return StatusCode::CONFLICT.into_response(),
        Err(media_ports::CallbackError::Malformed) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let fingerprint = format!("{:x}", Sha256::digest(&body));
    match state.jobs.intake(&attempt, observation, fingerprint).await {
        Ok(IntakeAck::Stored { inbox }) => {
            // Apply now; the ingest pass picks it up if this does not finish.
            let jobs = state.jobs.clone();
            tokio::spawn(async move {
                let _ = jobs.ingest(inbox).await;
            });
            axum::Json(json!({"ok": true})).into_response()
        }
        // A repeat and an erased account are acknowledged: a retry cannot change either. A payload
        // that differs from the delivery already stored for this attempt and remote id is held and
        // never applied (the core keeps it), and the provider is told it conflicts, not that it was
        // taken as a replay.
        Ok(IntakeAck::Duplicate { .. } | IntakeAck::Erased) => axum::Json(json!({"ok": true})).into_response(),
        Ok(IntakeAck::Conflict { .. }) => StatusCode::CONFLICT.into_response(),
        Err(_) => unavailable(),
    }
}

// ---- cron ----------------------------------------------------------------------

fn cron_allowed(secret: Option<&SecretString>, headers: &HeaderMap) -> Result<(), HttpRejection> {
    let Some(secret) = secret else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "not_configured", "Cron is not configured"));
    };
    let presented = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    if bool::from(presented.as_bytes().ct_eq(secret.expose().as_bytes())) && !presented.is_empty() {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized"))
    }
}

fn cron_answer(message: &str, state: &TransgateHttp, reports: &[(&str, &transgate_core::jobs::app::PassReport)]) -> Response {
    let detail: serde_json::Map<String, Value> = reports.iter().filter_map(|(name, report)| serde_json::to_value(report).ok().map(|v| (name.to_string(), v))).collect();
    axum::Json(json!({
        "success": true,
        "message": message,
        "passes": detail,
        "timestamp": transgate_api::iso8601(state.clock.now()),
    }))
    .into_response()
}

/// Recover stuck jobs, then submit what is waiting (the source's order).
async fn cron_deepgram_queue(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    if let Err((status, code, message)) = cron_allowed(state.cron_secret.as_ref(), &headers) {
        return error(status, code, message);
    }
    let stuck = match state.jobs.stuck_pass().await {
        Ok(report) => report,
        Err(e) => return jobs_error(&state, e),
    };
    match state.jobs.dispatch_pass().await {
        Ok(dispatched) => cron_answer("Deepgram queue processed", &state, &[("stuck", &stuck), ("dispatch", &dispatched)]),
        Err(e) => jobs_error(&state, e),
    }
}

async fn cron_soniox_recovery(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    if let Err((status, code, message)) = cron_allowed(state.cron_secret.as_ref(), &headers) {
        return error(status, code, message);
    }
    let reconciled = match state.jobs.reconcile_pass().await {
        Ok(report) => report,
        Err(e) => return jobs_error(&state, e),
    };
    match state.jobs.stuck_pass().await {
        Ok(stuck) => cron_answer("Soniox stuck jobs processed", &state, &[("reconcile", &reconciled), ("stuck", &stuck)]),
        Err(e) => jobs_error(&state, e),
    }
}

// ---- jobs, transcripts and settings --------------------------------------------

#[derive(Deserialize)]
struct ListQuery {
    before: Option<i64>,
    #[serde(rename = "offsetNow", default)]
    offset_now: i32,
    #[serde(rename = "offsetThen", default)]
    offset_then: Option<i32>,
}

async fn list_jobs(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Query(query): Query<ListQuery>) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let calendar = CalendarContext { offset_now_minutes: query.offset_now, offset_then_minutes: query.offset_then.unwrap_or(query.offset_now) };
    match state.jobs.list(caller.account, query.before.map(JobId), calendar).await {
        Ok(page) => ok_json(StatusCode::OK, &page),
        Err(e) => jobs_error(&state, e),
    }
}

async fn drafts(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.drafts(caller.account).await {
        Ok(drafts) => ok_json(StatusCode::OK, &drafts),
        Err(error) => jobs_error(&state, error),
    }
}

async fn job_detail(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.detail(caller.account, JobId(id)).await {
        Ok(detail) => {
            let mut value = match json_value(&detail) {
                Ok(v) => v,
                Err((status, code, message)) => return error(status, code, message),
            };
            if let Some(ai) = &state.ai {
                match ai.completion_summary(caller.account, JobId(id)).await {
                    Ok(Some(summary)) => { value["summary"] = json!(summary.text); }
                    Ok(None) | Err(transgate_core::ai::AiError::ConsentRequired) => {}
                    Err(e @ (transgate_core::ai::AiError::Erased | transgate_core::ai::AiError::NotFound)) => return ai::failure(e),
                    // Optional AI availability never makes the completed transcription fail.
                    Err(e) => eprintln!("transgate optional job summary {id}: {e}"),
                }
            }
            ok_json(StatusCode::OK, &value)
        }
        Err(e) => jobs_error(&state, e),
    }
}

async fn job_lines(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.lines(caller.account, JobId(id)).await {
        Ok(lines) => ok_json(StatusCode::OK, &lines),
        Err(e) => jobs_error(&state, e),
    }
}

async fn create_draft(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let Ok(draft) = serde_json::from_slice::<NewDraft>(&body) else { return invalid("Invalid draft") };
    match state.jobs.create_draft(caller.account, draft).await {
        Ok(job) => ok_json(StatusCode::CREATED, &job),
        Err(e) => jobs_error(&state, e),
    }
}

async fn set_draft_options(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let (ids, options) = match transgate_api::parse_draft_options(&body) {
        Ok(parsed) => parsed,
        Err(message) => return invalid(message),
    };
    match state.jobs.set_draft_options(caller.account, &ids, options).await {
        Ok(jobs) => ok_json(StatusCode::OK, &jobs),
        Err(e) => jobs_error(&state, e),
    }
}

async fn remove_draft(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.remove_draft(caller.account, JobId(id)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

async fn delete_job(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.delete_job(caller.account, JobId(id)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

async fn delete_all(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.delete_all(caller.account).await {
        Ok(count) => axum::Json(json!({"deleted": count})).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

/// Last write wins (D7): the edit is applied as sent, with no revision to check.
async fn edit_line(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let Ok(edit) = serde_json::from_slice::<LineEdit>(&body) else { return invalid("Invalid edit") };
    match state.jobs.edit(caller.account, edit).await {
        Ok(updated) => axum::Json(json!({"updated": updated})).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

async fn language_usage(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.language_usage(caller.account).await {
        Ok(usage) => axum::Json(transgate_api::language_usage_response(&usage)).into_response(),
        Err(e) => jobs_error(&state, e),
    }
}

async fn get_settings(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let caller = session!(state, headers, Denied::Json);
    match state.jobs.settings(caller.account).await {
        Ok(settings) => ok_json(StatusCode::OK, &settings),
        Err(e) => jobs_error(&state, e),
    }
}

/// Only the auto-delete preference is user-writable (PRV-027); the legacy monthly toggle is
/// stored too (PRV-036). Nothing else in the body is read.
async fn patch_settings(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let map = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(map)) => map,
        _ => return invalid("Request body is required"),
    };
    let flag = |key: &str| map.get(key).and_then(Value::as_bool);
    if flag("autoDelete").is_none() && flag("deletedMonthly").is_none() {
        return invalid("autoDelete or deletedMonthly must be a boolean");
    }
    let mut last = None;
    if let Some(value) = flag("autoDelete") {
        match state.jobs.set_auto_delete(caller.account, value).await {
            Ok(settings) => last = Some(settings),
            Err(e) => return jobs_error(&state, e),
        }
    }
    if let Some(value) = flag("deletedMonthly") {
        match state.jobs.set_deleted_monthly(caller.account, value).await {
            Ok(settings) => last = Some(settings),
            Err(e) => return jobs_error(&state, e),
        }
    }
    match last {
        Some(settings) => ok_json(StatusCode::OK, &settings),
        None => invalid("autoDelete or deletedMonthly must be a boolean"),
    }
}

// ---- export --------------------------------------------------------------------

/// `Content-Disposition` with an ASCII fallback and the exact UTF-8 name (RFC 6266).
fn content_disposition(name: &str) -> String {
    let fallback: String = name.chars().map(|c| if c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '%') || c == ' ' { c } else { '_' }).collect();
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// A completed job's zip, built by core (names, text, Word model) and written by the byte adapters.
/// Free for every owner: there is no balance or blocked gate on a download (FRE-022).
async fn export_job(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<i64>, body: Bytes) -> Response {
    let caller = session!(state, headers, Denied::Json);
    let mut request = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => return invalid("Request body is required"),
    };
    if request.get("labels").is_none() {
        match json_value(&ExportLabels::default()) {
            Ok(labels) => request["labels"] = labels,
            Err((status, code, message)) => return error(status, code, message),
        }
    }
    let Ok(request) = serde_json::from_value::<ExportRequest>(request) else { return invalid("Invalid export request") };
    let (bundle, event) = match state.jobs.export(caller.account, JobId(id), request).await {
        Ok(exported) => exported,
        Err(e) => return jobs_error(&state, e),
    };
    let bytes = match transgate_export::zip_bytes(&bundle) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("transgate: {e}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "An error occurred");
        }
    };
    if let Some(events) = state.downloads.clone() {
        let account = caller.account;
        tokio::spawn(async move { events.downloaded(account, &event).await });
    }
    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(transgate_export::ZIP_CONTENT_TYPE));
    if let Ok(value) = HeaderValue::from_str(&content_disposition(&bundle.zip_name)) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

#[cfg(test)]
mod tests;
