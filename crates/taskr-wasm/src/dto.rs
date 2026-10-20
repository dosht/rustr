//! The TypeScript-facing contract. `cargo test -p taskr-wasm` regenerates the
//! `.ts` files under `apps/web/src/lib/core/types/`.

use serde::{Deserialize, Serialize};
use taskr_core::tasks::app::Scope;
use taskr_core::tasks::presentation::{Align, Intent, TaskListVm};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TaskScope {
    Open,
    Done,
    All,
}

#[derive(Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum TaskIntent {
    Opened,
    Refreshed,
    AddSubmitted { title: String },
    ToggleRequested { id: u32 },
    ScopeChanged { scope: TaskScope },
    FilterChanged { text: String },
    MessageDismissed,
}

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ColumnAlign {
    Left,
    Right,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct ColumnView {
    pub title: String,
    pub align: ColumnAlign,
    pub wide: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct TaskRowView {
    pub id: u32,
    pub title: String,
    pub done: bool,
    pub cells: Vec<String>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct ScopeTabView {
    pub scope: TaskScope,
    pub label: String,
    pub selected: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskListView {
    pub tabs: Vec<ScopeTabView>,
    pub filter: String,
    pub columns: Vec<ColumnView>,
    pub rows: Vec<TaskRowView>,
    pub empty_message: Option<String>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub can_change: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct PreviewScreen {
    pub name: String,
    pub view: TaskListView,
}

impl From<TaskScope> for Scope {
    fn from(s: TaskScope) -> Scope {
        match s {
            TaskScope::Open => Scope::Open,
            TaskScope::Done => Scope::Done,
            TaskScope::All => Scope::All,
        }
    }
}

impl From<Scope> for TaskScope {
    fn from(s: Scope) -> TaskScope {
        match s {
            Scope::Open => TaskScope::Open,
            Scope::Done => TaskScope::Done,
            Scope::All => TaskScope::All,
        }
    }
}

impl From<TaskIntent> for Intent {
    fn from(i: TaskIntent) -> Intent {
        match i {
            TaskIntent::Opened => Intent::Opened,
            TaskIntent::Refreshed => Intent::Refreshed,
            TaskIntent::AddSubmitted { title } => Intent::AddSubmitted(title),
            TaskIntent::ToggleRequested { id } => Intent::ToggleRequested(id),
            TaskIntent::ScopeChanged { scope } => Intent::ScopeChanged(scope.into()),
            TaskIntent::FilterChanged { text } => Intent::FilterChanged(text),
            TaskIntent::MessageDismissed => Intent::MessageDismissed,
        }
    }
}

impl From<TaskListVm> for TaskListView {
    fn from(vm: TaskListVm) -> Self {
        TaskListView {
            tabs: vm
                .tabs
                .into_iter()
                .map(|t| ScopeTabView {
                    scope: t.scope.into(),
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
                        Align::Left => ColumnAlign::Left,
                        Align::Right => ColumnAlign::Right,
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
            can_change: vm.can_change,
        }
    }
}
