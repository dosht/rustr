//! The public API routes (P7): key-authenticated transcription routes, session key management,
//! and the administrators' callback replay. Every route is mounted from `transgate_api::openapi::OPERATIONS`
//! by operation id, so the documented table and the router cannot disagree. A handler authenticates,
//! parses through `transgate-api`, calls exactly one core use case and maps its answer or `ApiError`
//! to HTTP. Scopes, limits, idempotency and key lifecycle are core's (`transgate_core::api`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, delete, get, post};
use serde::Serialize;
use serde_json::Value;
use transgate_api::openapi::{OPERATIONS, document};
use transgate_api::public;
use transgate_core::api::app::Api;
use transgate_core::api::callbacks::DeliveryId;
use transgate_core::api::{Action, ApiError, Actor, KeyId, Resource};
use transgate_core::ids::Micros;
use uuid::Uuid;

use crate::{HttpError, TransgateHttp};

type Handler = MethodRouter<Arc<TransgateHttp>>;

/// The handler of one operation. A documented operation without a handler is a startup panic, and
/// the router test also proves the reverse: nothing is mounted that the table does not list.
fn handler_for(id: &str) -> Handler {
    match id {
        "startTranscriptions" => post(start_transcriptions),
        "getTranscription" => get(get_transcription),
        "listApiKeys" => get(list_api_keys),
        "createApiKey" => post(create_api_key),
        "revokeApiKey" => delete(revoke_api_key),
        "getSigningSecret" => get(get_signing_secret),
        "replayCallback" => post(replay_callback),
        other => panic!("operation `{other}` is documented but has no handler"),
    }
}

pub fn router() -> Router<Arc<TransgateHttp>> {
    let mut by_path: BTreeMap<&str, Handler> = BTreeMap::new();
    for op in OPERATIONS {
        let handler = handler_for(op.id);
        let merged = match by_path.remove(op.path) {
            Some(existing) => existing.merge(handler),
            None => handler,
        };
        by_path.insert(op.path, merged);
    }
    let mut router = Router::new();
    for (path, handler) in by_path {
        router = router.route(path, handler);
    }
    router
        .route("/api/v1/openapi.json", get(openapi_json))
        .route("/openapi/openapi.yaml", get(openapi_yaml))
}

// ---- documents -----------------------------------------------------------------

fn spec_bytes() -> &'static Bytes {
    static SPEC: std::sync::OnceLock<Bytes> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Bytes::from(serde_json::to_vec_pretty(&document()).unwrap_or_default()))
}

async fn openapi_json() -> Response {
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "public, max-age=300")], spec_bytes().clone()).into_response()
}

/// The same document at the URL P6's docs and existing clients use. JSON is a subset of YAML 1.2,
/// so the one generated document is served verbatim under the YAML media type.
async fn openapi_yaml() -> Response {
    ([(header::CONTENT_TYPE, "application/yaml"), (header::CACHE_CONTROL, "public, max-age=300")], spec_bytes().clone()).into_response()
}

// ---- helpers -------------------------------------------------------------------

fn api(state: &TransgateHttp) -> Result<&Arc<Api>, HttpError> {
    state.api.as_ref().ok_or_else(|| crate::error_value(StatusCode::NOT_FOUND, "not_found", "Not found"))
}

/// A refusal that needs no header: the status and the body `fail` builds.
fn refusal(error: &ApiError) -> HttpError {
    let body = serde_json::to_value(public::error_body(error)).unwrap_or(Value::Null);
    (StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), axum::Json(body))
}

fn json_response<T: Serialize>(status: StatusCode, value: &T, extra: Vec<(&'static str, String)>) -> Response {
    let mut response = match serde_json::to_value(value) {
        Ok(body) => (status, axum::Json(body)).into_response(),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "An error occurred"),
    };
    for (name, value) in extra {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::from_str(&value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A use case's failure. A refusal by the limiter carries the window headers.
fn fail(state: &TransgateHttp, error: ApiError) -> Response {
    let status = StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if status.is_server_error() {
        eprintln!("transgate api: {error:?}");
    }
    let extra = match &error {
        ApiError::RateLimited(rate) => public::rate_headers(rate, state.clock.now(), true),
        _ => Vec::new(),
    };
    json_response(status, &public::error_body(&error), extra)
}

/// `Authorization: Bearer <value>`; anything else is an empty value, which core refuses as a bad key
/// without a store read. There is no fallback from a bad key to a session (API-001).
fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(str::trim)
        .unwrap_or("")
}

async fn key_actor(api: &Api, headers: &HeaderMap) -> Result<Actor, HttpError> {
    api.authenticate(bearer(headers)).await.map_err(|e| refusal(&e))
}

/// A signed-in browser session as the actor of the key screens.
async fn session_actor(state: &TransgateHttp, headers: &HeaderMap) -> Result<(Actor, billing_http::Caller), HttpError> {
    match crate::authenticate(state, headers, crate::Denied::Json).await {
        Ok(caller) => Ok((Actor::Session { account: caller.account }, caller)),
        Err((error, denied)) => Err(crate::authentication_error(error, denied)),
    }
}

fn elapsed_ms(state: &TransgateHttp, started: Micros) -> u64 {
    ((state.clock.now().0 - started.0).max(0) / 1000) as u64
}

// ---- key-authenticated transcription routes ------------------------------------

async fn start_transcriptions(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let started = state.clock.now();
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let actor = match key_actor(&api, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    let request = match public::parse_public_start(&body) {
        Ok(request) => request,
        Err(message) => return fail(&state, ApiError::Invalid(message)),
    };
    let idempotency = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_string);
    let admitted = match api.start(&actor, request, idempotency).await {
        Ok(admitted) => admitted,
        Err(e) => return fail(&state, e),
    };
    let rate = public::rate_headers(&admitted.rate, state.clock.now(), false);
    match admitted.result {
        Ok(answer) => {
            let request_id = Uuid::new_v4().to_string();
            let mut extra = rate;
            extra.push(("X-Request-ID", request_id.clone()));
            if answer.replayed {
                extra.push(("Idempotency-Replayed", "true".into()));
            }
            json_response(StatusCode::CREATED, &public::public_start_response(&answer, &request_id, state.clock.now(), elapsed_ms(&state, started)), extra)
        }
        Err(e) => {
            let mut response = fail(&state, e);
            for (name, value) in rate {
                if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::from_str(&value)) {
                    response.headers_mut().insert(name, value);
                }
            }
            response
        }
    }
}

async fn get_transcription(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    let started = state.clock.now();
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let actor = match key_actor(&api, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    let id = match public::parse_job_id(query.get("jobId").map(String::as_str)) {
        Ok(id) => id,
        Err(message) => return fail(&state, ApiError::Invalid(message)),
    };
    // The key's own account is the owner asked about: scope and limit are decided first, and a job
    // that is not the key's is then simply not found (never a 403 that confirms it exists).
    let account = actor.account();
    let rate = match api.authorize(&actor, Action::TranscriptionRead, Resource::Job { id, owner: account }).await {
        Ok(rate) => rate,
        Err(e) => return fail(&state, e),
    };
    let extra = public::rate_headers(&rate, state.clock.now(), false);
    let detail = match state.jobs.detail(account, id).await {
        Ok(detail) => detail,
        Err(e) => {
            let mut response = fail(&state, e.into());
            for (name, value) in extra {
                if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::from_str(&value)) {
                    response.headers_mut().insert(name, value);
                }
            }
            return response;
        }
    };
    let summary = match &state.ai {
        Some(ai) => ai.completion_summary(account, id).await.ok().flatten(),
        None => None,
    };
    let audio_url = state.jobs.callback_audio_url(&detail.job).await;
    let request_id = Uuid::new_v4().to_string();
    let mut extra = extra;
    extra.push(("X-Request-ID", request_id.clone()));
    let body = public::status_response(&detail.job, &detail.lines, summary.as_ref(), audio_url, &request_id, state.clock.now(), elapsed_ms(&state, started));
    json_response(StatusCode::OK, &body, extra)
}

// ---- session key management ------------------------------------------------------

async fn list_api_keys(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let (actor, _) = match session_actor(&state, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    match api.list_keys(&actor).await {
        Ok(keys) => json_response(StatusCode::OK, &public::KeyListResponse { keys: keys.iter().map(public::key_view).collect() }, vec![]),
        Err(e) => fail(&state, e),
    }
}

async fn create_api_key(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let (actor, _) = match session_actor(&state, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    let new = match public::parse_new_key(&body) {
        Ok(new) => new,
        Err(message) => return fail(&state, ApiError::Invalid(message)),
    };
    match api.create_key(&actor, new).await {
        Ok(created) => json_response(StatusCode::CREATED, &public::CreatedKeyResponse { key: public::key_view(&created.key), api_key: created.api_key }, vec![]),
        Err(e) => fail(&state, e),
    }
}

async fn revoke_api_key(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let (actor, _) = match session_actor(&state, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    // A value that is not an id names no key: the same 404 as another account's key.
    let Ok(id) = Uuid::parse_str(&id) else { return fail(&state, ApiError::NotFound) };
    match api.revoke_key(&actor, KeyId(id)).await {
        Ok(key) => json_response(StatusCode::OK, &public::RevokedKeyResponse { key: public::key_view(&key) }, vec![]),
        Err(e) => fail(&state, e),
    }
}

async fn get_signing_secret(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap) -> Response {
    let api = match api(&state) { Ok(api) => api.clone(), Err(r) => return r.into_response() };
    let (actor, _) = match session_actor(&state, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    match api.signing_secret(&actor).await {
        Ok(secret) => json_response(StatusCode::OK, &public::SigningSecretResponse { secret }, vec![]),
        Err(e) => fail(&state, e),
    }
}

// ---- administrators ---------------------------------------------------------------

async fn replay_callback(State(state): State<Arc<TransgateHttp>>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    let (_, caller) = match session_actor(&state, &headers).await { Ok(a) => a, Err(r) => return r.into_response() };
    let Some(callbacks) = state.callbacks.clone() else { return crate::error(StatusCode::NOT_FOUND, "not_found", "Not found") };
    // The operator is the verified admin identity, never a request field. A non-admin is refused
    // before the delivery id is even read.
    if !caller.is_admin {
        return fail(&state, ApiError::Forbidden);
    }
    let Ok(id) = Uuid::parse_str(&id) else { return fail(&state, ApiError::NotFound) };
    match callbacks.replay(&caller.email, DeliveryId(id)).await {
        Ok(()) => json_response(StatusCode::OK, &public::ReplayResponse { status: "queued".into() }, vec![]),
        Err(e) => fail(&state, e),
    }
}
