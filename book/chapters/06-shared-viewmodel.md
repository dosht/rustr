# The shared ViewModel

Most multiplatform architectures stop at a shared domain and leave each platform to write its own ViewModels. This chapter is about the next step: sharing screen behavior too, without sharing UI.

## What's left after the domain

Look at the task list screen and list everything it does that isn't drawing:

- show three tabs with counts: "Open (2)", "Done (1)", "All (3)"
- filter rows by scope and by a search text, ignoring case
- format each row: id, title, an age like "3d" or "5m"
- decide whether tapping a task completes it or reopens it
- show `Added "Buy milk"` after an add, and turn every error into the same words
- explain an empty list: `Nothing here yet`, `All done. Nice.`, `No tasks match "x".`
- reload after every change

None of that is a business rule, but all of it is product behavior. A product manager would file a bug if Android said "No tasks" and iOS said "Nothing here yet". In an ordinary architecture each platform writes its own copy. Here it's written once, in `presentation.rs`.

## The Elm architecture, in Rust

The design comes from Elm (and Redux, and the Rust Crux framework, which does the same thing for mobile):

- The **state** is a plain struct.
- **Events** go in. `update(state, event)` changes the state and returns **effects**, which are descriptions of work to do, as data.
- `view(state)` builds the **ViewModel** the UI renders.

Effects as data is the key move. `update` never calls a repository. It returns `Effect::Add("Buy milk")`, and something else performs it. So `update` is a pure function you can test with plain values.

## Intents, events and effects

Three enums describe everything the screen can do:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub enum Intent {
    Opened,
    Refreshed,
    AddSubmitted(String),
    ToggleRequested(u32),
    ScopeChanged(Scope),
    FilterChanged(String),
    MessageDismissed,
}
```

An **intent** is what the user meant. Shells only ever send these. Note `ToggleRequested`, not "complete" or "reopen": the shell doesn't know which one a tap means. That's a product decision, so the state machine makes it.

**Events** are intents plus the results of effects coming back:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub enum Event {
    User(Intent),
    Loaded(Result<Vec<TaskSummary>, TasksError>),
    Changed(Change, Result<TaskSummary, TasksError>),
}
```

**Effects** are the work the state machine asks for:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub enum Effect {
    LoadAll,
    Add(String),
    Complete(u32),
    Reopen(u32),
}
```

## update: deciding what happens

Here is how a toggle becomes either a complete or a reopen:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
            Intent::ToggleRequested(id) => match self.tasks.iter().find(|t| t.id == id) {
                Some(t) if t.is_done() => vec![Effect::Reopen(id)],
                Some(_) => vec![Effect::Complete(id)],
                None => vec![],
            },
```

And how a successful change produces a notice and a reload:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
            Event::Changed(change, Ok(task)) => {
                self.error = None;
                self.notice = Some(match change {
                    Change::Added => format!("Added \"{}\"", task.title),
                    Change::Completed => format!("Done: \"{}\"", task.title),
                    Change::Reopened => format!("Reopened \"{}\"", task.title),
                });
                self.loading = true;
                vec![Effect::LoadAll]
            }
```

Scope and filter changes return no effects at all. The state machine loads every task once and filters in `view()`, so switching tabs is instant and the tab counts are always right.

## view: everything the renderer needs

`view(now)` returns a `TaskListVm`:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub struct TaskListVm {
    pub tabs: Vec<ScopeTab>,
    pub filter: String,
    pub columns: Vec<Column>,
    pub rows: Vec<TaskRow>,
    pub empty_message: Option<String>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// False for viewers: shells hide the add form and disable the toggles.
    pub can_change: bool,
}
```

Everything is **already decided**: labels with counts, which tab is selected, rows already filtered, every cell already formatted, and the empty-state text. A renderer only maps fields to widgets. Even permissions arrive as data: `can_change` comes from the role policy (chapter 13), so no shell ever asks "is this person a viewer?".

`now` is a parameter because ages ("5m") depend on the time, and the state machine must stay pure. The controller passes the clock's time in.

### Columns as data (a k9s lesson)

k9s, the Kubernetes terminal UI, draws hundreds of resource types with one generic table. Each resource provides a header (columns with attributes like alignment and "wide") and rows of strings. taskr copies that idea:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub struct Column {
    pub title: &'static str,
    pub align: Align,
    /// Only worth showing when there is room (k9s calls these wide columns).
    pub wide: bool,
}
```

The TUI and the web app draw a table straight from `columns` and `cells`. The mobile apps skip `wide` columns (the id) because phone screens are narrow. Nobody hardcodes "hide the first column".

### One place for words

Every error becomes text in exactly one function:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
/// The one place where errors become words. Every shell shows the same text.
pub fn describe(e: &TasksError) -> String {
```

Because `TaskError` is an enum and `match` is exhaustive, adding a new domain error doesn't compile until it has a message. When this product gets translated, this function becomes the lookup into a message catalog, still in one place.

## The controller: a tiny runtime

`TaskListState` is pure. Something has to actually run the effects. That's `TaskListController`, and it's short:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub struct TaskListController {
    state: Mutex<TaskListState>,
    api: Arc<dyn TasksApi>,
    workspace: WorkspaceId,
    clock: Arc<dyn Clock>,
}
```

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
    pub async fn dispatch(&self, intent: Intent) -> TaskListVm {
        let mut queue = VecDeque::from([Event::User(intent)]);
        while let Some(event) = queue.pop_front() {
            let effects = self.state().update(event);
            for effect in effects {
                queue.push_back(run(self.api.as_ref(), &self.workspace, effect).await);
            }
        }
        self.view()
    }
```

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
pub async fn run(api: &dyn TasksApi, workspace: &WorkspaceId, effect: Effect) -> Event {
    match effect {
        Effect::LoadAll => Event::Loaded(api.list(workspace).await),
        Effect::Add(title) => Event::Changed(Change::Added, api.add(workspace, title).await),
        Effect::Complete(id) => {
            Event::Changed(Change::Completed, api.complete(workspace, id).await)
        }
        Effect::Reopen(id) => Event::Changed(Change::Reopened, api.reopen(workspace, id).await),
    }
}
```

A queue of events, an effect runner, and a final view. Every shell calls `dispatch(intent)` and renders what comes back.

The controller doesn't call the use cases directly. It calls `TasksApi`, a port that says "list, add, complete, reopen in this workspace". Signed out, `LocalTasks` implements it by running the use cases in process (`TaskListController::local(tasks)` wires that up). Signed in, an HTTP client implements it. The controller can't tell the difference, which is exactly the point.

::: note
`dispatch` is async because the ports are. The state lock is only held inside `update`, never across an `.await`, so two dispatches in flight interleave like messages instead of blocking each other. When this book was first written every port was synchronous; making them async (chapter 13) changed the controller and the facades, and the state machine not at all, because it already modeled "loading" and "loaded" as separate events.
:::

## Fixtures: one set of screens for every platform

`presentation::fixtures` builds named screens by driving the real state machine:

```{.rust file="crates/taskr-core/src/tasks/presentation.rs"}
    pub fn all() -> Vec<(&'static str, TaskListVm)> {
        vec![
            ("empty", empty()),
            ("loading", loading()),
            ("sample", sample()),
            ("error", with_error()),
            ("read-only", read_only()),
        ]
    }
```

Both facades export them as `previewScreens()`. SwiftUI previews render them, the web smoke test checks them, and they're what you'd feed into snapshot tests on every platform. It's the same role Storybook stories play in the React guide, except one set serves all five UIs.

## What stays out of the shared ViewModel

Not everything belongs here. Keep these in the shell:

- **Cursor and selection.** Which row the TUI highlights is UI mechanics. The phone has no cursor.
- **Text being typed.** The add box's draft is local until the user submits.
- **Navigation mechanics.** "Show the detail screen" can be an effect or a field, but *how* (push, sheet, split view) is the shell's.
- **Animations, focus, accessibility wiring.**

The test from chapter 3 decides edge cases: would a product manager file a ticket about it?

::: exercise
Continue the rename feature from chapter 5:

1. Add `Intent::RenameSubmitted { id, title }`, `Effect::Rename { id, title }` and `Change::Renamed`. Let the compiler show you every `match` to update.
2. Write a pure `TaskListState` test: a successful rename sets the notice `Renamed "New title"` and asks for `LoadAll`.
3. Add the controller branch. Run the controller test with the in-memory repository.
4. Count how many files changed outside `taskr-core`. Right now it should be zero.
:::
