//! Client-side adapters over HTTP. The same code runs in the CLI (tokio) and
//! in the browser (wasm32, where reqwest uses `fetch`).

pub mod auth;
pub mod audio_summary;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;

use std::sync::Arc;

use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use taskr_api::errors::{ErrorBody, WireError, to_tasks_error, to_workspaces_error};
use taskr_api::{AddTaskBody, MeDto, MemberDto, RemoveMemberBody, SetMemberBody, TaskDto, paths};
use taskr_core::session::app::SessionStore;
use taskr_core::shared::access::{AccessError, Role, WorkspaceId};
use taskr_core::shared::future::BoxFuture;
use taskr_core::shared::storage::StorageError;
use taskr_core::tasks::app::{TaskSummary, TasksApi, TasksError};
use taskr_core::workspaces::app::{Me, MemberSummary, WorkspacesApi, WorkspacesError};

pub use auth::{DevAuth, SupabaseAuth};

enum Failure {
    NoSession,
    Server(u16, WireError),
    Network(String),
}

impl From<Failure> for TasksError {
    fn from(f: Failure) -> Self {
        match f {
            Failure::NoSession => TasksError::Access(AccessError::Unauthenticated),
            Failure::Server(status, body) => to_tasks_error(status, &body),
            Failure::Network(why) => TasksError::Storage(StorageError(why)),
        }
    }
}

impl From<Failure> for WorkspacesError {
    fn from(f: Failure) -> Self {
        match f {
            Failure::NoSession => WorkspacesError::Access(AccessError::Unauthenticated),
            Failure::Server(status, body) => to_workspaces_error(status, &body),
            Failure::Network(why) => WorkspacesError::Storage(StorageError(why)),
        }
    }
}

/// `TasksApi` and `WorkspacesApi` against a taskr-server. The bearer token is
/// read from the session store on every call, so signing in or out needs no
/// extra wiring.
pub struct HttpApi {
    http: reqwest::Client,
    base: String,
    sessions: Arc<dyn SessionStore>,
}

impl HttpApi {
    /// `base` is the API root, for example `http://localhost:8787/api`.
    pub fn new(base: impl Into<String>, sessions: Arc<dyn SessionStore>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
            sessions,
        }
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, Failure> {
        let session = self.sessions.load().ok_or(Failure::NoSession)?;
        Ok(self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(session.access_token))
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + Sync)>,
    ) -> Result<T, Failure> {
        let mut req = self.request(method, path)?;
        if let Some(body) = body {
            req = req.json(body);
        }
        let res = req.send().await.map_err(|e| Failure::Network(unreachable(&e)))?;
        let status = res.status();
        if status.is_success() {
            return res.json().await.map_err(|e| Failure::Network(format!("bad response: {e}")));
        }
        let text = res.text().await.unwrap_or_default();
        match serde_json::from_str::<ErrorBody>(&text) {
            Ok(body) => Err(Failure::Server(status.as_u16(), body.error)),
            Err(_) if status == StatusCode::UNAUTHORIZED => Err(Failure::Server(
                401,
                WireError { code: "unauthenticated".into(), message: text, detail: None },
            )),
            Err(_) => Err(Failure::Network(format!("server answered {status}: {text}"))),
        }
    }

    async fn task(
        &self,
        path: String,
        body: Option<&AddTaskBody>,
    ) -> Result<TaskSummary, TasksError> {
        Ok(self.call::<TaskDto>(Method::POST, &path, body).await?.into())
    }
}

fn unreachable(e: &reqwest::Error) -> String {
    format!("could not reach the server: {e}")
}

const NO_BODY: Option<&()> = None;

impl TasksApi for HttpApi {
    fn list<'a>(
        &'a self,
        ws: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>> {
        Box::pin(async move {
            let rows: Vec<TaskDto> = self.call(Method::GET, &paths::tasks(ws), NO_BODY).await?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
    }

    fn add<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        title: String,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.task(paths::tasks(ws), Some(&AddTaskBody { title })).await })
    }

    fn complete<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.task(paths::complete(ws, id), None).await })
    }

    fn reopen<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.task(paths::reopen(ws, id), None).await })
    }
}

impl WorkspacesApi for HttpApi {
    fn me(&self) -> BoxFuture<'_, Result<Me, WorkspacesError>> {
        Box::pin(
            async move { Ok(self.call::<MeDto>(Method::GET, paths::ME, NO_BODY).await?.into()) },
        )
    }

    fn members<'a>(
        &'a self,
        ws: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<MemberSummary>, WorkspacesError>> {
        Box::pin(async move {
            let rows: Vec<MemberDto> = self.call(Method::GET, &paths::members(ws), NO_BODY).await?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
    }

    fn set_member<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        email: String,
        role: Role,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>> {
        Box::pin(async move {
            let body = SetMemberBody { email, role: role.into() };
            let _: MemberDto = self.call(Method::PUT, &paths::members(ws), Some(&body)).await?;
            Ok(())
        })
    }

    fn remove_member<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        email: String,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>> {
        Box::pin(async move {
            let body = RemoveMemberBody { email };
            let _: serde_json::Value =
                self.call(Method::DELETE, &paths::members(ws), Some(&body)).await?;
            Ok(())
        })
    }
}
