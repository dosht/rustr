//! Use cases for the tasks context, and the ports they need.
//!
//! Every write is one unit of work: load the aggregate, apply one domain
//! operation, save it. Read models (`TaskSummary`) leave this layer; domain
//! entities never do.

use std::sync::Arc;

use crate::shared::clock::{Clock, Millis};
use crate::shared::telemetry::{LogEvent, Telemetry};
use crate::tasks::domain::{TaskError, TaskId, TaskList, TaskStatus, Title};

pub trait TaskListRepository: Send + Sync {
    fn load(&self) -> Result<TaskList, StorageError>;
    fn save(&self, list: &TaskList) -> Result<(), StorageError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageError(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TasksError {
    Rejected(TaskError),
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

    /// Open tasks oldest first, then done tasks most recently finished first.
    pub fn list(&self, scope: Scope) -> Result<Vec<TaskSummary>, TasksError> {
        let list = self.repo.load().inspect_err(|e| self.storage_failed("list", e))?;
        let mut rows: Vec<TaskSummary> =
            list.tasks().iter().map(summarize).filter(|t| scope.includes(t.is_done())).collect();
        rows.sort_by_key(|t| match t.done_at {
            None => (0, t.id as i64),
            Some(at) => (1, -at),
        });
        Ok(rows)
    }

    pub fn add(&self, raw_title: &str) -> Result<TaskSummary, TasksError> {
        let now = self.clock.now();
        self.change("task.added", |list| {
            let title = Title::parse(raw_title)?;
            list.add(title, now)
        })
    }

    pub fn complete(&self, id: u32) -> Result<TaskSummary, TasksError> {
        let now = self.clock.now();
        self.change("task.completed", |list| list.complete(TaskId(id), now).map(|_| TaskId(id)))
    }

    pub fn reopen(&self, id: u32) -> Result<TaskSummary, TasksError> {
        self.change("task.reopened", |list| list.reopen(TaskId(id)).map(|_| TaskId(id)))
    }

    fn change(
        &self,
        name: &'static str,
        apply: impl FnOnce(&mut TaskList) -> Result<TaskId, TaskError>,
    ) -> Result<TaskSummary, TasksError> {
        let mut list = self.repo.load().inspect_err(|e| self.storage_failed(name, e))?;
        let id = apply(&mut list).inspect_err(|e| {
            self.telemetry.record(
                LogEvent::warn("task.rejected")
                    .field("action", name)
                    .field("reason", format!("{e:?}")),
            )
        })?;
        self.repo.save(&list).inspect_err(|e| self.storage_failed(name, e))?;
        let task = list.get(id).map(summarize).expect("the task we just changed exists");
        self.telemetry
            .record(LogEvent::info(name).field("id", id.0).field("open", list.open_count()));
        Ok(task)
    }

    fn storage_failed(&self, action: &'static str, e: &StorageError) {
        self.telemetry
            .record(LogEvent::error("storage.failed").field("action", action).field("error", &e.0));
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
    use crate::shared::telemetry::Level;
    use crate::tasks::domain::TitleError;
    use crate::testing::{FixedClock, InMemoryTaskListRepository, RecordingTelemetry};

    struct World {
        tasks: Tasks,
        repo: Arc<InMemoryTaskListRepository>,
        clock: Arc<FixedClock>,
        telemetry: Arc<RecordingTelemetry>,
    }

    fn world() -> World {
        let repo = Arc::new(InMemoryTaskListRepository::default());
        let clock = Arc::new(FixedClock::at(1_000));
        let telemetry = Arc::new(RecordingTelemetry::default());
        let tasks = Tasks::new(repo.clone(), clock.clone(), telemetry.clone());
        World { tasks, repo, clock, telemetry }
    }

    #[test]
    fn add_persists_and_returns_a_summary() {
        let w = world();
        let added = w.tasks.add("  Buy milk ").unwrap();
        assert_eq!(
            added,
            TaskSummary { id: 1, title: "Buy milk".into(), created_at: 1_000, done_at: None }
        );
        assert_eq!(w.tasks.list(Scope::All).unwrap(), vec![added]);
    }

    #[test]
    fn rejected_add_changes_nothing_and_is_recorded() {
        let w = world();
        let err = w.tasks.add("   ").unwrap_err();
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
        w.tasks.add("a").unwrap();
        w.clock.set(2_000);
        assert_eq!(w.tasks.complete(1).unwrap().done_at, Some(2_000));
        assert_eq!(w.tasks.reopen(1).unwrap().done_at, None);
        let names: Vec<_> = w.telemetry.events().iter().map(|e| e.name).collect();
        assert_eq!(names, ["task.added", "task.completed", "task.reopened"]);
    }

    #[test]
    fn list_orders_open_first_then_most_recently_done() {
        let w = world();
        for t in ["a", "b", "c", "d"] {
            w.tasks.add(t).unwrap();
        }
        w.clock.set(2_000);
        w.tasks.complete(1).unwrap();
        w.clock.set(3_000);
        w.tasks.complete(3).unwrap();
        let ids = |scope| w.tasks.list(scope).unwrap().iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(Scope::All), [2, 4, 3, 1]);
        assert_eq!(ids(Scope::Open), [2, 4]);
        assert_eq!(ids(Scope::Done), [3, 1]);
    }

    #[test]
    fn storage_failure_surfaces_and_is_recorded() {
        let w = world();
        w.repo.fail_saves(true);
        assert!(matches!(w.tasks.add("a"), Err(TasksError::Storage(_))));
        assert_eq!(w.telemetry.events().last().unwrap().name, "storage.failed");
        assert!(w.tasks.list(Scope::All).unwrap().is_empty());
    }
}
