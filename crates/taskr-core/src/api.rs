//! `TasksApi` and `WorkspacesApi` run in process for one signed-in principal:
//! resolve the actor in the workspace, then call the use case. The server
//! builds one per request; tests use it to drive the app without HTTP.

use crate::shared::access::{Actor, Principal, Role, WorkspaceId};
use crate::shared::future::BoxFuture;
use crate::tasks::app::{Scope, TaskSummary, Tasks, TasksApi, TasksError};
use crate::workspaces::app::{Me, MemberSummary, Workspaces, WorkspacesApi, WorkspacesError};

pub struct InProcessApi {
    tasks: Tasks,
    workspaces: Workspaces,
    principal: Principal,
}

impl InProcessApi {
    pub fn new(tasks: Tasks, workspaces: Workspaces, principal: Principal) -> Self {
        Self { tasks, workspaces, principal }
    }

    async fn actor(&self, ws: &WorkspaceId) -> Result<Actor, WorkspacesError> {
        self.workspaces.actor(&self.principal, ws).await
    }

    async fn task_actor(&self, ws: &WorkspaceId) -> Result<Actor, TasksError> {
        self.actor(ws).await.map_err(|e| match e {
            WorkspacesError::Access(a) => TasksError::Access(a),
            WorkspacesError::Storage(s) => TasksError::Storage(s),
            WorkspacesError::Rejected(r) => {
                unreachable!("resolving an actor never applies a workspace rule: {r:?}")
            }
        })
    }
}

impl TasksApi for InProcessApi {
    fn list<'a>(
        &'a self,
        ws: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>> {
        Box::pin(async move { self.tasks.list(&self.task_actor(ws).await?, Scope::All).await })
    }

    fn add<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        title: String,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.add(&self.task_actor(ws).await?, &title).await })
    }

    fn complete<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.complete(&self.task_actor(ws).await?, id).await })
    }

    fn reopen<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.reopen(&self.task_actor(ws).await?, id).await })
    }
}

impl WorkspacesApi for InProcessApi {
    fn me(&self) -> BoxFuture<'_, Result<Me, WorkspacesError>> {
        Box::pin(self.workspaces.me(&self.principal))
    }

    fn members<'a>(
        &'a self,
        ws: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<MemberSummary>, WorkspacesError>> {
        Box::pin(async move { self.workspaces.members(&self.actor(ws).await?).await })
    }

    fn set_member<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        email: String,
        role: Role,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>> {
        Box::pin(
            async move { self.workspaces.set_member(&self.actor(ws).await?, &email, role).await },
        )
    }

    fn remove_member<'a>(
        &'a self,
        ws: &'a WorkspaceId,
        email: String,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>> {
        Box::pin(async move { self.workspaces.remove_member(&self.actor(ws).await?, &email).await })
    }
}
