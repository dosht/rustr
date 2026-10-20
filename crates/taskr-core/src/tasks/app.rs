//! Use cases for the tasks context, and the ports they need.
//!
//! Every write is one unit of work: load the aggregate, apply one domain
//! operation, save it. Read models (`TaskSummary`) leave this layer; domain
//! entities never do.

use std::sync::Arc;

use crate::shared::access::{AccessError, Action, Actor, WorkspaceId};
use crate::shared::clock::{Clock, Millis};
use crate::shared::events::DomainEvent;
use crate::shared::future::BoxFuture;
pub use crate::shared::storage::StorageError;
use crate::shared::storage::{MAX_ATTEMPTS, Revision, SaveError};
use crate::shared::telemetry::{LogEvent, Telemetry};
use crate::tasks::domain::{TaskError, TaskId, TaskList, TaskStatus, Title};

/// One task list per workspace. `save` must write the list and the events in
/// one atomic step, and fail with `SaveError::Conflict` when the stored
/// revision is not `expected`. Adapters without an outbox may drop the events.
pub trait TaskListRepository: Send + Sync {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>>;

    fn save<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        list: &'a TaskList,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TasksError {
    Rejected(TaskError),
    Access(AccessError),
    Storage(StorageError),
}

impl From<TaskError> for TasksError {
    fn from(e: TaskError) -> Self {
        TasksError::Rejected(e)
    }
}

impl From<StorageError> for TasksError {
    fn from(e: StorageError) -> Self {
        TasksError::Storage(e)
    }
}

impl From<AccessError> for TasksError {
    fn from(e: AccessError) -> Self {
        TasksError::Access(e)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Open,
    Done,
    All,
}

impl Scope {
    pub fn includes(self, done: bool) -> bool {
        match self {
            Scope::Open => !done,
            Scope::Done => done,
            Scope::All => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSummary {
    pub id: u32,
    pub title: String,
    pub created_at: Millis,
    pub done_at: Option<Millis>,
}

impl TaskSummary {
    pub fn is_done(&self) -> bool {
        self.done_at.is_some()
    }
}

/// The tasks API as a client sees it: one workspace at a time, the caller
/// already known. Implemented in process (`LocalTasks`, the server's
/// `InProcessApi`) and over HTTP (`taskr-client`), so the presentation layer
/// cannot tell a local list from a remote one.
pub trait TasksApi: Send + Sync {
    /// Every task in the workspace, ordered like `Tasks::list`.
    fn list<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>>;

    fn add<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        title: String,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>>;

    fn complete<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>>;

    fn reopen<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>>;
}

#[derive(Clone)]
pub struct Tasks {
    repo: Arc<dyn TaskListRepository>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<dyn Telemetry>,
}

impl Tasks {
    pub fn new(
        repo: Arc<dyn TaskListRepository>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<dyn Telemetry>,
    ) -> Self {
        Self { repo, clock, telemetry }
    }

    pub fn now(&self) -> Millis {
        self.clock.now()
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// Open tasks oldest first, then done tasks most recently finished first.
    pub async fn list(&self, actor: &Actor, scope: Scope) -> Result<Vec<TaskSummary>, TasksError> {
        actor.ensure(Action::ViewTasks)?;
        let (list, _) = self
            .repo
            .load(&actor.workspace)
            .await
            .inspect_err(|e| self.storage_failed("list", e))?;
        let mut rows: Vec<TaskSummary> =
            list.tasks().iter().map(summarize).filter(|t| scope.includes(t.is_done())).collect();
        rows.sort_by_key(|t| match t.done_at {
            None => (0, t.id as i64),
            Some(at) => (1, -at),
        });
        Ok(rows)
    }

    pub async fn add(&self, actor: &Actor, raw_title: &str) -> Result<TaskSummary, TasksError> {
        let now = self.clock.now();
        self.change(actor, "task.added", |list| {
            let title = Title::parse(raw_title)?;
            list.add(title, now)
        })
        .await
    }

    pub async fn complete(&self, actor: &Actor, id: u32) -> Result<TaskSummary, TasksError> {
        let now = self.clock.now();
        self.change(actor, "task.completed", |list| {
            list.complete(TaskId(id), now).map(|_| TaskId(id))
        })
        .await
    }

    pub async fn reopen(&self, actor: &Actor, id: u32) -> Result<TaskSummary, TasksError> {
        self.change(actor, "task.reopened", |list| list.reopen(TaskId(id)).map(|_| TaskId(id)))
            .await
    }

    async fn change(
        &self,
        actor: &Actor,
        name: &'static str,
        apply: impl Fn(&mut TaskList) -> Result<TaskId, TaskError> + Send,
    ) -> Result<TaskSummary, TasksError> {
        actor.ensure(Action::ChangeTasks).inspect_err(|e| self.rejected(name, e))?;
        for attempt in 1..=MAX_ATTEMPTS {
            let (mut list, revision) = self
                .repo
                .load(&actor.workspace)
                .await
                .inspect_err(|e| self.storage_failed(name, e))?;
            let id = apply(&mut list).inspect_err(|e| self.rejected(name, e))?;
            let task = list.get(id).map(summarize).expect("the task we just changed exists");
            let event = DomainEvent {
                name,
                workspace: actor.workspace.clone(),
                actor: actor.principal.user_id.clone(),
                at: self.clock.now(),
                fields: vec![],
            }
            .field("id", id.0)
            .field("title", &task.title);
            match self.repo.save(&actor.workspace, &list, revision, &[event]).await {
                Ok(()) => {
                    self.telemetry.record(
                        LogEvent::info(name)
                            .field("id", id.0)
                            .field("open", list.open_count())
                            .field("workspace", &actor.workspace),
                    );
                    return Ok(task);
                }
                Err(SaveError::Conflict) => self.telemetry.record(
                    LogEvent::warn("storage.conflict")
                        .field("action", name)
                        .field("attempt", attempt),
                ),
                Err(SaveError::Failed(e)) => {
                    self.storage_failed(name, &e);
                    return Err(e.into());
                }
            }
        }
        Err(TasksError::Storage(StorageError("too many people are editing at once".into())))
    }

    fn rejected(&self, action: &'static str, reason: &impl std::fmt::Debug) {
        self.telemetry.record(
            LogEvent::warn("task.rejected")
                .field("action", action)
                .field("reason", format!("{reason:?}")),
        );
    }

    fn storage_failed(&self, action: &'static str, e: &StorageError) {
        self.telemetry
            .record(LogEvent::error("storage.failed").field("action", action).field("error", &e.0));
    }
}

/// `TasksApi` for the signed-out shells: one actor, the owner of every workspace it opens.
pub struct LocalTasks {
    tasks: Tasks,
    actor: Actor,
}

impl LocalTasks {
    pub fn new(tasks: Tasks) -> Self {
        Self { tasks, actor: Actor::local_owner() }
    }

    fn actor(&self, workspace: &WorkspaceId) -> Actor {
        Actor { workspace: workspace.clone(), ..self.actor.clone() }
    }
}

impl TasksApi for LocalTasks {
    fn list<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>> {
        Box::pin(async move { self.tasks.list(&self.actor(workspace), Scope::All).await })
    }

    fn add<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        title: String,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.add(&self.actor(workspace), &title).await })
    }

    fn complete<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.complete(&self.actor(workspace), id).await })
    }

    fn reopen<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        id: u32,
    ) -> BoxFuture<'a, Result<TaskSummary, TasksError>> {
        Box::pin(async move { self.tasks.reopen(&self.actor(workspace), id).await })
    }
}

fn summarize(task: &crate::tasks::domain::Task) -> TaskSummary {
    TaskSummary {
        id: task.id().0,
        title: task.title().as_str().to_string(),
        created_at: task.created_at(),
        done_at: match task.status() {
            TaskStatus::Open => None,
            TaskStatus::Done { at } => Some(at),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::access::Role;
    use crate::shared::telemetry::Level;
    use crate::tasks::domain::TitleError;
    use crate::testing::{FixedClock, InMemoryTaskListRepository, RecordingTelemetry, block_on};

    struct World {
        tasks: Tasks,
        repo: Arc<InMemoryTaskListRepository>,
        clock: Arc<FixedClock>,
        telemetry: Arc<RecordingTelemetry>,
        owner: Actor,
    }

    fn world() -> World {
        let repo = Arc::new(InMemoryTaskListRepository::default());
        let clock = Arc::new(FixedClock::at(1_000));
        let telemetry = Arc::new(RecordingTelemetry::default());
        let tasks = Tasks::new(repo.clone(), clock.clone(), telemetry.clone());
        World { tasks, repo, clock, telemetry, owner: Actor::local_owner() }
    }

    fn as_role(actor: &Actor, role: Role) -> Actor {
        Actor { role, ..actor.clone() }
    }

    #[test]
    fn add_persists_and_returns_a_summary() {
        let w = world();
        let added = block_on(w.tasks.add(&w.owner, "  Buy milk ")).unwrap();
        assert_eq!(
            added,
            TaskSummary { id: 1, title: "Buy milk".into(), created_at: 1_000, done_at: None }
        );
        assert_eq!(block_on(w.tasks.list(&w.owner, Scope::All)).unwrap(), vec![added]);
    }

    #[test]
    fn rejected_add_changes_nothing_and_is_recorded() {
        let w = world();
        let err = block_on(w.tasks.add(&w.owner, "   ")).unwrap_err();
        assert_eq!(err, TasksError::Rejected(TaskError::InvalidTitle(TitleError::Empty)));
        assert_eq!(w.repo.save_count(), 0);
        let events = w.telemetry.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, Level::Warn);
        assert_eq!(events[0].get("action"), Some("task.added"));
    }

    #[test]
    fn complete_and_reopen_round_trip() {
        let w = world();
        block_on(w.tasks.add(&w.owner, "a")).unwrap();
        w.clock.set(2_000);
        assert_eq!(block_on(w.tasks.complete(&w.owner, 1)).unwrap().done_at, Some(2_000));
        assert_eq!(block_on(w.tasks.reopen(&w.owner, 1)).unwrap().done_at, None);
        let names: Vec<_> = w.telemetry.events().iter().map(|e| e.name).collect();
        assert_eq!(names, ["task.added", "task.completed", "task.reopened"]);
    }

    #[test]
    fn list_orders_open_first_then_most_recently_done() {
        let w = world();
        for t in ["a", "b", "c", "d"] {
            block_on(w.tasks.add(&w.owner, t)).unwrap();
        }
        w.clock.set(2_000);
        block_on(w.tasks.complete(&w.owner, 1)).unwrap();
        w.clock.set(3_000);
        block_on(w.tasks.complete(&w.owner, 3)).unwrap();
        let ids = |scope| {
            block_on(w.tasks.list(&w.owner, scope))
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Scope::All), [2, 4, 3, 1]);
        assert_eq!(ids(Scope::Open), [2, 4]);
        assert_eq!(ids(Scope::Done), [3, 1]);
    }

    #[test]
    fn storage_failure_surfaces_and_is_recorded() {
        let w = world();
        w.repo.fail_saves(true);
        assert!(matches!(block_on(w.tasks.add(&w.owner, "a")), Err(TasksError::Storage(_))));
        assert_eq!(w.telemetry.events().last().unwrap().name, "storage.failed");
        assert!(block_on(w.tasks.list(&w.owner, Scope::All)).unwrap().is_empty());
    }

    #[test]
    fn viewers_can_list_but_not_change() {
        let w = world();
        block_on(w.tasks.add(&w.owner, "a")).unwrap();
        let viewer = as_role(&w.owner, Role::Viewer);
        assert_eq!(block_on(w.tasks.list(&viewer, Scope::All)).unwrap().len(), 1);
        assert_eq!(
            block_on(w.tasks.add(&viewer, "b")),
            Err(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks)))
        );
        assert_eq!(
            block_on(w.tasks.complete(&viewer, 1)),
            Err(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks)))
        );
        assert_eq!(w.repo.save_count(), 1);
        assert!(block_on(w.tasks.add(&as_role(&w.owner, Role::Editor), "b")).is_ok());
    }

    #[test]
    fn workspaces_are_isolated() {
        let w = world();
        let other = Actor { workspace: WorkspaceId("other".into()), ..w.owner.clone() };
        block_on(w.tasks.add(&w.owner, "mine")).unwrap();
        assert!(block_on(w.tasks.list(&other, Scope::All)).unwrap().is_empty());
        assert_eq!(block_on(w.tasks.add(&other, "mine")).unwrap().id, 1);
    }

    #[test]
    fn a_change_writes_one_outbox_event_with_the_actor() {
        let w = world();
        block_on(w.tasks.add(&w.owner, "Buy milk")).unwrap();
        let events = w.repo.outbox();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "task.added");
        assert_eq!(events[0].actor, w.owner.principal.user_id);
        assert_eq!(events[0].workspace, WorkspaceId::local());
        assert_eq!(events[0].get("title"), Some("Buy milk"));
        assert_eq!(events[0].at, 1_000);
    }

    #[test]
    fn a_conflict_is_retried_on_fresh_data() {
        let w = world();
        block_on(w.tasks.add(&w.owner, "a")).unwrap();
        w.repo.conflict_next_saves(1);
        assert_eq!(block_on(w.tasks.add(&w.owner, "b")).unwrap().id, 2);
        assert_eq!(w.telemetry.events().iter().filter(|e| e.name == "storage.conflict").count(), 1);
    }

    #[test]
    fn endless_conflicts_give_up() {
        let w = world();
        w.repo.conflict_next_saves(MAX_ATTEMPTS);
        assert!(matches!(block_on(w.tasks.add(&w.owner, "a")), Err(TasksError::Storage(_))));
        assert!(block_on(w.tasks.list(&w.owner, Scope::All)).unwrap().is_empty());
    }
}
