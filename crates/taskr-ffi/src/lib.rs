//! The mobile facade. Coarse grained on purpose: one object per screen, one
//! `dispatch` call per user action, plain records out. Domain types never
//! cross this boundary.
//!
//! Threading: every call is synchronous and may touch the disk, so shells call
//! `dispatch` off the main thread. The presenter is internally locked and safe
//! to share.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use taskr_core::shared::telemetry::{Level, LogEvent, Telemetry};
use taskr_core::tasks::app::{Scope, Tasks};
use taskr_core::tasks::presentation::{
    self as p, Intent, TaskListController, TaskListVm, fixtures,
};
use taskr_infra::clock::SystemClock;
use taskr_infra::json_file::JsonFileTaskListRepository;
use taskr_infra::paths;

uniffi::setup_scaffolding!();

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskScope {
    Open,
    Done,
    All,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum TaskIntent {
    Opened,
    Refreshed,
    AddSubmitted { title: String },
    ToggleRequested { id: u32 },
    ScopeChanged { scope: TaskScope },
    FilterChanged { text: String },
    MessageDismissed,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Right,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ColumnView {
    pub title: String,
    pub align: ColumnAlign,
    pub wide: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TaskRowView {
    pub id: u32,
    pub title: String,
    pub done: bool,
    pub cells: Vec<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ScopeTabView {
    pub scope: TaskScope,
    pub label: String,
    pub selected: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TaskListView {
    pub tabs: Vec<ScopeTabView>,
    pub filter: String,
    pub columns: Vec<ColumnView>,
    pub rows: Vec<TaskRowView>,
    pub empty_message: Option<String>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PreviewScreen {
    pub name: String,
    pub view: TaskListView,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct LogField {
    pub key: String,
    pub value: String,
}

/// Implemented in Kotlin (logcat) and Swift (os_log): the telemetry port,
/// fulfilled by the platform.
#[uniffi::export(callback_interface)]
pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, event: String, fields: Vec<LogField>);
}

struct ForeignTelemetry(Box<dyn LogSink>);

impl Telemetry for ForeignTelemetry {
    fn record(&self, event: LogEvent) {
        let level = match event.level {
            Level::Debug => LogLevel::Debug,
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        };
        let fields = event
            .fields
            .into_iter()
            .map(|(k, v)| LogField { key: k.to_string(), value: v })
            .collect();
        self.0.log(level, event.name.to_string(), fields);
    }
}

#[derive(uniffi::Object)]
pub struct TaskListPresenter {
    controller: Mutex<TaskListController>,
}

#[uniffi::export]
impl TaskListPresenter {
    /// `data_dir` is the app's private files directory
    /// (`Context.filesDir` on Android, Application Support on iOS).
    #[uniffi::constructor]
    pub fn new(data_dir: String, sink: Box<dyn LogSink>) -> Self {
        let telemetry: Arc<dyn Telemetry> = Arc::new(ForeignTelemetry(sink));
        telemetry.record(LogEvent::info("app.started").field("shell", "mobile"));
        let repo = JsonFileTaskListRepository::new(paths::tasks_file(&PathBuf::from(data_dir)));
        let tasks = Tasks::new(Arc::new(repo), Arc::new(SystemClock), telemetry);
        Self { controller: Mutex::new(TaskListController::new(tasks)) }
    }

    pub fn dispatch(&self, intent: TaskIntent) -> TaskListView {
        let mut controller = self.controller.lock().unwrap_or_else(|e| e.into_inner());
        to_view(controller.dispatch(to_intent(intent)))
    }

    pub fn current(&self) -> TaskListView {
        let controller = self.controller.lock().unwrap_or_else(|e| e.into_inner());
        to_view(controller.view())
    }
}

/// Fixture screens for SwiftUI and Compose previews and snapshot tests.
#[uniffi::export]
pub fn preview_screens() -> Vec<PreviewScreen> {
    fixtures::all()
        .into_iter()
        .map(|(name, vm)| PreviewScreen { name: name.to_string(), view: to_view(vm) })
        .collect()
}

fn to_scope(s: TaskScope) -> Scope {
    match s {
        TaskScope::Open => Scope::Open,
        TaskScope::Done => Scope::Done,
        TaskScope::All => Scope::All,
    }
}

fn from_scope(s: Scope) -> TaskScope {
    match s {
        Scope::Open => TaskScope::Open,
        Scope::Done => TaskScope::Done,
        Scope::All => TaskScope::All,
    }
}

fn to_intent(i: TaskIntent) -> Intent {
    match i {
        TaskIntent::Opened => Intent::Opened,
        TaskIntent::Refreshed => Intent::Refreshed,
        TaskIntent::AddSubmitted { title } => Intent::AddSubmitted(title),
        TaskIntent::ToggleRequested { id } => Intent::ToggleRequested(id),
        TaskIntent::ScopeChanged { scope } => Intent::ScopeChanged(to_scope(scope)),
        TaskIntent::FilterChanged { text } => Intent::FilterChanged(text),
        TaskIntent::MessageDismissed => Intent::MessageDismissed,
    }
}

fn to_view(vm: TaskListVm) -> TaskListView {
    TaskListView {
        tabs: vm
            .tabs
            .into_iter()
            .map(|t| ScopeTabView {
                scope: from_scope(t.scope),
                label: t.label,
                selected: t.selected,
            })
            .collect(),
        filter: vm.filter,
        columns: vm
            .columns
            .into_iter()
            .map(|c| ColumnView {
                title: c.title.to_string(),
                align: match c.align {
                    p::Align::Left => ColumnAlign::Left,
                    p::Align::Right => ColumnAlign::Right,
                },
                wide: c.wide,
            })
            .collect(),
        rows: vm
            .rows
            .into_iter()
            .map(|r| TaskRowView { id: r.id, title: r.title, done: r.done, cells: r.cells })
            .collect(),
        empty_message: vm.empty_message,
        is_loading: vm.is_loading,
        error: vm.error,
        notice: vm.notice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<String>>>);

    impl LogSink for Sink {
        fn log(&self, _level: LogLevel, event: String, _fields: Vec<LogField>) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn presenter_round_trip_through_the_facade() {
        let dir = std::env::temp_dir().join(format!("taskr-ffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sink = Sink::default();
        let presenter = TaskListPresenter::new(dir.display().to_string(), Box::new(sink.clone()));

        presenter.dispatch(TaskIntent::Opened);
        let view = presenter.dispatch(TaskIntent::AddSubmitted { title: "Buy milk".into() });
        assert_eq!(view.rows[0].title, "Buy milk");
        assert_eq!(view.tabs[0].scope, TaskScope::Open);

        let view = presenter.dispatch(TaskIntent::ScopeChanged { scope: TaskScope::All });
        assert!(view.tabs[2].selected);
        assert_eq!(*sink.0.lock().unwrap(), ["app.started", "task.added"]);
        assert!(dir.join("tasks.json").exists());
    }

    #[test]
    fn preview_screens_are_exported() {
        let names: Vec<_> = preview_screens().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["empty", "loading", "sample", "error"]);
    }
}
