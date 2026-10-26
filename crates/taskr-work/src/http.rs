//! Thin Axum routes over `app::Service`. Identity is the host's own verifier
//! (the same `BillingIdentity` the billing sample uses), so the account here is
//! the account billing erases. Provider callbacks are the only unauthenticated
//! route: the provider proves itself through the stored per-attempt secret.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use billing_http::{AuthError, BillingIdentity, Caller};
use media_ports::{Headers, Provider, TemporaryAccess};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::{CallbackOutcome, ErasureView, Service, ServiceError};
use crate::store::{parse_provider, provider_str};
use crate::work::Work;

/// Provider callbacks can carry a whole transcript.
const CALLBACK_BODY_LIMIT: usize = 64 * 1024 * 1024;

pub struct WorkHttp {
    pub service: Arc<Service>,
    pub identity: Arc<dyn BillingIdentity>,
}

pub fn router(state: Arc<WorkHttp>) -> Router {
    Router::new()
        .route("/api/audio-work/enroll", post(enroll))
        .route("/api/audio-work", get(list).post(create))
        .route("/api/audio-work/settings", put(settings))
        .route("/api/audio-work/account", axum::routing::delete(erase))
        .route("/api/audio-work/account/erasure", get(erasure))
        .route("/api/audio-work/{id}", get(show))
        .route("/api/audio-work/{id}/upload-url", post(upload_url))
        .route("/api/audio-work/{id}/uploaded", post(uploaded))
        .route("/api/audio-work/callback/{provider}", post(callback).layer(DefaultBodyLimit::max(CALLBACK_BODY_LIMIT)))
        .with_state(state)
}

/// A small error value; handlers turn it into a `Response` at the edge.
type HttpError = (StatusCode, axum::Json<Value>);

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> HttpError {
    (status, axum::Json(json!({"error": {"code": code, "message": message.into()}})))
}

fn service_error(e: ServiceError) -> HttpError {
    match e {
        ServiceError::NotEnrolled => error(StatusCode::FORBIDDEN, "not_enrolled", "Enroll in audio work first."),
        ServiceError::Erased => error(StatusCode::GONE, "erased", "This account is unavailable due to account erasure."),
        ServiceError::NotFound => error(StatusCode::NOT_FOUND, "not_found", "No such work."),
        ServiceError::NotReady(why) => error(StatusCode::CONFLICT, "not_ready", why),
        ServiceError::Invalid(why) => error(StatusCode::BAD_REQUEST, "invalid", why),
        ServiceError::Conflict => error(StatusCode::CONFLICT, "conflict", "The work changed. Try again."),
        ServiceError::Mismatch => error(StatusCode::CONFLICT, "identity_mismatch", "The provider named a different job than the one bound."),
        ServiceError::PayloadConflict => error(StatusCode::CONFLICT, "payload_conflict", "A different result was already delivered for this job."),
        ServiceError::Unavailable(why) => {
            eprintln!("taskr audio work: {why}");
            error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "Audio work is unavailable.")
        }
    }
}

async fn authenticate(state: &WorkHttp, headers: &HeaderMap) -> Result<Caller, HttpError> {
    state.identity.authenticate(headers).await.map_err(|e| match e {
        AuthError::Unauthenticated(_) => error(StatusCode::UNAUTHORIZED, "unauthenticated", "Your session has ended. Sign in again."),
        AuthError::AccountErased(_) => error(StatusCode::GONE, "erased", "This account is unavailable due to account erasure."),
        AuthError::Unavailable(_) => error(StatusCode::SERVICE_UNAVAILABLE, "identity_unavailable", "The sign-in service is not reachable."),
    })
}

#[derive(Serialize)]
struct AttemptView {
    number: u32,
    provider: &'static str,
    phase: &'static str,
    reason: Option<String>,
}

#[derive(Serialize)]
struct WorkView {
    id: Uuid,
    name: String,
    status: &'static str,
    language: Option<String>,
    attempts: Vec<AttemptView>,
    transcript: Option<String>,
    error: Option<String>,
    created_at_us: i64,
    completed_at_us: Option<i64>,
}

fn work_view(work: &Work) -> WorkView {
    WorkView {
        id: work.id,
        name: work.name.clone(),
        status: work.status.as_str(),
        language: work.language.clone(),
        attempts: work
            .attempts
            .iter()
            .map(|a| AttemptView { number: a.number, provider: provider_str(a.provider), phase: a.phase.as_str(), reason: a.reason.clone() })
            .collect(),
        transcript: work.transcript.clone(),
        error: work.error.clone(),
        created_at_us: work.created_at.0,
        completed_at_us: work.completed_at.map(|m| m.0),
    }
}

fn upload_view(upload: &TemporaryAccess) -> Value {
    json!({
        "url": upload.url.expose(),
        "method": "PUT",
        "headers": upload.headers.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
        "expires_at_us": upload.expires_at.0,
    })
}

fn erasure_view(view: &ErasureView) -> Value {
    json!({
        "complete": view.complete,
        "open": view.open.iter().map(|r| json!({"kind": r.kind, "state": r.state, "reason": r.reason})).collect::<Vec<_>>(),
    })
}

async fn enroll(State(state): State<Arc<WorkHttp>>, headers: HeaderMap) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.enroll(caller.account.0, &caller.email).await {
        Ok(enrollment) => axum::Json(json!({"enrolled": true, "auto_delete": enrollment.auto_delete})).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    name: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}

async fn create(State(state): State<Arc<WorkHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    let Ok(body) = serde_json::from_slice::<CreateBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid", "Send {\"name\": ..., \"language\"?: ..., \"provider\"?: \"soniox\"|\"deepgram\"}.").into_response();
    };
    let requested: Option<Provider> = match body.provider.as_deref() {
        None => None,
        Some(text) => match parse_provider(text) {
            Some(provider) => Some(provider),
            None => return error(StatusCode::BAD_REQUEST, "invalid", "provider must be soniox or deepgram.").into_response(),
        },
    };
    match state.service.create(caller.account.0, &body.name, body.language, requested).await {
        Ok(created) => {
            (StatusCode::CREATED, axum::Json(json!({"work": work_view(&created.work), "upload": upload_view(&created.upload)}))).into_response()
        }
        Err(e) => service_error(e).into_response(),
    }
}

async fn list(State(state): State<Arc<WorkHttp>>, headers: HeaderMap) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.list(caller.account.0).await {
        Ok(works) => axum::Json(json!({"works": works.iter().map(work_view).collect::<Vec<_>>()})).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

async fn show(State(state): State<Arc<WorkHttp>>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.get(caller.account.0, id).await {
        Ok(work) => axum::Json(work_view(&work)).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

async fn upload_url(State(state): State<Arc<WorkHttp>>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.reissue_upload(caller.account.0, id).await {
        Ok(upload) => axum::Json(upload_view(&upload)).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

async fn uploaded(State(state): State<Arc<WorkHttp>>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.confirm_upload(caller.account.0, id).await {
        Ok(work) => axum::Json(work_view(&work)).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsBody {
    auto_delete: bool,
}

async fn settings(State(state): State<Arc<WorkHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    let Ok(body) = serde_json::from_slice::<SettingsBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid", "Send {\"auto_delete\": true|false}.").into_response();
    };
    match state.service.set_auto_delete(caller.account.0, body.auto_delete).await {
        Ok(()) => axum::Json(json!({"auto_delete": body.auto_delete})).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

/// Starts the account's erasure. 202: the barrier is in place and the local data is gone; the
/// body lists what is still being removed outside the database.
async fn erase(State(state): State<Arc<WorkHttp>>, headers: HeaderMap) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.erase_account(caller.account.0).await {
        Ok(view) => (if view.complete { StatusCode::OK } else { StatusCode::ACCEPTED }, axum::Json(erasure_view(&view))).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

async fn erasure(State(state): State<Arc<WorkHttp>>, headers: HeaderMap) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    match state.service.erasure_view(caller.account.0).await {
        Ok(view) => axum::Json(erasure_view(&view)).into_response(),
        Err(e) => service_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    work: Uuid,
    attempt: Uuid,
}

fn callback_headers(headers: &HeaderMap) -> Headers {
    headers.iter().filter_map(|(name, value)| value.to_str().ok().map(|v| (name.as_str().to_string(), v.to_string()))).collect()
}

/// A provider's completion callback. Nothing in the answer says why a request was refused
/// beyond the status code, and nothing is logged from the request.
async fn callback(State(state): State<Arc<WorkHttp>>, Path(provider): Path<String>, Query(query): Query<CallbackQuery>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(provider) = parse_provider(&provider) else { return StatusCode::NOT_FOUND.into_response() };
    match state.service.callback(provider, query.work, query.attempt, &callback_headers(&headers), &body).await {
        CallbackOutcome::Applied | CallbackOutcome::Ignored | CallbackOutcome::Dropped => axum::Json(json!({"ok": true})).into_response(),
        CallbackOutcome::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
        CallbackOutcome::Mismatch | CallbackOutcome::Conflict => StatusCode::CONFLICT.into_response(),
        CallbackOutcome::Malformed => StatusCode::BAD_REQUEST.into_response(),
        CallbackOutcome::Unavailable => {
            let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
            response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
            response
        }
    }
}
