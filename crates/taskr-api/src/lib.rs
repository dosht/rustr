//! The HTTP contract. The server serializes with these types and the client
//! deserializes with the same ones, and every error crosses the wire as a code
//! that maps back to the exact core error, so `describe()` reads the same on
//! both ends.

pub mod errors;
pub mod paths;

use serde::{Deserialize, Serialize};
use taskr_core::shared::access::{Role, WorkspaceId};
use taskr_core::tasks::app::TaskSummary;
use taskr_core::workspaces::app::{Me, MemberSummary, WorkspaceSummary};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskDto {
    pub id: u32,
    pub title: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<i64>,
}

impl From<TaskSummary> for TaskDto {
    fn from(t: TaskSummary) -> Self {
        TaskDto { id: t.id, title: t.title, created_at: t.created_at, done_at: t.done_at }
    }
}

impl From<TaskDto> for TaskSummary {
    fn from(t: TaskDto) -> Self {
        TaskSummary { id: t.id, title: t.title, created_at: t.created_at, done_at: t.done_at }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoleDto {
    Owner,
    Editor,
    Viewer,
}

impl From<Role> for RoleDto {
    fn from(r: Role) -> Self {
        match r {
            Role::Owner => RoleDto::Owner,
            Role::Editor => RoleDto::Editor,
            Role::Viewer => RoleDto::Viewer,
        }
    }
}

impl From<RoleDto> for Role {
    fn from(r: RoleDto) -> Self {
        match r {
            RoleDto::Owner => Role::Owner,
            RoleDto::Editor => Role::Editor,
            RoleDto::Viewer => Role::Viewer,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDto {
    pub id: String,
    pub name: String,
    pub role: RoleDto,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MeDto {
    pub email: String,
    pub workspaces: Vec<WorkspaceDto>,
}

impl From<Me> for MeDto {
    fn from(me: Me) -> Self {
        MeDto {
            email: me.email,
            workspaces: me
                .workspaces
                .into_iter()
                .map(|w| WorkspaceDto { id: w.id.0, name: w.name, role: w.role.into() })
                .collect(),
        }
    }
}

impl From<MeDto> for Me {
    fn from(me: MeDto) -> Self {
        Me {
            email: me.email,
            workspaces: me
                .workspaces
                .into_iter()
                .map(|w| WorkspaceSummary {
                    id: WorkspaceId(w.id),
                    name: w.name,
                    role: w.role.into(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MemberDto {
    pub email: String,
    pub role: RoleDto,
}

impl From<MemberSummary> for MemberDto {
    fn from(m: MemberSummary) -> Self {
        MemberDto { email: m.email, role: m.role.into() }
    }
}

impl From<MemberDto> for MemberSummary {
    fn from(m: MemberDto) -> Self {
        MemberSummary { email: m.email, role: m.role.into() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AddTaskBody {
    pub title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SetMemberBody {
    pub email: String,
    pub role: RoleDto,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RemoveMemberBody {
    pub email: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes_are_stable() {
        let task = TaskDto { id: 1, title: "a".into(), created_at: 5, done_at: None };
        assert_eq!(serde_json::to_string(&task).unwrap(), r#"{"id":1,"title":"a","created_at":5}"#);
        let body = SetMemberBody { email: "a@b.co".into(), role: RoleDto::Viewer };
        assert_eq!(serde_json::to_string(&body).unwrap(), r#"{"email":"a@b.co","role":"viewer"}"#);
    }

    #[test]
    fn roles_round_trip() {
        for role in Role::ALL {
            assert_eq!(Role::from(RoleDto::from(role)), role);
        }
    }
}
