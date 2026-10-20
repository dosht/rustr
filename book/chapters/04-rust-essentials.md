# Rust essentials for this architecture

This is not a full Rust course. It covers the parts of the language this architecture leans on, using taskr's own code. If you've written TypeScript, Kotlin or Swift you'll recognize most ideas. The new one is ownership.

## Cargo, crates and modules

A **crate** is a unit of compilation: a library or a binary. A **workspace** is a set of crates built together with one lock file. taskr's root `Cargo.toml` is a workspace:

```{.toml file="Cargo.toml"}
[workspace]
resolver = "3"
members = ["crates/*"]
```

Shared dependency versions go in `[workspace.dependencies]`, and each crate opts in with `serde.workspace = true`. This keeps versions in one place.

Inside a crate, code is split into **modules**. `mod tasks;` in `lib.rs` loads `tasks/mod.rs` or `tasks.rs`. Everything is **private by default**:

- no keyword: visible in this module and its children
- `pub(crate)`: visible anywhere in this crate
- `pub`: visible to other crates

That default is what makes rich domain models work. `Task`'s fields have no `pub`, so code outside `domain.rs` can read them only through methods, and change them only through `TaskList`.

### Features and conditional compilation

**Features** are compile-time switches. `taskr-infra` has a `native` feature that pulls in filesystem code, and the web build turns it off to reuse only the JSON format:

```{.toml file="crates/taskr-infra/Cargo.toml"}
[features]
default = ["native"]
native = ["dep:dirs"]
```

`#[cfg(...)]` includes code only under a condition. `taskr-core` exposes its test fakes to other crates only when they ask for them:

```{.rust file="crates/taskr-core/src/lib.rs"}
#[cfg(any(test, feature = "testing"))]
pub mod testing;
```

## Structs, enums and pattern matching

Structs are like records or data classes. Enums are where Rust shines: each variant can carry its own data. This is how taskr models a task's status:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
pub enum TaskStatus {
    Open,
    Done { at: Millis },
}
```

A done task *has* a completion time and an open task *can't* have one. You can't represent "open but completed at 3pm". Compare with the usual `status: string; doneAt?: number`, where nothing stops the two fields from disagreeing.

You take enums apart with `match`, and the compiler checks that every case is handled:

```rust
match t.status {
    TaskStatus::Open => None,
    TaskStatus::Done { at } => Some(at),
}
```

Add a third variant (`Archived`) and every `match` that doesn't handle it stops compiling. That's how the compiler walks you through every place a change matters.

## Option and Result instead of null and exceptions

Rust has no `null` and no exceptions. Instead:

- `Option<T>` is `Some(value)` or `None`.
- `Result<T, E>` is `Ok(value)` or `Err(error)`.

The `?` operator returns early on `None` or `Err`, which keeps the happy path readable. Here is the `Title` value object. It can only be built through `parse`, so every `Title` that exists is valid:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
impl Title {
    pub fn parse(raw: &str) -> Result<Title, TitleError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TitleError::Empty);
        }
        if trimmed.chars().count() > MAX_TITLE_CHARS {
            return Err(TitleError::TooLong { max: MAX_TITLE_CHARS });
        }
        if trimmed.chars().any(char::is_control) {
            return Err(TitleError::ControlCharacter);
        }
        Ok(Title(trimmed.to_string()))
    }
```

Note `chars().count()` rather than `len()`. `len()` counts bytes, and "é" is two bytes in UTF-8. The rule is about characters the user sees.

### Converting errors with From

`?` also converts errors if there's a `From` implementation. The application layer wraps domain errors and storage errors in one type:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
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
```

So inside a function that returns `Result<_, TasksError>`, `list.add(title, now)?` turns a `TaskError` into `TasksError::Rejected` automatically. `Access` is for the signed-in mode: who you are and what your role allows (chapter 13).

## Ownership and borrowing

This is the idea that's new. Every value has exactly one **owner**. When the owner goes away, the value is freed. You can lend a value out:

- `&T`: a shared borrow. Many readers at once, nobody writes.
- `&mut T`: an exclusive borrow. One writer, nobody else even reads.

The compiler checks these rules at compile time, which is why Rust needs no garbage collector and has no data races.

You can read the rules straight off method signatures:

```rust
pub fn get(&self, id: TaskId) -> Option<&Task>          // reads, lends out a task
pub fn add(&mut self, title: Title, now: Millis) -> ... // changes the list, takes the title
```

`add` takes `title: Title` by value: the title **moves** into the new task, and the caller can't use it afterwards. No copy, no aliasing.

### A borrow checker story

`reopen` is a good example of the borrow checker shaping code:

```{.rust file="crates/taskr-core/src/tasks/domain.rs"}
    pub fn reopen(&mut self, id: TaskId) -> Result<(), TaskError> {
        let task = self.get(id).ok_or(TaskError::NotFound(id))?;
        if task.is_open() {
            return Err(TaskError::AlreadyOpen(id));
        }
        let title = task.title.clone();
        self.ensure_room_for_one_more_open()?;
        self.ensure_no_open_task_titled(&title)?;
        self.task_mut(id)?.status = TaskStatus::Open;
        Ok(())
    }
```

`task` is a shared borrow into `self`. To check uniqueness we need to look at *all* tasks, and then to change one we need an exclusive borrow. We can't hold `task` across those steps, so we `clone()` the title (a short string) and let the borrow end. Then `task_mut` takes a fresh exclusive borrow for the actual change. The compiler forces the order "check everything, then mutate", which happens to be the right order for an invariant check anyway.

::: tip
When the borrow checker complains, the fix is usually one of: clone something small, shorten a borrow by finishing with it sooner, or split a method so reading and writing happen in separate steps. Reaching for `Rc<RefCell<...>>` in domain code is almost always a sign the design is fighting you.
:::

## Traits: interfaces, and more

A **trait** is an interface. Ports are traits:

```{.rust file="crates/taskr-core/src/shared/telemetry.rs"}
pub trait Telemetry: Send + Sync {
    fn record(&self, event: LogEvent);
}
```

`Send + Sync` are **marker traits**: `Send` means the value can move to another thread, `Sync` means it can be shared between threads. Requiring them on ports means every adapter must be thread-safe, which matters on mobile where calls come from background threads.

### Generics or trait objects?

There are two ways to accept "anything that implements a trait":

```rust
struct Tasks<R: TaskListRepository> { repo: R }   // generic: one compiled copy per R
struct Tasks { repo: Arc<dyn TaskListRepository> } // trait object: one copy, dynamic dispatch
```

Generics are zero-cost and great inside a library. taskr uses **trait objects** (`Arc<dyn Trait>`) for ports, for two practical reasons: UniFFI and wasm-bindgen can't export generic types, and composition roots choose adapters at runtime (the TUI falls back to `NoTelemetry` if the log file can't be opened). A virtual call next to a disk write costs nothing you could measure.

`Arc` is an atomically reference-counted pointer: several owners share one value, and it's freed when the last one goes. `Arc<dyn Telemetry>` is "a shared, thread-safe handle to some telemetry adapter".

### Closures and impl Trait

The use cases share one "unit of work" helper that takes the domain operation as a closure:

```rust
fn change(
    &self,
    name: &'static str,
    apply: impl FnOnce(&mut TaskList) -> Result<TaskId, TaskError>,
) -> Result<TaskSummary, TasksError>
```

`impl FnOnce(...)` means "some closure you can call once with a mutable task list". Each use case passes a different closure, and the helper handles load, save, errors and telemetry in one place.

## Iterators

Iterator chains replace most loops, and they compile down to the same machine code. The presentation layer builds the visible rows like this:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
        let rows: Vec<TaskRow> = self
            .tasks
            .iter()
            .filter(|t| self.scope.includes(t.is_done()))
            .filter(|t| needle.is_empty() || t.title.to_lowercase().contains(&needle))
            .map(|t| TaskRow {
                id: t.id,
                title: t.title.clone(),
                done: t.is_done(),
                cells: vec![t.id.to_string(), t.title.clone(), age_label(t.created_at, now)],
            })
            .collect();
```

`iter()` borrows, `filter` and `map` are lazy, and `collect()` builds the `Vec`. Nothing runs until `collect`.

## Derive, Debug and friends

`#[derive(...)]` generates common trait implementations:

- `Debug`: printable with `{:?}`, for tests and logs
- `Clone`: explicit `.clone()`; `Copy`: implicit copying for small values like `TaskId`
- `PartialEq`, `Eq`: `==`, which tests use with `assert_eq!`
- `Default`: an empty starting value

They're opt-in on purpose. A type that is expensive to copy simply doesn't derive `Copy`.

## Constant tables

Rust lets you build real data at compile time. The TUI's key bindings are a `const` array of structs, and the help screen is generated from it (chapter 8). The column spec shared by all shells is the same idea:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub const COLUMNS: [Column; 3] = [
    Column { title: "ID", align: Align::Right, wide: true },
    Column { title: "TITLE", align: Align::Left, wide: false },
    Column { title: "AGE", align: Align::Right, wide: false },
];
```

## Tests live next to the code

Unit tests go in a `tests` module at the bottom of the file, compiled only for `cargo test`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_titles_are_unique_ignoring_case() {
        let mut list = TaskList::new();
        list.add(title("Buy milk"), 0).unwrap();
        assert_eq!(list.add(title("buy MILK"), 0), Err(TaskError::DuplicateOpenTitle));
    }
}
```

Because the tests are inside the module, they can see private items. Integration tests in a crate's `tests/` folder see only the public API, like any other user would. taskr's CLI tests live there and run the real binary.

## Edition 2024 niceties

taskr uses the 2024 edition, which allows chaining `let` patterns with `&&`:

```{.rust file="crates/taskr-tui/src/main.rs"}
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(key) = translate(key)
        {
            shell.handle(key);
        }
```

Before 2024 this was four nested `if`s.

::: exercise
1. Add an `Archived` variant to `TaskStatus` and run `cargo build`. Read every error: each one is a place your change matters.
2. Change `Title::parse` to use `len()` instead of `chars().count()`, then run `cargo test -p taskr-core`. Which test catches it, and why?
3. Try to write `reopen` without the `clone()`. Read the borrow checker's message carefully: it tells you exactly which two borrows overlap.
:::
