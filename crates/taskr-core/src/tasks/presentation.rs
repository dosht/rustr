//! The shared ViewModel for the task list screen.
//!
//! `TaskListState` is a pure state machine: `update(event) -> effects` and
//! `view(now) -> TaskListVm`. It never does I/O. `TaskListController` is the
//! small runtime that executes effects through the use cases and feeds the
//! results back in. Shells only send `Intent`s and render `TaskListVm`.
//!
//! What stays in each shell: cursor and scrolling, focus, text input buffers,
//! navigation mechanics, platform widgets. What lives here: anything that
//! would otherwise be written once per platform.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::shared::access::{AccessError, Action, WorkspaceId, describe_access};
use crate::shared::clock::{Clock, Millis};
use crate::tasks::app::{LocalTasks, Scope, TaskSummary, Tasks, TasksApi, TasksError};
use crate::tasks::domain::{TaskError, TitleError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Opened,
    Refreshed,
    AddSubmitted(String),
    ToggleRequested(u32),
    ScopeChanged(Scope),
    FilterChanged(String),
    MessageDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    User(Intent),
    Loaded(Result<Vec<TaskSummary>, TasksError>),
    Changed(Change, Result<TaskSummary, TasksError>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Completed,
    Reopened,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    LoadAll,
    Add(String),
    Complete(u32),
    Reopen(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub title: &'static str,
    pub align: Align,
    /// Only worth showing when there is room (k9s calls these wide columns).
    pub wide: bool,
}

pub const COLUMNS: [Column; 3] = [
    Column { title: "ID", align: Align::Right, wide: true },
    Column { title: "TITLE", align: Align::Left, wide: false },
    Column { title: "AGE", align: Align::Right, wide: false },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub id: u32,
    pub title: String,
    pub done: bool,
    /// One value per entry in `TaskListVm::columns`, already formatted.
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeTab {
    pub scope: Scope,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskListState {
    scope: Scope,
    filter: String,
    tasks: Vec<TaskSummary>,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    can_change: bool,
}

impl Default for TaskListState {
    fn default() -> Self {
        Self::new(true)
    }
}

impl TaskListState {
    pub fn new(can_change: bool) -> Self {
        Self {
            scope: Scope::default(),
            filter: String::new(),
            tasks: vec![],
            loading: false,
            error: None,
            notice: None,
            can_change,
        }
    }

    /// The role changed while the screen was open.
    pub fn set_can_change(&mut self, can_change: bool) {
        self.can_change = can_change;
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::User(intent) => self.on_intent(intent),
            Event::Loaded(Ok(tasks)) => {
                self.loading = false;
                self.tasks = tasks;
                vec![]
            }
            Event::Loaded(Err(e)) => {
                self.loading = false;
                self.show_error(&e);
                vec![]
            }
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
            Event::Changed(_, Err(e)) => {
                self.show_error(&e);
                vec![]
            }
        }
    }

    fn on_intent(&mut self, intent: Intent) -> Vec<Effect> {
        match intent {
            Intent::Opened | Intent::Refreshed => {
                self.loading = true;
                vec![Effect::LoadAll]
            }
            Intent::AddSubmitted(_) | Intent::ToggleRequested(_) if !self.can_change => {
                self.show_error(&AccessError::Forbidden(Action::ChangeTasks).into());
                vec![]
            }
            Intent::AddSubmitted(title) => vec![Effect::Add(title)],
            Intent::ToggleRequested(id) => match self.tasks.iter().find(|t| t.id == id) {
                Some(t) if t.is_done() => vec![Effect::Reopen(id)],
                Some(_) => vec![Effect::Complete(id)],
                None => vec![],
            },
            Intent::ScopeChanged(scope) => {
                self.scope = scope;
                vec![]
            }
            Intent::FilterChanged(text) => {
                self.filter = text;
                vec![]
            }
            Intent::MessageDismissed => {
                self.error = None;
                self.notice = None;
                vec![]
            }
        }
    }

    fn show_error(&mut self, e: &TasksError) {
        self.notice = None;
        self.error = Some(describe(e));
    }

    pub fn view(&self, now: Millis) -> TaskListVm {
        let needle = self.filter.trim().to_lowercase();
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

        let open = self.tasks.iter().filter(|t| !t.is_done()).count();
        let done = self.tasks.len() - open;
        let tab = |scope, label: String| ScopeTab { scope, label, selected: self.scope == scope };

        TaskListVm {
            tabs: vec![
                tab(Scope::Open, format!("Open ({open})")),
                tab(Scope::Done, format!("Done ({done})")),
                tab(Scope::All, format!("All ({})", self.tasks.len())),
            ],
            filter: self.filter.clone(),
            columns: COLUMNS.to_vec(),
            empty_message: rows.is_empty().then(|| self.empty_message(&needle)),
            rows,
            is_loading: self.loading,
            error: self.error.clone(),
            notice: self.notice.clone(),
            can_change: self.can_change,
        }
    }

    fn empty_message(&self, needle: &str) -> String {
        if !needle.is_empty() {
            return format!("No tasks match \"{}\".", self.filter.trim());
        }
        match self.scope {
            Scope::Open if self.tasks.is_empty() => "Nothing here yet. Add your first task.".into(),
            Scope::Open => "All done. Nice.".into(),
            Scope::Done => "No finished tasks yet.".into(),
            Scope::All => "Nothing here yet. Add your first task.".into(),
        }
    }
}

/// The one place where errors become words. Every shell shows the same text.
pub fn describe(e: &TasksError) -> String {
    match e {
        TasksError::Storage(s) => format!("Could not read or save your tasks ({}).", s.0),
        TasksError::Access(a) => describe_access(a),
        TasksError::Rejected(r) => match r {
            TaskError::InvalidTitle(TitleError::Empty) => "A task needs a title.".into(),
            TaskError::InvalidTitle(TitleError::TooLong { max }) => {
                format!("Titles can be at most {max} characters.")
            }
            TaskError::InvalidTitle(TitleError::ControlCharacter) => {
                "Titles cannot contain control characters.".into()
            }
            TaskError::DuplicateOpenTitle => {
                "There is already an open task with that title.".into()
            }
            TaskError::TooManyOpenTasks { max } => {
                format!("You can have at most {max} open tasks. Finish some first.")
            }
            TaskError::NotFound(id) => format!("Task {id} does not exist."),
            TaskError::AlreadyDone(id) => format!("Task {id} is already done."),
            TaskError::AlreadyOpen(id) => format!("Task {id} is already open."),
        },
    }
}

pub fn age_label(from: Millis, now: Millis) -> String {
    let secs = (now - from).max(0) / 1000;
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

pub struct TaskListController {
    state: Mutex<TaskListState>,
    api: Arc<dyn TasksApi>,
    workspace: WorkspaceId,
    clock: Arc<dyn Clock>,
}

impl TaskListController {
    pub fn new(api: Arc<dyn TasksApi>, workspace: WorkspaceId, clock: Arc<dyn Clock>) -> Self {
        Self { state: Mutex::new(TaskListState::default()), api, workspace, clock }
    }

    /// The signed-out shells: the local workspace, run in process.
    pub fn local(tasks: Tasks) -> Self {
        let clock = tasks.clock();
        Self::new(Arc::new(LocalTasks::new(tasks)), WorkspaceId::local(), clock)
    }

    /// The state lock is only held while updating, never across an await, so
    /// concurrent dispatches interleave like messages instead of blocking.
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

    pub fn view(&self) -> TaskListVm {
        self.state().view(self.clock.now())
    }

    fn state(&self) -> MutexGuard<'_, TaskListState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

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

/// Named screens for previews, Storybook and snapshot tests on every platform.
pub mod fixtures {
    use super::*;

    pub const NOW: Millis = 1_760_000_000_000;
    const MIN: Millis = 60_000;

    fn task(id: u32, title: &str, age_min: i64, done: bool) -> TaskSummary {
        TaskSummary {
            id,
            title: title.into(),
            created_at: NOW - age_min * MIN,
            done_at: done.then_some(NOW - MIN),
        }
    }

    fn loaded(tasks: Vec<TaskSummary>) -> TaskListState {
        let mut s = TaskListState::default();
        s.update(Event::Loaded(Ok(tasks)));
        s
    }

    pub fn empty() -> TaskListVm {
        loaded(vec![]).view(NOW)
    }

    pub fn loading() -> TaskListVm {
        let mut s = TaskListState::default();
        s.update(Event::User(Intent::Opened));
        s.view(NOW)
    }

    pub fn sample() -> TaskListVm {
        sample_state().view(NOW)
    }

    pub fn with_error() -> TaskListVm {
        let mut s = sample_state();
        s.update(Event::Changed(
            Change::Added,
            Err(TasksError::Rejected(TaskError::DuplicateOpenTitle)),
        ));
        s.view(NOW)
    }

    pub fn read_only() -> TaskListVm {
        let mut s = sample_state();
        s.can_change = false;
        s.view(NOW)
    }

    pub fn all() -> Vec<(&'static str, TaskListVm)> {
        vec![
            ("empty", empty()),
            ("loading", loading()),
            ("sample", sample()),
            ("error", with_error()),
            ("read-only", read_only()),
        ]
    }

    pub fn sample_tasks() -> Vec<TaskSummary> {
        vec![
            task(1, "Write the architecture doc", 3 * 24 * 60, false),
            task(2, "Try the TUI with vim keys", 90, false),
            task(4, "Generate Swift bindings", 5, false),
            task(3, "Set up the workspace", 2 * 24 * 60, true),
        ]
    }

    fn sample_state() -> TaskListState {
        loaded(sample_tasks())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::telemetry::NoTelemetry;
    use crate::tasks::app::StorageError;
    use crate::testing::{FixedClock, InMemoryTaskListRepository, block_on};

    fn summary(id: u32, title: &str, done: bool) -> TaskSummary {
        TaskSummary { id, title: title.into(), created_at: 0, done_at: done.then_some(1) }
    }

    fn loaded(tasks: Vec<TaskSummary>) -> TaskListState {
        let mut s = TaskListState::default();
        s.update(Event::Loaded(Ok(tasks)));
        s
    }

    fn titles(vm: &TaskListVm) -> Vec<&str> {
        vm.rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn opening_loads_everything() {
        let mut s = TaskListState::default();
        assert_eq!(s.update(Event::User(Intent::Opened)), vec![Effect::LoadAll]);
        assert!(s.view(0).is_loading);
    }

    #[test]
    fn scope_and_filter_are_applied_without_reloading() {
        let mut s = loaded(vec![summary(1, "Buy milk", false), summary(2, "Buy bread", true)]);
        assert_eq!(titles(&s.view(0)), ["Buy milk"]);

        assert!(s.update(Event::User(Intent::ScopeChanged(Scope::All))).is_empty());
        assert_eq!(titles(&s.view(0)), ["Buy milk", "Buy bread"]);

        assert!(s.update(Event::User(Intent::FilterChanged(" BREAD ".into()))).is_empty());
        assert_eq!(titles(&s.view(0)), ["Buy bread"]);
    }

    #[test]
    fn tabs_show_counts_and_selection() {
        let s = loaded(vec![summary(1, "a", false), summary(2, "b", true), summary(3, "c", true)]);
        let tabs = s.view(0).tabs;
        let labels: Vec<_> = tabs.iter().map(|t| (t.label.as_str(), t.selected)).collect();
        assert_eq!(labels, [("Open (1)", true), ("Done (2)", false), ("All (3)", false)]);
    }

    #[test]
    fn toggle_picks_complete_or_reopen_from_current_state() {
        let mut s = loaded(vec![summary(1, "a", false), summary(2, "b", true)]);
        assert_eq!(s.update(Event::User(Intent::ToggleRequested(1))), vec![Effect::Complete(1)]);
        assert_eq!(s.update(Event::User(Intent::ToggleRequested(2))), vec![Effect::Reopen(2)]);
        assert!(s.update(Event::User(Intent::ToggleRequested(9))).is_empty());
    }

    #[test]
    fn successful_change_sets_a_notice_and_reloads() {
        let mut s = loaded(vec![]);
        let effects = s.update(Event::Changed(Change::Added, Ok(summary(1, "Buy milk", false))));
        assert_eq!(effects, vec![Effect::LoadAll]);
        assert_eq!(s.view(0).notice.as_deref(), Some("Added \"Buy milk\""));
    }

    #[test]
    fn failed_change_shows_the_described_error() {
        let mut s = loaded(vec![]);
        let err = TasksError::Rejected(TaskError::InvalidTitle(TitleError::Empty));
        assert!(s.update(Event::Changed(Change::Added, Err(err))).is_empty());
        let vm = s.view(0);
        assert_eq!(vm.error.as_deref(), Some("A task needs a title."));
        assert_eq!(vm.notice, None);

        s.update(Event::User(Intent::MessageDismissed));
        assert_eq!(s.view(0).error, None);
    }

    #[test]
    fn storage_errors_are_described() {
        let mut s = TaskListState::default();
        s.update(Event::Loaded(Err(TasksError::Storage(StorageError("denied".into())))));
        assert_eq!(s.view(0).error.as_deref(), Some("Could not read or save your tasks (denied)."));
    }

    #[test]
    fn empty_messages_explain_why() {
        let mut s = loaded(vec![]);
        assert_eq!(
            s.view(0).empty_message.as_deref(),
            Some("Nothing here yet. Add your first task.")
        );
        s.update(Event::User(Intent::FilterChanged("zzz".into())));
        assert_eq!(s.view(0).empty_message.as_deref(), Some("No tasks match \"zzz\"."));

        let done_only = loaded(vec![summary(1, "a", true)]);
        assert_eq!(done_only.view(0).empty_message.as_deref(), Some("All done. Nice."));
    }

    #[test]
    fn rows_carry_one_cell_per_column() {
        let vm = loaded(vec![summary(7, "a", false)]).view(2 * 3_600_000);
        assert_eq!(vm.columns.len(), vm.rows[0].cells.len());
        assert_eq!(vm.rows[0].cells, ["7", "a", "2h"]);
    }

    #[test]
    fn age_labels() {
        assert_eq!(age_label(0, 59_000), "now");
        assert_eq!(age_label(0, 61_000), "1m");
        assert_eq!(age_label(0, 3_600_000), "1h");
        assert_eq!(age_label(0, 3 * 86_400_000), "3d");
        assert_eq!(age_label(10, 0), "now");
    }

    #[test]
    fn viewers_see_why_they_cannot_change_anything() {
        let mut s = TaskListState::new(false);
        s.update(Event::Loaded(Ok(vec![summary(1, "a", false)])));
        assert!(!s.view(0).can_change);
        assert!(s.update(Event::User(Intent::ToggleRequested(1))).is_empty());
        assert!(s.update(Event::User(Intent::AddSubmitted("b".into()))).is_empty());
        assert_eq!(s.view(0).error.as_deref(), Some("Viewers cannot change tasks."));
    }

    #[test]
    fn fixtures_cover_the_main_screen_states() {
        assert!(fixtures::loading().is_loading);
        assert!(fixtures::empty().empty_message.is_some());
        assert_eq!(fixtures::sample().rows.len(), 3);
        assert!(fixtures::with_error().error.is_some());
        assert_eq!(fixtures::sample().rows[0].cells[2], "3d");
        assert!(fixtures::sample().can_change && !fixtures::read_only().can_change);
    }

    #[test]
    fn controller_runs_the_full_loop_through_the_use_cases() {
        let tasks = Tasks::new(
            Arc::new(InMemoryTaskListRepository::default()),
            Arc::new(FixedClock::at(0)),
            Arc::new(NoTelemetry),
        );
        let c = TaskListController::local(tasks);
        assert!(block_on(c.dispatch(Intent::Opened)).rows.is_empty());

        let vm = block_on(c.dispatch(Intent::AddSubmitted("Buy milk".into())));
        assert_eq!(titles(&vm), ["Buy milk"]);
        assert!(!vm.is_loading);

        let vm = block_on(c.dispatch(Intent::AddSubmitted("buy milk".into())));
        assert_eq!(vm.error.as_deref(), Some("There is already an open task with that title."));

        let vm = block_on(c.dispatch(Intent::ToggleRequested(1)));
        assert!(vm.rows.is_empty());
        assert_eq!(vm.tabs[1].label, "Done (1)");
        assert_eq!(vm.notice.as_deref(), Some("Done: \"Buy milk\""));
    }
}
