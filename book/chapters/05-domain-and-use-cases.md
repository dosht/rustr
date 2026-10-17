# The domain and the use cases

This chapter walks through `domain.rs` and `app.rs`. Together they're about 300 lines of code plus tests, and they hold every business rule taskr has.

## Finding the aggregate

The first design question in any domain is: **what has to be consistent together?**

taskr has two rules that span tasks:

1. No two **open** tasks share a title, ignoring case.
2. At most 50 tasks are open at once.

You can't check either rule by looking at one task. So the consistency boundary, the **aggregate**, is the whole list. `TaskList` is the root, and every change goes through it:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
/// The aggregate root. The whole list is one consistency boundary because the
/// invariants ("no two open tasks share a title", "at most N open tasks") span tasks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskList {
    tasks: Vec<Task>,
    next_id: u32,
}
```

Both fields are private. The only way to add, complete or reopen a task is through `TaskList`'s methods, so the invariants can't be bypassed.

::: note
"The whole list is the aggregate" is right for a personal task list with dozens of items. It wouldn't be right for a team product with millions of tasks, where loading everything to add one is too slow. Chapter 11 shows how the design changes when the aggregate gets too big: you make `Task` the aggregate and enforce uniqueness with a database index plus a domain check. The trick is that only `domain.rs` and the repository change.
:::

## Operations that guard invariants

Here is `add`:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
    pub fn add(&mut self, title: Title, now: Millis) -> Result<TaskId, TaskError> {
        self.ensure_room_for_one_more_open()?;
        self.ensure_no_open_task_titled(&title)?;
        let id = TaskId(self.next_id);
        self.next_id += 1;
        self.tasks.push(Task { id, title, created_at: now, status: TaskStatus::Open });
        Ok(id)
    }
```

A few deliberate choices:

- It takes a `Title`, not a `String`. Title validation already happened, and the type proves it.
- It takes `now` as a parameter. The domain never reads the clock itself, which keeps it deterministic. The use case asks the `Clock` port.
- The checks are named after the rule (`ensure_no_open_task_titled`), so the method reads like the spec.
- It returns the new id, and the caller decides what to do with it.

`complete` and `reopen` follow the same shape: check, then change. `reopen` is the subtle one, because reopening a done task can break the uniqueness rule if someone has since created an open task with the same title. The tests capture exactly that case:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
    #[test]
    fn reopen_respects_the_unique_open_title_rule() {
        let mut list = TaskList::new();
        let first = list.add(title("Call mom"), 0).unwrap();
        list.complete(first, 1).unwrap();
        list.add(title("call mom"), 2).unwrap();
        assert_eq!(list.reopen(first), Err(TaskError::DuplicateOpenTitle));
    }
```

## Errors are part of the domain

Domain errors are an enum, one variant per way a rule can say no:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
pub enum TaskError {
    InvalidTitle(TitleError),
    DuplicateOpenTitle,
    TooManyOpenTasks { max: usize },
    NotFound(TaskId),
    AlreadyDone(TaskId),
    AlreadyOpen(TaskId),
}
```

They carry data (which id, what limit) but **no user-facing text**. Turning errors into words is a presentation decision, made once in chapter 6. That's how all five shells end up saying the same thing.

## Persistence without serde

The domain must not depend on a serialization library, but adapters need to store it. The bridge is a **snapshot**: a plain struct with public fields, no behavior.

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
pub struct TaskListSnapshot {
    pub next_id: u32,
    pub tasks: Vec<TaskSnapshot>,
}
```

`TaskList::snapshot()` produces one, and `TaskList::restore(snapshot)` rebuilds the aggregate. The important part is that `restore` **re-checks every invariant**:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
    /// Rebuilds the aggregate from stored data, re-checking every invariant so a
    /// hand-edited or corrupted file can never produce an invalid list.
    pub fn restore(snapshot: TaskListSnapshot) -> Result<TaskList, RestoreError> {
```

A JSON file is just text. Somebody will edit it by hand one day, or a half-migrated version will write something odd. Re-validating on load means the rest of the system can trust every `TaskList` it ever sees. It also never reuses an id, even if the stored `next_id` is wrong.

The adapter in `taskr-infra` maps the snapshot to its own versioned JSON structs with serde. The format lives in the adapter, so changing it never touches the domain.

## Use cases: the application layer

`app.rs` defines what the outside world can ask for: list, add, complete, reopen. It also defines the port it needs:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
pub trait TaskListRepository: Send + Sync {
    fn load(&self) -> Result<TaskList, StorageError>;
    fn save(&self, list: &TaskList) -> Result<(), StorageError>;
}
```

`Tasks` is the use case object. It's built from three ports, and it has no other state:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
pub struct Tasks {
    repo: Arc<dyn TaskListRepository>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<dyn Telemetry>,
}
```

### One unit of work

Every write follows the same steps: load the aggregate, apply one domain operation, save, record what happened. That's written once:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
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
```

and each use case is a one-liner on top of it:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
    pub fn add(&self, raw_title: &str) -> Result<TaskSummary, TasksError> {
        let now = self.clock.now();
        self.change("task.added", |list| {
            let title = Title::parse(raw_title)?;
            list.add(title, now)
        })
    }
```

If the domain says no, nothing is saved. If saving fails, the caller gets a `Storage` error and telemetry records it. There's no partial state.

### Read models, not entities

Use cases return `TaskSummary`, a plain read model, never a `Task`:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
pub struct TaskSummary {
    pub id: u32,
    pub title: String,
    pub created_at: Millis,
    pub done_at: Option<Millis>,
}
```

This keeps the domain's types from leaking outward. If `Task` gains a private field or changes how it stores status, nothing above the application layer notices. It's the same reason the mobile and web facades have their own record types (chapters 9 and 10).

### Telemetry as a cross-cutting port

The use cases record `task.added`, `task.completed`, `task.reopened`, `task.rejected` with the reason, and `storage.failed`. They don't know or care where those go. A `LogEvent` is plain data with a level, a name and fields:

```rust
LogEvent::info("task.added").field("id", 3).field("open", 2)
```

The same events end up as JSON lines in a terminal, `console.info` in a browser, logcat on Android and os_log on iOS. In tests they go into a `RecordingTelemetry`, and tests assert on them like any other output (chapter 7).

## Listing and ordering

Ordering is a product decision ("open tasks oldest first, then done tasks most recently finished first"), and it lives in the use case so every caller gets the same order:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
        rows.sort_by_key(|t| match t.done_at {
            None => (0, t.id as i64),
            Some(at) => (1, -at),
        });
```

Sorting by a tuple compares the first element, then the second: open (0) before done (1), then by id or by negative completion time.

::: exercise
Add a **rename** operation, test first:

1. In `domain.rs`, write a failing test: renaming an open task to the title of another open task returns `DuplicateOpenTitle`.
2. Add `TaskList::rename(&mut self, id, title: Title)`. Which existing helpers can you reuse?
3. Add `Tasks::rename(id, raw_title)` using `change`. Write the use case test with the in-memory repository and check the telemetry event.
4. Don't touch any shell yet. Chapter 6 adds the screen behavior.
:::
