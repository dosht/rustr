//! Errors on the wire: `{"error": {"code": "...", "message": "...", "detail": "..."}}`.
//! `message` is the human text from `describe()`, for curl users. Clients map
//! `code` and `detail` back to the core error and describe it themselves.

use serde::{Deserialize, Serialize};
use taskr_core::shared::access::{AccessError, Action, Email};
use taskr_core::shared::storage::StorageError;
use taskr_core::tasks::app::TasksError;
use taskr_core::tasks::domain::{TaskError, TaskId, TitleError};
use taskr_core::tasks::presentation::describe as describe_tasks;
use taskr_core::workspaces::app::WorkspacesError;
use taskr_core::workspaces::domain::WorkspaceError;
use taskr_core::workspaces::presentation::describe as describe_workspaces;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WireError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    pub error: WireError,
}

fn wire(status: u16, code: &str, detail: Option<String>, message: String) -> (u16, ErrorBody) {
    (status, ErrorBody { error: WireError { code: code.into(), message, detail } })
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::ViewTasks => "view_tasks",
        Action::ChangeTasks => "change_tasks",
        Action::ViewMembers => "view_members",
        Action::ManageMembers => "manage_members",
    }
}

fn parse_action(s: &str) -> Option<Action> {
    [Action::ViewTasks, Action::ChangeTasks, Action::ViewMembers, Action::ManageMembers]
        .into_iter()
        .find(|a| action_name(*a) == s)
}

fn access_parts(e: &AccessError) -> (u16, &'static str, Option<String>) {
    match e {
        AccessError::Unauthenticated => (401, "unauthenticated", None),
        AccessError::NotAMember => (404, "not_a_member", None),
        AccessError::Forbidden(a) => (403, "forbidden", Some(action_name(*a).into())),
    }
}

fn access_from(code: &str, detail: Option<&str>) -> Option<AccessError> {
    Some(match code {
        "unauthenticated" => AccessError::Unauthenticated,
        "not_a_member" => AccessError::NotAMember,
        "forbidden" => AccessError::Forbidden(parse_action(detail?)?),
        _ => return None,
    })
}

pub fn from_tasks_error(e: &TasksError) -> (u16, ErrorBody) {
    let message = describe_tasks(e);
    match e {
        TasksError::Access(a) => {
            let (status, code, detail) = access_parts(a);
            wire(status, code, detail, message)
        }
        TasksError::Storage(s) => wire(503, "storage", Some(s.0.clone()), message),
        TasksError::Rejected(r) => {
            let (status, code, detail) = match r {
                TaskError::InvalidTitle(TitleError::Empty) => (422, "title_empty", None),
                TaskError::InvalidTitle(TitleError::TooLong { max }) => {
                    (422, "title_too_long", Some(max.to_string()))
                }
                TaskError::InvalidTitle(TitleError::ControlCharacter) => {
                    (422, "title_control_character", None)
                }
                TaskError::DuplicateOpenTitle => (409, "duplicate_open_title", None),
                TaskError::TooManyOpenTasks { max } => {
                    (409, "too_many_open_tasks", Some(max.to_string()))
                }
                TaskError::NotFound(id) => (404, "task_not_found", Some(id.0.to_string())),
                TaskError::AlreadyDone(id) => (409, "task_already_done", Some(id.0.to_string())),
                TaskError::AlreadyOpen(id) => (409, "task_already_open", Some(id.0.to_string())),
            };
            wire(status, code, detail, message)
        }
    }
}

pub fn to_tasks_error(status: u16, body: &WireError) -> TasksError {
    let detail = body.detail.as_deref();
    let num = || detail.and_then(|d| d.parse::<u32>().ok());
    if let Some(a) = access_from(&body.code, detail) {
        return TasksError::Access(a);
    }
    let rejected = match body.code.as_str() {
        "title_empty" => Some(TaskError::InvalidTitle(TitleError::Empty)),
        "title_too_long" => {
            num().map(|max| TaskError::InvalidTitle(TitleError::TooLong { max: max as usize }))
        }
        "title_control_character" => Some(TaskError::InvalidTitle(TitleError::ControlCharacter)),
        "duplicate_open_title" => Some(TaskError::DuplicateOpenTitle),
        "too_many_open_tasks" => num().map(|max| TaskError::TooManyOpenTasks { max: max as usize }),
        "task_not_found" => num().map(|id| TaskError::NotFound(TaskId(id))),
        "task_already_done" => num().map(|id| TaskError::AlreadyDone(TaskId(id))),
        "task_already_open" => num().map(|id| TaskError::AlreadyOpen(TaskId(id))),
        _ => None,
    };
    match rejected {
        Some(r) => TasksError::Rejected(r),
        None => TasksError::Storage(StorageError(unexpected(status, body))),
    }
}

pub fn from_workspaces_error(e: &WorkspacesError) -> (u16, ErrorBody) {
    let message = describe_workspaces(e);
    match e {
        WorkspacesError::Access(a) => {
            let (status, code, detail) = access_parts(a);
            wire(status, code, detail, message)
        }
        WorkspacesError::Storage(s) => wire(503, "storage", Some(s.0.clone()), message),
        WorkspacesError::Rejected(r) => {
            let (status, code, detail) = match r {
                WorkspaceError::InvalidName => (422, "workspace_name_invalid", None),
                WorkspaceError::InvalidEmail => (422, "email_invalid", None),
                WorkspaceError::LastOwner => (409, "last_owner", None),
                WorkspaceError::NotAMember(email) => {
                    (404, "member_not_found", Some(email.as_str().to_string()))
                }
            };
            wire(status, code, detail, message)
        }
    }
}

pub fn to_workspaces_error(status: u16, body: &WireError) -> WorkspacesError {
    let detail = body.detail.as_deref();
    if let Some(a) = access_from(&body.code, detail) {
        return WorkspacesError::Access(a);
    }
    let rejected = match body.code.as_str() {
        "workspace_name_invalid" => Some(WorkspaceError::InvalidName),
        "email_invalid" => Some(WorkspaceError::InvalidEmail),
        "last_owner" => Some(WorkspaceError::LastOwner),
        "member_not_found" => {
            detail.and_then(|d| Email::parse(d).ok()).map(WorkspaceError::NotAMember)
        }
        _ => None,
    };
    match rejected {
        Some(r) => WorkspacesError::Rejected(r),
        None => WorkspacesError::Storage(StorageError(unexpected(status, body))),
    }
}

/// Anything the client cannot map is shown as a storage problem with the
/// server's own words, so nothing is silently lost.
fn unexpected(status: u16, body: &WireError) -> String {
    match &body.detail {
        Some(d) if body.code == "storage" => d.clone(),
        _ => format!("server said {status} {}: {}", body.code, body.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_tasks_error() -> Vec<TasksError> {
        vec![
            TasksError::Rejected(TaskError::InvalidTitle(TitleError::Empty)),
            TasksError::Rejected(TaskError::InvalidTitle(TitleError::TooLong { max: 80 })),
            TasksError::Rejected(TaskError::InvalidTitle(TitleError::ControlCharacter)),
            TasksError::Rejected(TaskError::DuplicateOpenTitle),
            TasksError::Rejected(TaskError::TooManyOpenTasks { max: 50 }),
            TasksError::Rejected(TaskError::NotFound(TaskId(7))),
            TasksError::Rejected(TaskError::AlreadyDone(TaskId(7))),
            TasksError::Rejected(TaskError::AlreadyOpen(TaskId(7))),
            TasksError::Access(AccessError::Unauthenticated),
            TasksError::Access(AccessError::NotAMember),
            TasksError::Access(AccessError::Forbidden(Action::ChangeTasks)),
            TasksError::Storage(StorageError("db down".into())),
        ]
    }

    #[test]
    fn every_tasks_error_survives_the_wire() {
        for e in every_tasks_error() {
            let (status, body) = from_tasks_error(&e);
            let json = serde_json::to_string(&body).unwrap();
            let back: ErrorBody = serde_json::from_str(&json).unwrap();
            assert_eq!(to_tasks_error(status, &back.error), e, "{json}");
        }
    }

    #[test]
    fn every_workspaces_error_survives_the_wire() {
        let all = vec![
            WorkspacesError::Rejected(WorkspaceError::InvalidName),
            WorkspacesError::Rejected(WorkspaceError::InvalidEmail),
            WorkspacesError::Rejected(WorkspaceError::LastOwner),
            WorkspacesError::Rejected(WorkspaceError::NotAMember(Email::parse("a@b.co").unwrap())),
            WorkspacesError::Access(AccessError::Forbidden(Action::ManageMembers)),
            WorkspacesError::Access(AccessError::Unauthenticated),
            WorkspacesError::Storage(StorageError("db down".into())),
        ];
        for e in all {
            let (status, body) = from_workspaces_error(&e);
            assert_eq!(to_workspaces_error(status, &body.error), e);
        }
    }

    #[test]
    fn statuses_follow_http_meaning() {
        let status = |e: TasksError| from_tasks_error(&e).0;
        assert_eq!(status(TasksError::Access(AccessError::Unauthenticated)), 401);
        assert_eq!(status(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks))), 403);
        assert_eq!(status(TasksError::Access(AccessError::NotAMember)), 404);
        assert_eq!(status(TasksError::Rejected(TaskError::DuplicateOpenTitle)), 409);
        assert_eq!(status(TasksError::Rejected(TaskError::InvalidTitle(TitleError::Empty))), 422);
    }

    #[test]
    fn unknown_codes_keep_the_server_message() {
        let body =
            WireError { code: "teapot".into(), message: "short and stout".into(), detail: None };
        assert_eq!(
            to_tasks_error(418, &body),
            TasksError::Storage(StorageError("server said 418 teapot: short and stout".into()))
        );
    }
}
