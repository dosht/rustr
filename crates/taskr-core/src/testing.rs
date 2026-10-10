//! Hand-written fakes for every port, plus contract tests that real adapters
//! must also pass. The contract is what keeps the fakes honest.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

use crate::shared::clock::{Clock, Millis};
use crate::shared::telemetry::{LogEvent, Telemetry};
use crate::tasks::app::{StorageError, TaskListRepository};
use crate::tasks::domain::{TaskList, TaskListSnapshot, Title};

pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn at(now: Millis) -> Self {
        Self(AtomicI64::new(now))
    }

    pub fn set(&self, now: Millis) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}

/// Stores a snapshot rather than the aggregate so every load goes through
/// `TaskList::restore`, exactly like the real adapters.
#[derive(Default)]
pub struct InMemoryTaskListRepository {
    stored: Mutex<TaskListSnapshot>,
    fail_saves: AtomicBool,
    saves: AtomicUsize,
}

impl InMemoryTaskListRepository {
    pub fn fail_saves(&self, fail: bool) {
        self.fail_saves.store(fail, Ordering::SeqCst);
    }

    pub fn save_count(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }
}

impl TaskListRepository for InMemoryTaskListRepository {
    fn load(&self) -> Result<TaskList, StorageError> {
        let snapshot = self.stored.lock().unwrap().clone();
        TaskList::restore(snapshot).map_err(|e| StorageError(e.0))
    }

    fn save(&self, list: &TaskList) -> Result<(), StorageError> {
        if self.fail_saves.load(Ordering::SeqCst) {
            return Err(StorageError("disk is full".into()));
        }
        *self.stored.lock().unwrap() = list.snapshot();
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingTelemetry(Mutex<Vec<LogEvent>>);

impl RecordingTelemetry {
    pub fn events(&self) -> Vec<LogEvent> {
        self.0.lock().unwrap().clone()
    }
}

impl Telemetry for RecordingTelemetry {
    fn record(&self, event: LogEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// Run against every `TaskListRepository` implementation. `fresh` must return
/// a repository with no stored data; `reopen` must return a second repository
/// reading the same storage, to prove data survives a restart.
pub fn task_list_repository_contract<R: TaskListRepository>(
    fresh: impl Fn() -> R,
    reopen: impl Fn(&R) -> R,
) {
    let repo = fresh();
    assert_eq!(repo.load().expect("empty storage loads"), TaskList::new());

    let mut list = TaskList::new();
    let a = list.add(Title::parse("first").unwrap(), 10).unwrap();
    list.add(Title::parse("second ünïcode").unwrap(), 20).unwrap();
    list.complete(a, 30).unwrap();
    repo.save(&list).expect("save succeeds");
    assert_eq!(repo.load().unwrap(), list);

    let again = reopen(&repo);
    assert_eq!(again.load().unwrap(), list, "data survives reopening the storage");

    list.add(Title::parse("third").unwrap(), 40).unwrap();
    again.save(&list).unwrap();
    assert_eq!(again.load().unwrap(), list, "a later save replaces the earlier one");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn in_memory_repository_honours_the_contract() {
        task_list_repository_contract(
            || Arc::new(InMemoryTaskListRepository::default()),
            |r| r.clone(),
        );
    }

    impl TaskListRepository for Arc<InMemoryTaskListRepository> {
        fn load(&self) -> Result<TaskList, StorageError> {
            self.as_ref().load()
        }
        fn save(&self, list: &TaskList) -> Result<(), StorageError> {
            self.as_ref().save(list)
        }
    }
}
