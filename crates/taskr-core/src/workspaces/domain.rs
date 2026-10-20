use crate::shared::access::{Email, Role, WorkspaceId};

pub const MAX_NAME_CHARS: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub email: Email,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    InvalidName,
    InvalidEmail,
    /// A workspace always keeps at least one owner.
    LastOwner,
    NotAMember(Email),
}

/// The aggregate root. Members live inside it because "at least one owner"
/// spans members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    members: Vec<Member>,
}

impl Workspace {
    pub fn create(id: WorkspaceId, name: &str, owner: Email) -> Result<Workspace, WorkspaceError> {
        Ok(Workspace {
            id,
            name: parse_name(name)?,
            members: vec![Member { email: owner, role: Role::Owner }],
        })
    }

    pub fn id(&self) -> &WorkspaceId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn members(&self) -> &[Member] {
        &self.members
    }

    pub fn role_of(&self, email: &Email) -> Option<Role> {
        self.members.iter().find(|m| &m.email == email).map(|m| m.role)
    }

    /// Adds the person, or changes their role if they are already a member.
    pub fn set_member(&mut self, email: Email, role: Role) -> Result<(), WorkspaceError> {
        match self.members.iter().position(|m| m.email == email) {
            Some(i) => {
                if role != Role::Owner && self.is_last_owner(&email) {
                    return Err(WorkspaceError::LastOwner);
                }
                self.members[i].role = role;
            }
            None => self.members.push(Member { email, role }),
        }
        Ok(())
    }

    pub fn remove_member(&mut self, email: &Email) -> Result<(), WorkspaceError> {
        let i = self
            .members
            .iter()
            .position(|m| &m.email == email)
            .ok_or_else(|| WorkspaceError::NotAMember(email.clone()))?;
        if self.is_last_owner(email) {
            return Err(WorkspaceError::LastOwner);
        }
        self.members.remove(i);
        Ok(())
    }

    fn is_last_owner(&self, email: &Email) -> bool {
        let owners: Vec<_> = self.members.iter().filter(|m| m.role == Role::Owner).collect();
        owners.len() == 1 && &owners[0].email == email
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            id: self.id.clone(),
            name: self.name.clone(),
            members: self
                .members
                .iter()
                .map(|m| MemberSnapshot { email: m.email.as_str().to_string(), role: m.role })
                .collect(),
        }
    }

    /// Rebuilds the aggregate from stored data, re-checking every invariant.
    pub fn restore(s: WorkspaceSnapshot) -> Result<Workspace, String> {
        let name = parse_name(&s.name).map_err(|_| format!("workspace {}: invalid name", s.id))?;
        let mut members: Vec<Member> = Vec::with_capacity(s.members.len());
        for m in s.members {
            let email = Email::parse(&m.email)
                .map_err(|_| format!("workspace {}: invalid member email", s.id))?;
            if members.iter().any(|x| x.email == email) {
                return Err(format!("workspace {}: duplicate member {email}", s.id));
            }
            members.push(Member { email, role: m.role });
        }
        if !members.iter().any(|m| m.role == Role::Owner) {
            return Err(format!("workspace {}: no owner", s.id));
        }
        Ok(Workspace { id: s.id, name, members })
    }
}

fn parse_name(raw: &str) -> Result<String, WorkspaceError> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(WorkspaceError::InvalidName);
    }
    Ok(name.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub id: WorkspaceId,
    pub name: String,
    pub members: Vec<MemberSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSnapshot {
    pub email: String,
    pub role: Role,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(s: &str) -> Email {
        Email::parse(s).unwrap()
    }

    fn ws() -> Workspace {
        Workspace::create(WorkspaceId("w".into()), " Team ", email("ada@example.com")).unwrap()
    }

    #[test]
    fn the_creator_is_the_owner() {
        let w = ws();
        assert_eq!(w.name(), "Team");
        assert_eq!(w.role_of(&email("ada@example.com")), Some(Role::Owner));
        assert_eq!(w.role_of(&email("bob@example.com")), None);
    }

    #[test]
    fn names_are_validated() {
        let id = WorkspaceId("w".into());
        for bad in ["", "  ", &"x".repeat(MAX_NAME_CHARS + 1), "a\nb"] {
            assert_eq!(
                Workspace::create(id.clone(), bad, email("a@b.co")),
                Err(WorkspaceError::InvalidName)
            );
        }
    }

    #[test]
    fn set_member_adds_then_changes_role() {
        let mut w = ws();
        w.set_member(email("bob@example.com"), Role::Viewer).unwrap();
        w.set_member(email("bob@example.com"), Role::Editor).unwrap();
        assert_eq!(w.members().len(), 2);
        assert_eq!(w.role_of(&email("bob@example.com")), Some(Role::Editor));
    }

    #[test]
    fn the_last_owner_cannot_leave_or_step_down() {
        let mut w = ws();
        let ada = email("ada@example.com");
        assert_eq!(w.set_member(ada.clone(), Role::Editor), Err(WorkspaceError::LastOwner));
        assert_eq!(w.remove_member(&ada), Err(WorkspaceError::LastOwner));

        w.set_member(email("bob@example.com"), Role::Owner).unwrap();
        assert_eq!(w.set_member(ada.clone(), Role::Editor), Ok(()));
        assert_eq!(w.remove_member(&ada), Ok(()));
    }

    #[test]
    fn removing_a_stranger_is_rejected() {
        let mut w = ws();
        let bob = email("bob@example.com");
        assert_eq!(w.remove_member(&bob), Err(WorkspaceError::NotAMember(bob)));
    }

    #[test]
    fn snapshot_round_trips_and_restore_checks_invariants() {
        let mut w = ws();
        w.set_member(email("bob@example.com"), Role::Viewer).unwrap();
        assert_eq!(Workspace::restore(w.snapshot()), Ok(w.clone()));

        let mut no_owner = w.snapshot();
        no_owner.members.retain(|m| m.role != Role::Owner);
        assert!(Workspace::restore(no_owner).is_err());

        let mut dup = w.snapshot();
        dup.members.push(MemberSnapshot { email: "BOB@example.com".into(), role: Role::Owner });
        assert!(Workspace::restore(dup).is_err());
    }
}
