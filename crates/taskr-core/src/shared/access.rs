//! Who is acting, in which workspace, with which role, and what that role may
//! do. `can` is the whole RBAC policy: pure, tested once, enforced by the use
//! cases and read by the presentation layer to hide what a role cannot do.

use std::fmt;

use crate::shared::future::BoxFuture;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceId(pub String);

impl WorkspaceId {
    /// The single workspace of the local, signed-out shells (CLI, TUI, mobile).
    pub fn local() -> WorkspaceId {
        WorkspaceId("local".into())
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub const MAX_EMAIL_CHARS: usize = 254;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Email(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidEmail;

impl Email {
    /// Trimmed and lowercased, so membership lookups never miss on case.
    pub fn parse(raw: &str) -> Result<Email, InvalidEmail> {
        let email = raw.trim().to_lowercase();
        let valid = email.chars().count() <= MAX_EMAIL_CHARS
            && !email.chars().any(|c| c.is_whitespace() || c.is_control())
            && matches!(email.split_once('@'), Some((user, domain))
                if !user.is_empty() && domain.contains('.') && !domain.contains('@')
                    && !domain.starts_with('.') && !domain.ends_with('.'));
        if valid { Ok(Email(email)) } else { Err(InvalidEmail) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A signed-in person, as the identity provider vouches for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub user_id: UserId,
    pub email: Email,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Viewer,
    Editor,
    Owner,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Owner, Role::Editor, Role::Viewer];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Viewer => "Viewer",
            Role::Editor => "Editor",
            Role::Owner => "Owner",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ViewTasks,
    ChangeTasks,
    ViewMembers,
    ManageMembers,
}

pub fn can(role: Role, action: Action) -> bool {
    match action {
        Action::ViewTasks | Action::ViewMembers => true,
        Action::ChangeTasks => role >= Role::Editor,
        Action::ManageMembers => role == Role::Owner,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub principal: Principal,
    pub workspace: WorkspaceId,
    pub role: Role,
}

impl Actor {
    /// The owner of the local workspace, for shells that run without sign-in.
    pub fn local_owner() -> Actor {
        Actor {
            principal: Principal {
                user_id: UserId("local".into()),
                email: Email("me@localhost.local".into()),
            },
            workspace: WorkspaceId::local(),
            role: Role::Owner,
        }
    }

    pub fn ensure(&self, action: Action) -> Result<(), AccessError> {
        if can(self.role, action) { Ok(()) } else { Err(AccessError::Forbidden(action)) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// No valid session. The shell should ask the person to sign in again.
    Unauthenticated,
    /// Also returned for workspaces that do not exist, so ids leak nothing.
    NotAMember,
    Forbidden(Action),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    Invalid(String),
    Unavailable(String),
}

/// Server-side port: turns a bearer token into a principal (Supabase JWT, dev tokens).
pub trait Identity: Send + Sync {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>>;
}

/// Shared by every context, so a missing permission reads the same everywhere.
pub fn describe_access(e: &AccessError) -> String {
    match e {
        AccessError::Unauthenticated => "Your session has ended. Sign in again.".into(),
        AccessError::NotAMember => "You are not a member of this workspace.".into(),
        AccessError::Forbidden(Action::ChangeTasks) => "Viewers cannot change tasks.".into(),
        AccessError::Forbidden(Action::ManageMembers) => "Only owners can manage members.".into(),
        AccessError::Forbidden(_) => "You do not have access to this.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_table() {
        use Action::*;
        let table = [
            (Role::Owner, [true, true, true, true]),
            (Role::Editor, [true, true, true, false]),
            (Role::Viewer, [true, false, true, false]),
        ];
        for (role, expected) in table {
            let actual = [ViewTasks, ChangeTasks, ViewMembers, ManageMembers].map(|a| can(role, a));
            assert_eq!(actual, expected, "{role:?}");
        }
    }

    #[test]
    fn email_is_normalized_and_validated() {
        assert_eq!(Email::parse("  Ada@Example.COM ").unwrap().as_str(), "ada@example.com");
        for bad in [
            "",
            "ada",
            "@example.com",
            "ada@",
            "ada@example",
            "a da@example.com",
            "a@b@c.com",
            "ada@.com",
        ] {
            assert_eq!(Email::parse(bad), Err(InvalidEmail), "{bad:?}");
        }
    }

    #[test]
    fn roles_round_trip_through_strings() {
        for role in Role::ALL {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("admin"), None);
    }

    #[test]
    fn actor_ensure_reports_the_forbidden_action() {
        let mut actor = Actor::local_owner();
        assert_eq!(actor.ensure(Action::ManageMembers), Ok(()));
        actor.role = Role::Viewer;
        assert_eq!(
            actor.ensure(Action::ChangeTasks),
            Err(AccessError::Forbidden(Action::ChangeTasks))
        );
    }
}
