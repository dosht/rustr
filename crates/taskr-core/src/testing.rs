//! Hand-written fakes for every port, plus contract tests that real adapters
//! must also pass. The contract is what keeps the fakes honest.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

use crate::session::app::{AuthClient, AuthError, Session, SessionStore, SignUp};
use crate::shared::access::{Email, Identity, IdentityError, Principal, Role, UserId, WorkspaceId};
use crate::shared::clock::{Clock, Millis};
use crate::shared::events::DomainEvent;
use crate::shared::future::BoxFuture;
pub use crate::shared::future::block_on;
use crate::shared::storage::{Revision, SaveError, StorageError};
use crate::shared::telemetry::{LogEvent, Telemetry};
use crate::tasks::app::TaskListRepository;
use crate::tasks::domain::{TaskList, TaskListSnapshot, Title};
use crate::workspaces::app::{WorkspaceRepository, WorkspaceSummary};
use crate::workspaces::domain::{Workspace, WorkspaceSnapshot};

pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn at(now: Millis) -> Self {
        Self(AtomicI64::new(now))
    }

    pub fn set(&self, now: Millis) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}

/// Stores snapshots rather than aggregates so every load goes through
/// `restore`, exactly like the real adapters.
#[derive(Default)]
pub struct InMemoryTaskListRepository {
    stored: Mutex<BTreeMap<WorkspaceId, (TaskListSnapshot, Revision)>>,
    outbox: Mutex<Vec<DomainEvent>>,
    fail_saves: AtomicBool,
    conflicts_left: AtomicUsize,
    saves: AtomicUsize,
}

impl InMemoryTaskListRepository {
    pub fn fail_saves(&self, fail: bool) {
        self.fail_saves.store(fail, Ordering::SeqCst);
    }

    /// The next `n` saves fail as if someone else had saved first.
    pub fn conflict_next_saves(&self, n: usize) {
        self.conflicts_left.store(n, Ordering::SeqCst);
    }

    pub fn save_count(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }

    pub fn outbox(&self) -> Vec<DomainEvent> {
        self.outbox.lock().unwrap().clone()
    }
}

impl TaskListRepository for InMemoryTaskListRepository {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
        let stored = self.stored.lock().unwrap().get(workspace).cloned();
        Box::pin(async move {
            match stored {
                None => Ok((TaskList::new(), Revision::NEW)),
                Some((snapshot, rev)) => {
                    Ok((TaskList::restore(snapshot).map_err(|e| StorageError(e.0))?, rev))
                }
            }
        })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        list: &'a TaskList,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            if self.fail_saves.load(Ordering::SeqCst) {
                return Err(StorageError("disk is full".into()).into());
            }
            if self.conflicts_left.load(Ordering::SeqCst) > 0 {
                self.conflicts_left.fetch_sub(1, Ordering::SeqCst);
                return Err(SaveError::Conflict);
            }
            let mut stored = self.stored.lock().unwrap();
            let current = stored.get(workspace).map(|(_, r)| *r).unwrap_or(Revision::NEW);
            if current != expected {
                return Err(SaveError::Conflict);
            }
            stored.insert(workspace.clone(), (list.snapshot(), expected.next()));
            self.outbox.lock().unwrap().extend_from_slice(events);
            self.saves.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[derive(Default)]
pub struct InMemoryWorkspaceRepository {
    stored: Mutex<BTreeMap<WorkspaceId, (WorkspaceSnapshot, Revision)>>,
    outbox: Mutex<Vec<DomainEvent>>,
    next_id: AtomicUsize,
}

impl InMemoryWorkspaceRepository {
    pub fn outbox(&self) -> Vec<DomainEvent> {
        self.outbox.lock().unwrap().clone()
    }
}

impl WorkspaceRepository for InMemoryWorkspaceRepository {
    fn new_id(&self) -> WorkspaceId {
        WorkspaceId(format!("ws-{}", self.next_id.fetch_add(1, Ordering::SeqCst) + 1))
    }

    fn personal_id(&self, user: &UserId) -> WorkspaceId {
        WorkspaceId(format!("personal-{}", user.0))
    }

    fn list_for<'a>(
        &'a self,
        email: &'a Email,
    ) -> BoxFuture<'a, Result<Vec<WorkspaceSummary>, StorageError>> {
        let mut found: Vec<WorkspaceSummary> = self
            .stored
            .lock()
            .unwrap()
            .values()
            .filter_map(|(s, _)| {
                let m = s.members.iter().find(|m| m.email == email.as_str())?;
                Some(WorkspaceSummary { id: s.id.clone(), name: s.name.clone(), role: m.role })
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Box::pin(async move { Ok(found) })
    }

    fn load<'a>(
        &'a self,
        id: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Option<(Workspace, Revision)>, StorageError>> {
        let stored = self.stored.lock().unwrap().get(id).cloned();
        Box::pin(async move {
            stored
                .map(|(s, rev)| Workspace::restore(s).map(|w| (w, rev)).map_err(StorageError))
                .transpose()
        })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a Workspace,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            let mut stored = self.stored.lock().unwrap();
            let current = stored.get(workspace.id()).map(|(_, r)| *r).unwrap_or(Revision::NEW);
            if current != expected {
                return Err(SaveError::Conflict);
            }
            stored.insert(workspace.id().clone(), (workspace.snapshot(), expected.next()));
            self.outbox.lock().unwrap().extend_from_slice(events);
            Ok(())
        })
    }
}

#[derive(Default)]
pub struct RecordingTelemetry(Mutex<Vec<LogEvent>>);

impl RecordingTelemetry {
    pub fn events(&self) -> Vec<LogEvent> {
        self.0.lock().unwrap().clone()
    }
}

impl Telemetry for RecordingTelemetry {
    fn record(&self, event: LogEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// Signs anyone in with a `test:<email>` token, which `FakeIdentity` accepts.
pub struct FakeAuth;

fn fake_session(email: &str) -> Session {
    Session {
        access_token: format!("test:{}", email.trim().to_lowercase()),
        refresh_token: None,
        email: email.trim().to_lowercase(),
        expires_at: None,
    }
}

impl AuthClient for FakeAuth {
    fn sign_in<'a>(
        &'a self,
        email: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Session, AuthError>> {
        Box::pin(async move { Ok(fake_session(email)) })
    }

    fn sign_up<'a>(
        &'a self,
        email: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Result<SignUp, AuthError>> {
        Box::pin(async move { Ok(SignUp::SignedIn(fake_session(email))) })
    }
}

#[derive(Default)]
pub struct MemorySessionStore(Mutex<Option<Session>>);

impl SessionStore for MemorySessionStore {
    fn load(&self) -> Option<Session> {
        self.0.lock().unwrap().clone()
    }

    fn save(&self, session: &Session) {
        *self.0.lock().unwrap() = Some(session.clone());
    }

    fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

/// Accepts `test:<email>` tokens. Never wire this into a real shell.
pub struct FakeIdentity;

impl Identity for FakeIdentity {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>> {
        Box::pin(async move {
            let email = token
                .strip_prefix("test:")
                .and_then(|e| Email::parse(e).ok())
                .ok_or_else(|| IdentityError::Invalid("not a test token".into()))?;
            Ok(Principal { user_id: UserId(format!("user-{email}")), email })
        })
    }
}

fn sample_event(name: &'static str, ws: &WorkspaceId) -> DomainEvent {
    DomainEvent {
        name,
        workspace: ws.clone(),
        actor: UserId("contract".into()),
        at: 1,
        fields: vec![("title", "ünïcode".into())],
    }
}

/// Run against every `TaskListRepository` implementation. `fresh` must return
/// a repository with no stored data; `reopen` must return a second repository
/// reading the same storage, to prove data survives a restart. `workspace`
/// returns a workspace id the storage accepts; ids must be distinct.
pub fn task_list_repository_contract<R: TaskListRepository>(
    fresh: impl Fn() -> R,
    reopen: impl Fn(&R) -> R,
    workspace: impl Fn(&R) -> WorkspaceId,
) {
    let repo = fresh();
    let ws = workspace(&repo);
    let other = workspace(&repo);
    assert_eq!(
        block_on(repo.load(&ws)).expect("empty storage loads"),
        (TaskList::new(), Revision::NEW)
    );

    let mut list = TaskList::new();
    let a = list.add(Title::parse("first").unwrap(), 10).unwrap();
    list.add(Title::parse("second ünïcode").unwrap(), 20).unwrap();
    list.complete(a, 30).unwrap();
    block_on(repo.save(&ws, &list, Revision::NEW, &[sample_event("task.added", &ws)]))
        .expect("save succeeds");
    let (loaded, rev) = block_on(repo.load(&ws)).unwrap();
    assert_eq!(loaded, list);
    assert_ne!(rev, Revision::NEW, "a save moves the revision on");

    let again = reopen(&repo);
    assert_eq!(block_on(again.load(&ws)).unwrap(), (list.clone(), rev), "data survives reopening");

    assert_eq!(
        block_on(again.save(&ws, &list, Revision::NEW, &[])),
        Err(SaveError::Conflict),
        "a stale revision is refused"
    );

    list.add(Title::parse("third").unwrap(), 40).unwrap();
    block_on(again.save(&ws, &list, rev, &[])).unwrap();
    assert_eq!(block_on(again.load(&ws)).unwrap().0, list, "a later save replaces the earlier one");

    assert_eq!(
        block_on(again.load(&other)).unwrap(),
        (TaskList::new(), Revision::NEW),
        "workspaces never see each other's tasks"
    );
}

/// Run against every `WorkspaceRepository` implementation.
pub fn workspace_repository_contract<R: WorkspaceRepository>(repo: R) {
    let ada = Email::parse(&format!("ada-{}@example.com", repo.new_id())).unwrap();
    let bob = Email::parse(&format!("bob-{}@example.com", repo.new_id())).unwrap();
    assert!(block_on(repo.list_for(&ada)).unwrap().is_empty());

    let id = repo.new_id();
    assert_ne!(id, repo.new_id(), "ids are never reused");
    assert_eq!(block_on(repo.load(&id)).unwrap(), None);

    let mut ws = Workspace::create(id.clone(), "Zeta ünïcode", ada.clone()).unwrap();
    block_on(repo.save(&ws, Revision::NEW, &[sample_event("workspace.created", &id)])).unwrap();
    let (loaded, rev) = block_on(repo.load(&id)).unwrap().expect("saved workspace loads");
    assert_eq!(loaded, ws);
    assert_eq!(block_on(repo.save(&ws, Revision::NEW, &[])), Err(SaveError::Conflict));

    let alpha = Workspace::create(repo.new_id(), "Alpha", ada.clone()).unwrap();
    block_on(repo.save(&alpha, Revision::NEW, &[])).unwrap();

    ws.set_member(bob.clone(), Role::Viewer).unwrap();
    block_on(repo.save(&ws, rev, &[])).unwrap();
    assert_eq!(block_on(repo.save(&ws, rev, &[])), Err(SaveError::Conflict));

    let names = |e: &Email| {
        block_on(repo.list_for(e))
            .unwrap()
            .into_iter()
            .map(|s| (s.name, s.role))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&ada), [("Alpha".into(), Role::Owner), ("Zeta ünïcode".into(), Role::Owner)]);
    assert_eq!(names(&bob), [("Zeta ünïcode".into(), Role::Viewer)]);

    ws.remove_member(&bob).unwrap();
    let (_, rev) = block_on(repo.load(&id)).unwrap().unwrap();
    block_on(repo.save(&ws, rev, &[])).unwrap();
    assert!(names(&bob).is_empty(), "removed members lose access");

    let (u1, u2) = (UserId(format!("u-{}", repo.new_id())), UserId(format!("u-{}", repo.new_id())));
    let personal = repo.personal_id(&u1);
    assert_eq!(personal, repo.personal_id(&u1), "the personal id is stable");
    assert_ne!(personal, repo.personal_id(&u2), "and differs per person");
    let mine = Workspace::create(personal, "Mine", bob.clone()).unwrap();
    block_on(repo.save(&mine, Revision::NEW, &[])).unwrap();
    assert_eq!(
        block_on(repo.save(&mine, Revision::NEW, &[])),
        Err(SaveError::Conflict),
        "a second creation of the same personal workspace conflicts"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn in_memory_task_repository_honours_the_contract() {
        task_list_repository_contract(
            || Arc::new(InMemoryTaskListRepository::default()),
            |r| r.clone(),
            |_| WorkspaceId(format!("w{}", NEXT.fetch_add(1, Ordering::SeqCst))),
        );
    }

    #[test]
    fn in_memory_workspace_repository_honours_the_contract() {
        workspace_repository_contract(InMemoryWorkspaceRepository::default());
    }

    impl TaskListRepository for Arc<InMemoryTaskListRepository> {
        fn load<'a>(
            &'a self,
            ws: &'a WorkspaceId,
        ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
            self.as_ref().load(ws)
        }
        fn save<'a>(
            &'a self,
            ws: &'a WorkspaceId,
            list: &'a TaskList,
            expected: Revision,
            events: &'a [DomainEvent],
        ) -> BoxFuture<'a, Result<(), SaveError>> {
            self.as_ref().save(ws, list, expected, events)
        }
    }
}
