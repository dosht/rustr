//! The TypeScript-facing contract of the signed-in app: intents in, one view
//! out. Generated into `apps/web/src/lib/core/types/` like `dto.rs`.

use serde::{Deserialize, Serialize};
use taskr_core::session::presentation::{AppIntent, AppVm, Screen, ScreenVm, SignInVm, SignedInVm};
use taskr_core::shared::access::{Role, WorkspaceId};
use taskr_core::workspaces::presentation::{MembersIntent, MembersVm};
use ts_rs::TS;

use crate::dto::{TaskIntent, TaskListView};

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MemberRole {
    Owner,
    Editor,
    Viewer,
}

impl From<MemberRole> for Role {
    fn from(r: MemberRole) -> Role {
        match r {
            MemberRole::Owner => Role::Owner,
            MemberRole::Editor => Role::Editor,
            MemberRole::Viewer => Role::Viewer,
        }
    }
}

impl From<Role> for MemberRole {
    fn from(r: Role) -> MemberRole {
        match r {
            Role::Owner => MemberRole::Owner,
            Role::Editor => MemberRole::Editor,
            Role::Viewer => MemberRole::Viewer,
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AppScreen {
    Tasks,
    Members,
}

impl From<AppScreen> for Screen {
    fn from(s: AppScreen) -> Screen {
        match s {
            AppScreen::Tasks => Screen::Tasks,
            AppScreen::Members => Screen::Members,
        }
    }
}

impl From<Screen> for AppScreen {
    fn from(s: Screen) -> AppScreen {
        match s {
            Screen::Tasks => AppScreen::Tasks,
            Screen::Members => AppScreen::Members,
        }
    }
}

#[derive(Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum MembersIntentDto {
    Opened,
    AddSubmitted { email: String, role: MemberRole },
    RoleChanged { email: String, role: MemberRole },
    RemoveRequested { email: String },
    MessageDismissed,
}

impl From<MembersIntentDto> for MembersIntent {
    fn from(i: MembersIntentDto) -> MembersIntent {
        match i {
            MembersIntentDto::Opened => MembersIntent::Opened,
            MembersIntentDto::AddSubmitted { email, role } => {
                MembersIntent::AddSubmitted { email, role: role.into() }
            }
            MembersIntentDto::RoleChanged { email, role } => {
                MembersIntent::RoleChanged { email, role: role.into() }
            }
            MembersIntentDto::RemoveRequested { email } => MembersIntent::RemoveRequested { email },
            MembersIntentDto::MessageDismissed => MembersIntent::MessageDismissed,
        }
    }
}

#[derive(Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum AppIntentDto {
    Started,
    SignInSubmitted {
        email: String,
        password: String,
    },
    SignUpSubmitted {
        email: String,
        password: String,
    },
    SignOutRequested,
    WorkspaceSelected {
        id: String,
    },
    ScreenSelected {
        screen: AppScreen,
    },
    MessageDismissed,
    /// `workspace`: the `workspace` of the signed-in view the intent came from.
    Tasks {
        workspace: String,
        intent: TaskIntent,
    },
    Members {
        workspace: String,
        intent: MembersIntentDto,
    },
}

impl From<AppIntentDto> for AppIntent {
    fn from(i: AppIntentDto) -> AppIntent {
        match i {
            AppIntentDto::Started => AppIntent::Started,
            AppIntentDto::SignInSubmitted { email, password } => {
                AppIntent::SignInSubmitted { email, password }
            }
            AppIntentDto::SignUpSubmitted { email, password } => {
                AppIntent::SignUpSubmitted { email, password }
            }
            AppIntentDto::SignOutRequested => AppIntent::SignOutRequested,
            AppIntentDto::WorkspaceSelected { id } => AppIntent::WorkspaceSelected(WorkspaceId(id)),
            AppIntentDto::ScreenSelected { screen } => AppIntent::ScreenSelected(screen.into()),
            AppIntentDto::MessageDismissed => AppIntent::MessageDismissed,
            AppIntentDto::Tasks { workspace, intent } => {
                AppIntent::Tasks(WorkspaceId(workspace), intent.into())
            }
            AppIntentDto::Members { workspace, intent } => {
                AppIntent::Members(WorkspaceId(workspace), intent.into())
            }
        }
    }
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct SignInView {
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceNavView {
    pub id: String,
    pub name: String,
    pub role_label: String,
    pub selected: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct ScreenNavView {
    pub screen: AppScreen,
    pub label: String,
    pub selected: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemberRowView {
    pub email: String,
    pub role: MemberRole,
    pub role_label: String,
    pub is_you: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct RoleOptionView {
    pub role: MemberRole,
    pub label: String,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MembersView {
    pub rows: Vec<MemberRowView>,
    pub can_manage: bool,
    pub role_options: Vec<RoleOptionView>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export)]
pub enum ScreenView {
    Loading,
    Tasks { view: TaskListView },
    Members { view: MembersView },
}

#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct SignedInView {
    pub email: String,
    pub workspaces: Vec<WorkspaceNavView>,
    pub screens: Vec<ScreenNavView>,
    /// The open workspace; send it back with `Tasks` and `Members` intents.
    pub workspace: Option<String>,
    pub title: String,
    pub error: Option<String>,
    pub screen: ScreenView,
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum AppView {
    Starting,
    SignedOut { view: SignInView },
    SignedIn { view: SignedInView },
}

impl From<MembersVm> for MembersView {
    fn from(vm: MembersVm) -> Self {
        MembersView {
            rows: vm
                .rows
                .into_iter()
                .map(|r| MemberRowView {
                    email: r.email,
                    role: r.role.into(),
                    role_label: r.role_label,
                    is_you: r.is_you,
                })
                .collect(),
            can_manage: vm.can_manage,
            role_options: vm
                .role_options
                .into_iter()
                .map(|o| RoleOptionView { role: o.role.into(), label: o.label })
                .collect(),
            is_loading: vm.is_loading,
            error: vm.error,
            notice: vm.notice,
        }
    }
}

impl From<SignInVm> for SignInView {
    fn from(vm: SignInVm) -> Self {
        SignInView { busy: vm.busy, error: vm.error, notice: vm.notice }
    }
}

impl From<SignedInVm> for SignedInView {
    fn from(vm: SignedInVm) -> Self {
        SignedInView {
            email: vm.email,
            workspaces: vm
                .workspaces
                .into_iter()
                .map(|w| WorkspaceNavView {
                    id: w.id.0,
                    name: w.name,
                    role_label: w.role_label,
                    selected: w.selected,
                })
                .collect(),
            screens: vm
                .screens
                .into_iter()
                .map(|s| ScreenNavView {
                    screen: s.screen.into(),
                    label: s.label,
                    selected: s.selected,
                })
                .collect(),
            workspace: vm.workspace.map(|w| w.0),
            title: vm.title,
            error: vm.error,
            screen: match vm.screen {
                ScreenVm::Loading => ScreenView::Loading,
                ScreenVm::Tasks(t) => ScreenView::Tasks { view: t.into() },
                ScreenVm::Members(m) => ScreenView::Members { view: m.into() },
            },
        }
    }
}

impl From<AppVm> for AppView {
    fn from(vm: AppVm) -> Self {
        match vm {
            AppVm::Starting => AppView::Starting,
            AppVm::SignedOut(v) => AppView::SignedOut { view: v.into() },
            AppVm::SignedIn(v) => AppView::SignedIn { view: v.into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskr_core::session::presentation::fixtures;

    #[test]
    fn app_view_json_is_tagged_for_typescript() {
        let json = serde_json::to_value(AppView::from(fixtures::signed_in(Role::Owner))).unwrap();
        assert_eq!(json["state"], "signedIn");
        assert_eq!(json["view"]["screen"]["kind"], "tasks");
        assert_eq!(json["view"]["screen"]["view"]["canChange"], true);
        assert_eq!(json["view"]["workspaces"][0]["roleLabel"], "Owner");
        assert_eq!(json["view"]["workspace"], json["view"]["workspaces"][0]["id"]);
        let intent: AppIntentDto = serde_json::from_value(serde_json::json!({
            "type": "Members",
            "workspace": "w1",
            "intent": {"type": "AddSubmitted", "email": "a@b.co", "role": "viewer"}
        }))
        .unwrap();
        assert!(matches!(
            AppIntent::from(intent),
            AppIntent::Members(WorkspaceId(w), MembersIntent::AddSubmitted { role: Role::Viewer, .. })
                if w == "w1"
        ));
    }
}
