//! The shared ViewModel for the members screen. Same shape as the task list:
//! a pure `MembersState`, effects run by whoever owns it (the session
//! controller), and a `MembersVm` that every shell renders as is.

use crate::shared::access::{AccessError, Action, Role, can, describe_access};
use crate::workspaces::app::{MemberSummary, WorkspacesError};
use crate::workspaces::domain::WorkspaceError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MembersIntent {
    Opened,
    AddSubmitted { email: String, role: Role },
    RoleChanged { email: String, role: Role },
    RemoveRequested { email: String },
    MessageDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MembersEvent {
    User(MembersIntent),
    Loaded(Result<Vec<MemberSummary>, WorkspacesError>),
    Changed(MemberChange, Result<(), WorkspacesError>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberChange {
    Set { email: String, role: Role },
    Removed { email: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MembersEffect {
    Load,
    Set { email: String, role: Role },
    Remove { email: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleOption {
    pub role: Role,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRow {
    pub email: String,
    pub role: Role,
    pub role_label: String,
    pub is_you: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembersVm {
    pub rows: Vec<MemberRow>,
    /// Owners see the add form, the role pickers and the remove buttons.
    pub can_manage: bool,
    pub role_options: Vec<RoleOption>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembersState {
    me: String,
    role: Role,
    members: Vec<MemberSummary>,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
}

impl MembersState {
    pub fn new(me: &str, role: Role) -> Self {
        Self {
            me: me.to_string(),
            role,
            members: vec![],
            loading: false,
            error: None,
            notice: None,
        }
    }

    /// The role changed while the screen was open.
    pub fn set_role(&mut self, role: Role) {
        self.role = role;
    }

    pub fn update(&mut self, event: MembersEvent) -> Vec<MembersEffect> {
        match event {
            MembersEvent::User(intent) => self.on_intent(intent),
            MembersEvent::Loaded(Ok(members)) => {
                self.loading = false;
                self.members = members;
                vec![]
            }
            MembersEvent::Loaded(Err(e)) => {
                self.loading = false;
                self.show_error(&e);
                vec![]
            }
            MembersEvent::Changed(change, Ok(())) => {
                self.error = None;
                self.notice = Some(match change {
                    MemberChange::Set { email, role } => format!("{email} is now {}", role.label()),
                    MemberChange::Removed { email } => format!("Removed {email}"),
                });
                self.loading = true;
                vec![MembersEffect::Load]
            }
            MembersEvent::Changed(_, Err(e)) => {
                self.show_error(&e);
                vec![]
            }
        }
    }

    fn on_intent(&mut self, intent: MembersIntent) -> Vec<MembersEffect> {
        let manages = can(self.role, Action::ManageMembers);
        match intent {
            MembersIntent::Opened => {
                self.loading = true;
                vec![MembersEffect::Load]
            }
            MembersIntent::AddSubmitted { email, role }
            | MembersIntent::RoleChanged { email, role }
                if manages =>
            {
                vec![MembersEffect::Set { email, role }]
            }
            MembersIntent::RemoveRequested { email } if manages => {
                vec![MembersEffect::Remove { email }]
            }
            MembersIntent::AddSubmitted { .. }
            | MembersIntent::RoleChanged { .. }
            | MembersIntent::RemoveRequested { .. } => {
                self.show_error(&AccessError::Forbidden(Action::ManageMembers).into());
                vec![]
            }
            MembersIntent::MessageDismissed => {
                self.error = None;
                self.notice = None;
                vec![]
            }
        }
    }

    fn show_error(&mut self, e: &WorkspacesError) {
        self.notice = None;
        self.error = Some(describe(e));
    }

    pub fn view(&self) -> MembersVm {
        MembersVm {
            rows: self
                .members
                .iter()
                .map(|m| MemberRow {
                    email: m.email.clone(),
                    role: m.role,
                    role_label: m.role.label().to_string(),
                    is_you: m.email == self.me,
                })
                .collect(),
            can_manage: can(self.role, Action::ManageMembers),
            role_options: Role::ALL
                .into_iter()
                .map(|role| RoleOption { role, label: role.label().to_string() })
                .collect(),
            is_loading: self.loading,
            error: self.error.clone(),
            notice: self.notice.clone(),
        }
    }
}

pub fn describe(e: &WorkspacesError) -> String {
    match e {
        WorkspacesError::Storage(s) => format!("Could not load or save the workspace ({}).", s.0),
        WorkspacesError::Access(a) => describe_access(a),
        WorkspacesError::Rejected(r) => match r {
            WorkspaceError::InvalidName => "Workspace names need 1 to 60 characters.".into(),
            WorkspaceError::InvalidEmail => "That does not look like an email address.".into(),
            WorkspaceError::LastOwner => "A workspace needs at least one owner.".into(),
            WorkspaceError::NotAMember(email) => format!("{email} is not a member."),
        },
    }
}

/// Named screens for previews and snapshot tests.
pub mod fixtures {
    use super::*;

    pub fn team(role: Role) -> MembersVm {
        let mut s = MembersState::new("ada@example.com", role);
        s.update(MembersEvent::Loaded(Ok(vec![
            MemberSummary { email: "ada@example.com".into(), role: Role::Owner },
            MemberSummary { email: "bob@example.com".into(), role: Role::Editor },
            MemberSummary { email: "eve@example.com".into(), role: Role::Viewer },
        ])));
        s.view()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(role: Role) -> MembersState {
        let mut s = MembersState::new("ada@example.com", role);
        s.update(MembersEvent::Loaded(Ok(vec![
            MemberSummary { email: "ada@example.com".into(), role },
            MemberSummary { email: "bob@example.com".into(), role: Role::Viewer },
        ])));
        s
    }

    #[test]
    fn opening_loads_members() {
        let mut s = MembersState::new("a@b.co", Role::Owner);
        assert_eq!(s.update(MembersEvent::User(MembersIntent::Opened)), vec![MembersEffect::Load]);
        assert!(s.view().is_loading);
    }

    #[test]
    fn owners_can_add_change_and_remove() {
        let mut s = loaded(Role::Owner);
        let vm = s.view();
        assert!(vm.can_manage);
        assert!(vm.rows[0].is_you && !vm.rows[1].is_you);
        assert_eq!(
            s.update(MembersEvent::User(MembersIntent::AddSubmitted {
                email: "eve@example.com".into(),
                role: Role::Editor
            })),
            vec![MembersEffect::Set { email: "eve@example.com".into(), role: Role::Editor }]
        );
        assert_eq!(
            s.update(MembersEvent::User(MembersIntent::RemoveRequested {
                email: "bob@example.com".into()
            })),
            vec![MembersEffect::Remove { email: "bob@example.com".into() }]
        );
    }

    #[test]
    fn non_owners_get_an_explanation_instead_of_an_effect() {
        let mut s = loaded(Role::Editor);
        assert!(!s.view().can_manage);
        let effects = s.update(MembersEvent::User(MembersIntent::RemoveRequested {
            email: "bob@example.com".into(),
        }));
        assert!(effects.is_empty());
        assert_eq!(s.view().error.as_deref(), Some("Only owners can manage members."));
    }

    #[test]
    fn a_successful_change_reloads_and_says_what_happened() {
        let mut s = loaded(Role::Owner);
        let change = MemberChange::Set { email: "bob@example.com".into(), role: Role::Editor };
        assert_eq!(s.update(MembersEvent::Changed(change, Ok(()))), vec![MembersEffect::Load]);
        assert_eq!(s.view().notice.as_deref(), Some("bob@example.com is now Editor"));
    }

    #[test]
    fn rejections_are_described() {
        let mut s = loaded(Role::Owner);
        let change = MemberChange::Removed { email: "ada@example.com".into() };
        s.update(MembersEvent::Changed(change, Err(WorkspaceError::LastOwner.into())));
        assert_eq!(s.view().error.as_deref(), Some("A workspace needs at least one owner."));
    }
}
