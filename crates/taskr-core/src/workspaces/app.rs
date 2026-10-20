//! Use cases for workspaces and membership, and the ports they need.

use std::sync::Arc;

use crate::shared::access::{
    AccessError, Action, Actor, Email, Principal, Role, UserId, WorkspaceId,
};
use crate::shared::clock::Clock;
use crate::shared::events::DomainEvent;
use crate::shared::future::BoxFuture;
use crate::shared::storage::{MAX_ATTEMPTS, Revision, SaveError, StorageError};
use crate::shared::telemetry::{LogEvent, Telemetry};
use crate::workspaces::domain::{Workspace, WorkspaceError};

/// Named after the owner, because invited members see the same name.
pub fn personal_workspace_name(owner: &Email) -> String {
    let who = owner.as_str().split('@').next().unwrap_or("my");
    let who: String = who.chars().take(crate::workspaces::domain::MAX_NAME_CHARS - 12).collect();
    format!("{who}'s workspace")
}

pub trait WorkspaceRepository: Send + Sync {
    /// A fresh, unused id (UUIDv7 in the real adapters).
    fn new_id(&self) -> WorkspaceId;

    /// The id of the person's own workspace: the same on every call, so two
    /// racing first visits create it once (the second save is a conflict).
    fn personal_id(&self, user: &UserId) -> WorkspaceId;

    /// Every workspace the email is a member of, by name.
    fn list_for<'a>(
        &'a self,
        email: &'a Email,
    ) -> BoxFuture<'a, Result<Vec<WorkspaceSummary>, StorageError>>;

    fn load<'a>(
        &'a self,
        id: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Option<(Workspace, Revision)>, StorageError>>;

    /// `Revision::NEW` creates the workspace. Same atomicity rules as `TaskListRepository::save`.
    fn save<'a>(
        &'a self,
        workspace: &'a Workspace,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSummary {
    pub email: String,
    pub role: Role,
}

/// The signed-in person and where they can go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    pub email: String,
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspacesError {
    Rejected(WorkspaceError),
    Access(AccessError),
    Storage(StorageError),
}

impl From<WorkspaceError> for WorkspacesError {
    fn from(e: WorkspaceError) -> Self {
        WorkspacesError::Rejected(e)
    }
}

impl From<AccessError> for WorkspacesError {
    fn from(e: AccessError) -> Self {
        WorkspacesError::Access(e)
    }
}

impl From<StorageError> for WorkspacesError {
    fn from(e: StorageError) -> Self {
        WorkspacesError::Storage(e)
    }
}

/// The workspaces API as a signed-in client sees it. Implemented in process by
/// the server and over HTTP by `taskr-client`.
pub trait WorkspacesApi: Send + Sync {
    fn me(&self) -> BoxFuture<'_, Result<Me, WorkspacesError>>;

    fn members<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<MemberSummary>, WorkspacesError>>;

    fn set_member<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        email: String,
        role: Role,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>>;

    fn remove_member<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        email: String,
    ) -> BoxFuture<'a, Result<(), WorkspacesError>>;
}

#[derive(Clone)]
pub struct Workspaces {
    repo: Arc<dyn WorkspaceRepository>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<dyn Telemetry>,
}

impl Workspaces {
    pub fn new(
        repo: Arc<dyn WorkspaceRepository>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<dyn Telemetry>,
    ) -> Self {
        Self { repo, clock, telemetry }
    }

    /// Everything the person can open. Anyone who owns no workspace yet gets
    /// a personal one, even if they were invited somewhere first, so they can
    /// always keep their own tasks. That is the whole onboarding for now.
    pub async fn me(&self, principal: &Principal) -> Result<Me, WorkspacesError> {
        let mut workspaces = self.repo.list_for(&principal.email).await?;
        if !workspaces.iter().any(|w| w.role == Role::Owner) {
            let ws = Workspace::create(
                self.repo.personal_id(&principal.user_id),
                &personal_workspace_name(&principal.email),
                principal.email.clone(),
            )?;
            let event =
                self.event("workspace.created", principal, ws.id()).field("name", ws.name());
            match self.repo.save(&ws, Revision::NEW, &[event]).await {
                Ok(()) => self
                    .telemetry
                    .record(LogEvent::info("workspace.created").field("workspace", ws.id())),
                // Another request for the same person created it first.
                Err(SaveError::Conflict) => {}
                Err(SaveError::Failed(e)) => return Err(e.into()),
            }
            workspaces = self.repo.list_for(&principal.email).await?;
        }
        Ok(Me { email: principal.email.as_str().to_string(), workspaces })
    }

    /// Resolves what the person may do in a workspace. Unknown workspaces and
    /// workspaces they are not in look the same from outside.
    pub async fn actor(
        &self,
        principal: &Principal,
        workspace: &WorkspaceId,
    ) -> Result<Actor, WorkspacesError> {
        let (ws, _) = self.repo.load(workspace).await?.ok_or(AccessError::NotAMember)?;
        let role = ws.role_of(&principal.email).ok_or(AccessError::NotAMember)?;
        Ok(Actor { principal: principal.clone(), workspace: workspace.clone(), role })
    }

    pub async fn members(&self, actor: &Actor) -> Result<Vec<MemberSummary>, WorkspacesError> {
        actor.ensure(Action::ViewMembers)?;
        let (ws, _) = self.repo.load(&actor.workspace).await?.ok_or(AccessError::NotAMember)?;
        let mut members: Vec<MemberSummary> = ws
            .members()
            .iter()
            .map(|m| MemberSummary { email: m.email.as_str().to_string(), role: m.role })
            .collect();
        members.sort_by(|a, b| b.role.cmp(&a.role).then_with(|| a.email.cmp(&b.email)));
        Ok(members)
    }

    pub async fn set_member(
        &self,
        actor: &Actor,
        raw_email: &str,
        role: Role,
    ) -> Result<(), WorkspacesError> {
        let email = Email::parse(raw_email).map_err(|_| WorkspaceError::InvalidEmail)?;
        self.change(
            actor,
            "member.set",
            |ws| ws.set_member(email.clone(), role),
            |e| e.field("email", &email).field("role", role.as_str()),
        )
        .await
    }

    pub async fn remove_member(
        &self,
        actor: &Actor,
        raw_email: &str,
    ) -> Result<(), WorkspacesError> {
        let email = Email::parse(raw_email).map_err(|_| WorkspaceError::InvalidEmail)?;
        self.change(
            actor,
            "member.removed",
            |ws| ws.remove_member(&email),
            |e| e.field("email", &email),
        )
        .await
    }

    /// Membership is checked against the workspace being saved, on every
    /// attempt, not against the role resolved when the request started: the
    /// save only lands on the revision that was checked, so a member removed
    /// or demoted meanwhile cannot slip a change in.
    async fn change(
        &self,
        actor: &Actor,
        name: &'static str,
        apply: impl Fn(&mut Workspace) -> Result<(), WorkspaceError> + Send,
        describe: impl Fn(DomainEvent) -> DomainEvent + Send,
    ) -> Result<(), WorkspacesError> {
        actor.ensure(Action::ManageMembers)?;
        for _ in 0..MAX_ATTEMPTS {
            let (mut ws, revision) =
                self.repo.load(&actor.workspace).await?.ok_or(AccessError::NotAMember)?;
            let role = ws.role_of(&actor.principal.email).ok_or(AccessError::NotAMember)?;
            Actor { role, ..actor.clone() }.ensure(Action::ManageMembers)?;
            apply(&mut ws)?;
            let event = describe(self.event(name, &actor.principal, &actor.workspace));
            match self.repo.save(&ws, revision, &[event]).await {
                Ok(()) => {
                    self.telemetry
                        .record(LogEvent::info(name).field("workspace", &actor.workspace));
                    return Ok(());
                }
                Err(SaveError::Conflict) => continue,
                Err(SaveError::Failed(e)) => return Err(e.into()),
            }
        }
        Err(StorageError("too many people are editing at once".into()).into())
    }

    fn event(&self, name: &'static str, by: &Principal, ws: &WorkspaceId) -> DomainEvent {
        DomainEvent {
            name,
            workspace: ws.clone(),
            actor: by.user_id.clone(),
            at: self.clock.now(),
            fields: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::access::UserId;
    use crate::shared::telemetry::NoTelemetry;
    use crate::testing::{FixedClock, InMemoryWorkspaceRepository, block_on};

    fn principal(email: &str) -> Principal {
        Principal { user_id: UserId(format!("u-{email}")), email: Email::parse(email).unwrap() }
    }

    fn world() -> (Workspaces, Arc<InMemoryWorkspaceRepository>) {
        let repo = Arc::new(InMemoryWorkspaceRepository::default());
        let ws = Workspaces::new(repo.clone(), Arc::new(FixedClock::at(7)), Arc::new(NoTelemetry));
        (ws, repo)
    }

    #[test]
    fn first_visit_creates_a_personal_workspace_once() {
        let (w, repo) = world();
        let ada = principal("ada@example.com");
        let me = block_on(w.me(&ada)).unwrap();
        assert_eq!(me.email, "ada@example.com");
        assert_eq!(me.workspaces.len(), 1);
        assert_eq!(me.workspaces[0].name, "ada's workspace");
        assert_eq!(me.workspaces[0].role, Role::Owner);
        assert_eq!(block_on(w.me(&ada)).unwrap(), me);
        assert_eq!(repo.outbox().iter().map(|e| e.name).collect::<Vec<_>>(), ["workspace.created"]);
    }

    #[test]
    fn owners_share_a_workspace_and_roles_apply() {
        let (w, _) = world();
        let ada = principal("ada@example.com");
        let bob = principal("bob@example.com");
        let ws = block_on(w.me(&ada)).unwrap().workspaces[0].id.clone();

        assert_eq!(
            block_on(w.actor(&bob, &ws)),
            Err(WorkspacesError::Access(AccessError::NotAMember))
        );
        let owner = block_on(w.actor(&ada, &ws)).unwrap();
        block_on(w.set_member(&owner, " BOB@example.com", Role::Viewer)).unwrap();

        let bob_actor = block_on(w.actor(&bob, &ws)).unwrap();
        assert_eq!(bob_actor.role, Role::Viewer);
        assert_eq!(
            block_on(w.set_member(&bob_actor, "eve@example.com", Role::Owner)),
            Err(WorkspacesError::Access(AccessError::Forbidden(Action::ManageMembers)))
        );
        let emails: Vec<_> =
            block_on(w.members(&bob_actor)).unwrap().into_iter().map(|m| m.email).collect();
        assert_eq!(emails, ["ada@example.com", "bob@example.com"]);

        let me = block_on(w.me(&bob)).unwrap();
        let roles: Vec<_> = me.workspaces.iter().map(|w| (w.name.as_str(), w.role)).collect();
        assert_eq!(
            roles,
            [("ada's workspace", Role::Viewer), ("bob's workspace", Role::Owner)],
            "an invited person still gets a workspace of their own"
        );
    }

    #[test]
    fn personal_names_fit_the_name_rule() {
        let long = Email::parse(&format!("{}@example.com", "x".repeat(64))).unwrap();
        let name = personal_workspace_name(&long);
        assert!(Workspace::create(WorkspaceId("w".into()), &name, long).is_ok(), "{name}");
    }

    #[test]
    fn unknown_workspaces_look_like_foreign_ones() {
        let (w, _) = world();
        assert_eq!(
            block_on(w.actor(&principal("a@b.co"), &WorkspaceId("nope".into()))),
            Err(WorkspacesError::Access(AccessError::NotAMember))
        );
    }

    #[test]
    fn invalid_email_and_last_owner_are_rejected() {
        let (w, _) = world();
        let ada = principal("ada@example.com");
        let ws = block_on(w.me(&ada)).unwrap().workspaces[0].id.clone();
        let owner = block_on(w.actor(&ada, &ws)).unwrap();
        assert_eq!(
            block_on(w.set_member(&owner, "not-an-email", Role::Viewer)),
            Err(WorkspacesError::Rejected(WorkspaceError::InvalidEmail))
        );
        assert_eq!(
            block_on(w.remove_member(&owner, "ada@example.com")),
            Err(WorkspacesError::Rejected(WorkspaceError::LastOwner))
        );
    }

    #[test]
    fn an_owner_removed_mid_request_cannot_restore_themselves() {
        let (w, _) = world();
        let (ada, bob) = (principal("ada@example.com"), principal("bob@example.com"));
        let ws = block_on(w.me(&ada)).unwrap().workspaces[0].id.clone();
        let owner = block_on(w.actor(&ada, &ws)).unwrap();
        block_on(w.set_member(&owner, "bob@example.com", Role::Owner)).unwrap();

        let bob_in_flight = block_on(w.actor(&bob, &ws)).unwrap();
        block_on(w.remove_member(&owner, "bob@example.com")).unwrap();
        assert_eq!(
            block_on(w.set_member(&bob_in_flight, "bob@example.com", Role::Owner)),
            Err(WorkspacesError::Access(AccessError::NotAMember))
        );

        let bob_demoted = {
            block_on(w.set_member(&owner, "bob@example.com", Role::Owner)).unwrap();
            let actor = block_on(w.actor(&bob, &ws)).unwrap();
            block_on(w.set_member(&owner, "bob@example.com", Role::Viewer)).unwrap();
            actor
        };
        assert_eq!(
            block_on(w.set_member(&bob_demoted, "eve@example.com", Role::Owner)),
            Err(WorkspacesError::Access(AccessError::Forbidden(Action::ManageMembers)))
        );
    }

    /// Answers the first `list_for` from before the other request created
    /// the workspace, like a second tab racing the first one.
    struct StaleFirstList {
        inner: Arc<InMemoryWorkspaceRepository>,
        stale: std::sync::atomic::AtomicBool,
    }

    impl WorkspaceRepository for StaleFirstList {
        fn new_id(&self) -> WorkspaceId {
            self.inner.new_id()
        }
        fn personal_id(&self, user: &UserId) -> WorkspaceId {
            self.inner.personal_id(user)
        }
        fn list_for<'a>(
            &'a self,
            email: &'a Email,
        ) -> BoxFuture<'a, Result<Vec<WorkspaceSummary>, StorageError>> {
            if self.stale.swap(false, std::sync::atomic::Ordering::SeqCst) {
                return Box::pin(async { Ok(vec![]) });
            }
            self.inner.list_for(email)
        }
        fn load<'a>(
            &'a self,
            id: &'a WorkspaceId,
        ) -> BoxFuture<'a, Result<Option<(Workspace, Revision)>, StorageError>> {
            self.inner.load(id)
        }
        fn save<'a>(
            &'a self,
            workspace: &'a Workspace,
            expected: Revision,
            events: &'a [DomainEvent],
        ) -> BoxFuture<'a, Result<(), SaveError>> {
            self.inner.save(workspace, expected, events)
        }
    }

    #[test]
    fn racing_first_visits_share_one_personal_workspace() {
        let inner = Arc::new(InMemoryWorkspaceRepository::default());
        let repo = Arc::new(StaleFirstList { inner: inner.clone(), stale: false.into() });
        let w = Workspaces::new(repo.clone(), Arc::new(FixedClock::at(7)), Arc::new(NoTelemetry));
        let ada = principal("ada@example.com");
        let first = block_on(w.me(&ada)).unwrap();

        repo.stale.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(block_on(w.me(&ada)).unwrap(), first);
        assert_eq!(first.workspaces.len(), 1);
        assert_eq!(inner.outbox().len(), 1, "one workspace.created");
    }
}
