//! Composition of the HTTP shell. `router` is everything a request touches;
//! `main.rs` only reads configuration and picks adapters.
//!
//! Each request gets a request id, and once the bearer token is verified, a
//! per-request `InProcessApi` whose telemetry carries the request id and the
//! user id, so every log line of one request can be found together.

pub mod identity;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use taskr_api::errors::{ErrorBody, WireError, from_tasks_error, from_workspaces_error};
use taskr_api::{AddTaskBody, MeDto, MemberDto, RemoveMemberBody, SetMemberBody, TaskDto, paths};
use taskr_core::api::InProcessApi;
use taskr_core::shared::access::{Identity, IdentityError, WorkspaceId};
use taskr_core::shared::clock::Clock;
use taskr_core::shared::telemetry::{LogEvent, Telemetry};
use taskr_core::tasks::app::{TaskListRepository, Tasks, TasksApi, TasksError};
use taskr_core::workspaces::app::{
    WorkspaceRepository, Workspaces, WorkspacesApi, WorkspacesError,
};

pub const BUILD: &str = match option_env!("TASKR_COMMIT") {
    Some(sha) => sha,
    None => "dev",
};

#[derive(Clone)]
pub struct Server {
    pub task_lists: Arc<dyn TaskListRepository>,
    pub workspaces: Arc<dyn WorkspaceRepository>,
    pub identity: Arc<dyn Identity>,
    pub clock: Arc<dyn Clock>,
    pub telemetry: Arc<dyn Telemetry>,
}

pub fn router(server: Server) -> Router {
    let api = Router::new()
        .route(paths::HEALTH, get(health))
        .route(paths::ME, get(me))
        .route(paths::routes::TASKS, get(list_tasks).post(add_task))
        .route(paths::routes::COMPLETE, post(complete_task))
        .route(paths::routes::REOPEN, post(reopen_task))
        .route(paths::routes::MEMBERS, get(list_members).put(set_member).delete(remove_member))
        .fallback(|| async {
            ApiError::new(StatusCode::NOT_FOUND, "no_route", "No such endpoint.")
        })
        .with_state(server.clone());
    Router::new().nest("/api", api).layer(middleware::from_fn_with_state(server, log_requests))
}

/// Adds the request id and, once known, the user to every event.
struct RequestTelemetry {
    inner: Arc<dyn Telemetry>,
    request_id: String,
    user: String,
}

impl Telemetry for RequestTelemetry {
    fn record(&self, event: LogEvent) {
        self.inner.record(
            event
                .field("request_id", &self.request_id)
                .field("user", &self.user)
                .field("build", BUILD),
        );
    }
}

#[derive(Clone)]
struct RequestContext {
    id: String,
    user: Arc<Mutex<Option<String>>>,
}

async fn log_requests(State(server): State<Server>, mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 64)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let user = Arc::new(Mutex::new(None));
    req.extensions_mut().insert(RequestContext { id: id.clone(), user: user.clone() });
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let started = Instant::now();
    let mut res = next.run(req).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", value);
    }
    let status = res.status().as_u16();
    let event = if status >= 500 {
        LogEvent::error("http.request")
    } else {
        LogEvent::info("http.request")
    };
    server.telemetry.record(
        event
            .field("request_id", &id)
            .field("user", user.lock().unwrap().clone().unwrap_or_default())
            .field("method", method)
            .field("path", path)
            .field("status", status)
            .field("ms", started.elapsed().as_millis())
            .field("build", BUILD),
    );
    res
}

struct ApiError(StatusCode, ErrorBody);

impl ApiError {
    fn new(status: StatusCode, code: &str, message: &str) -> Self {
        ApiError(
            status,
            ErrorBody {
                error: WireError { code: code.into(), message: message.into(), detail: None },
            },
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl From<TasksError> for ApiError {
    fn from(e: TasksError) -> Self {
        let (status, body) = from_tasks_error(&e);
        ApiError(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), body)
    }
}

impl From<WorkspacesError> for ApiError {
    fn from(e: WorkspacesError) -> Self {
        let (status, body) = from_workspaces_error(&e);
        ApiError(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), body)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(e: JsonRejection) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", &e.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(e: PathRejection) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", &e.body_text())
    }
}

/// The verified caller, as the core's in-process API for this request.
struct Caller(InProcessApi);

impl FromRequestParts<Server> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, server: &Server) -> Result<Self, ApiError> {
        let ctx =
            parts.extensions.get::<RequestContext>().cloned().expect("log_requests runs first");
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let unauthenticated = || {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "Your session has ended. Sign in again.",
            )
        };
        let Some(token) = token else { return Err(unauthenticated()) };
        let principal = match server.identity.verify(token).await {
            Ok(p) => p,
            Err(IdentityError::Invalid(why)) => {
                server.telemetry.record(
                    LogEvent::warn("auth.rejected")
                        .field("request_id", &ctx.id)
                        .field("reason", why),
                );
                return Err(unauthenticated());
            }
            Err(IdentityError::Unavailable(why)) => {
                server.telemetry.record(
                    LogEvent::error("auth.unavailable")
                        .field("request_id", &ctx.id)
                        .field("reason", &why),
                );
                return Err(ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "storage",
                    "The sign-in service is not reachable.",
                ));
            }
        };
        *ctx.user.lock().unwrap() = Some(principal.user_id.0.clone());
        let telemetry: Arc<dyn Telemetry> = Arc::new(RequestTelemetry {
            inner: server.telemetry.clone(),
            request_id: ctx.id,
            user: principal.user_id.0.clone(),
        });
        Ok(Caller(InProcessApi::new(
            Tasks::new(server.task_lists.clone(), server.clock.clone(), telemetry.clone()),
            Workspaces::new(server.workspaces.clone(), server.clock.clone(), telemetry),
            principal,
        )))
    }
}

type Reply<T> = Result<Json<T>, ApiError>;

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "build": BUILD }))
}

async fn me(caller: Caller) -> Reply<MeDto> {
    Ok(Json(caller.0.me().await?.into()))
}

async fn list_tasks(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
) -> Reply<Vec<TaskDto>> {
    let ws = WorkspaceId(ws?.0);
    Ok(Json(caller.0.list(&ws).await?.into_iter().map(Into::into).collect()))
}

async fn add_task(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
    body: Result<Json<AddTaskBody>, JsonRejection>,
) -> Result<(StatusCode, Json<TaskDto>), ApiError> {
    let ws = WorkspaceId(ws?.0);
    let task = caller.0.add(&ws, body?.0.title).await?;
    Ok((StatusCode::CREATED, Json(task.into())))
}

async fn complete_task(
    caller: Caller,
    p: Result<Path<(String, u32)>, PathRejection>,
) -> Reply<TaskDto> {
    let Path((ws, id)) = p?;
    Ok(Json(caller.0.complete(&WorkspaceId(ws), id).await?.into()))
}

async fn reopen_task(
    caller: Caller,
    p: Result<Path<(String, u32)>, PathRejection>,
) -> Reply<TaskDto> {
    let Path((ws, id)) = p?;
    Ok(Json(caller.0.reopen(&WorkspaceId(ws), id).await?.into()))
}

async fn list_members(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
) -> Reply<Vec<MemberDto>> {
    let ws = WorkspaceId(ws?.0);
    Ok(Json(caller.0.members(&ws).await?.into_iter().map(Into::into).collect()))
}

async fn set_member(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
    body: Result<Json<SetMemberBody>, JsonRejection>,
) -> Reply<MemberDto> {
    let ws = WorkspaceId(ws?.0);
    let body = body?.0;
    caller.0.set_member(&ws, body.email.clone(), body.role.into()).await?;
    Ok(Json(MemberDto { email: body.email.trim().to_lowercase(), role: body.role }))
}

async fn remove_member(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
    body: Result<Json<RemoveMemberBody>, JsonRejection>,
) -> Reply<serde_json::Value> {
    let ws = WorkspaceId(ws?.0);
    caller.0.remove_member(&ws, body?.0.email).await?;
    Ok(Json(serde_json::json!({})))
}
