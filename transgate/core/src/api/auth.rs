//! Who is asking (`Actor`), what for (`Action`, `Resource`) and the pure
//! `can` decision (D1). Use cases call `can` before anything else; the HTTP
//! layer never decides (API-D01, API-D02).

use serde::{Deserialize, Serialize};

use crate::api::KeyId;
use crate::ids::{AccountId, JobId};

/// A transcription scope with the source strings (`dashboard/api-keys.tsx:50-69`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "transcriptions:read")]
    TranscriptionsRead,
    #[serde(rename = "transcriptions:create")]
    TranscriptionsCreate,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::TranscriptionsRead => "transcriptions:read",
            Scope::TranscriptionsCreate => "transcriptions:create",
        }
    }
}

/// The authenticated caller. HTTP builds it; the route decides which kind it
/// accepts. There is no fallback from a bad key to a session (API-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Actor {
    /// A signed-in browser session.
    Session { account: AccountId },
    /// A verified API key. `scopes` are the stored strings; unknown ones are
    /// kept (migrated rows) and grant nothing.
    ApiKey { account: AccountId, key: KeyId, scopes: Vec<String> },
}

impl Actor {
    pub fn account(&self) -> AccountId {
        match self {
            Actor::Session { account } | Actor::ApiKey { account, .. } => *account,
        }
    }

    fn has(&self, scope: Scope) -> bool {
        matches!(self, Actor::ApiKey { scopes, .. } if scopes.iter().any(|s| s == scope.as_str()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `POST /api/v1/transcriptions`.
    TranscriptionCreate,
    /// `GET /api/v1/transcriptions`.
    TranscriptionRead,
    /// Create, list or revoke keys (the dashboard).
    KeysManage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// The account a new job or key is created for, or whose keys are listed.
    Account(AccountId),
    Job { id: JobId, owner: AccountId },
    Key { id: KeyId, owner: AccountId },
}

/// The whole authorization table.
///
/// | Action | Allowed |
/// |---|---|
/// | `TranscriptionCreate` | an API key with `transcriptions:create`, for its own account |
/// | `TranscriptionRead` | an API key with `transcriptions:read`, for a job its account owns |
/// | `KeysManage` | a session, for its own account or a key its account owns |
///
/// Sessions never use the public transcription routes (P5 D5) and keys never
/// manage keys (D1).
pub fn can(actor: &Actor, action: Action, resource: Resource) -> bool {
    match (actor, action, resource) {
        (Actor::ApiKey { account, .. }, Action::TranscriptionCreate, Resource::Account(target)) => {
            *account == target && actor.has(Scope::TranscriptionsCreate)
        }
        (Actor::ApiKey { account, .. }, Action::TranscriptionRead, Resource::Job { owner, .. }) => {
            *account == owner && actor.has(Scope::TranscriptionsRead)
        }
        (Actor::Session { account }, Action::KeysManage, Resource::Account(owner) | Resource::Key { owner, .. }) => *account == owner,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(n: u128) -> AccountId {
        AccountId(uuid::Uuid::from_u128(n))
    }

    fn key(scopes: &[&str]) -> Actor {
        Actor::ApiKey { account: account(1), key: KeyId(uuid::Uuid::from_u128(9)), scopes: scopes.iter().map(|s| s.to_string()).collect() }
    }

    // API-D01 (fixes API-004): every public operation requires its scope; a read key
    // cannot create, a create key cannot read; unknown stored strings grant nothing.
    #[test]
    fn api_d01_public_operations_require_their_scope() {
        let mine = Resource::Account(account(1));
        let job = Resource::Job { id: JobId(5), owner: account(1) };
        assert!(can(&key(&["transcriptions:create"]), Action::TranscriptionCreate, mine));
        assert!(!can(&key(&["transcriptions:read"]), Action::TranscriptionCreate, mine));
        assert!(can(&key(&["transcriptions:read"]), Action::TranscriptionRead, job));
        assert!(!can(&key(&["transcriptions:create"]), Action::TranscriptionRead, job));
        assert!(can(&key(&["transcriptions:read", "transcriptions:create"]), Action::TranscriptionRead, job));
        assert!(!can(&key(&["transcriptions:*", "admin"]), Action::TranscriptionCreate, mine));
        assert!(!can(&key(&[]), Action::TranscriptionRead, job));
    }

    // API-012 owner filter, API-001: a key reads and creates only for its own account,
    // and a session never uses the public transcription routes.
    #[test]
    fn api_001_public_routes_are_key_only_and_owner_scoped() {
        let both = key(&["transcriptions:read", "transcriptions:create"]);
        assert!(!can(&both, Action::TranscriptionRead, Resource::Job { id: JobId(5), owner: account(2) }));
        assert!(!can(&both, Action::TranscriptionCreate, Resource::Account(account(2))));
        let session = Actor::Session { account: account(1) };
        assert!(!can(&session, Action::TranscriptionCreate, Resource::Account(account(1))));
        assert!(!can(&session, Action::TranscriptionRead, Resource::Job { id: JobId(5), owner: account(1) }));
    }

    // API-005, API-D02 (fixes API-006): only the owner's session lists, creates or
    // revokes keys; another account cannot, and a key actor never manages keys.
    #[test]
    fn api_d02_only_the_owners_session_manages_keys() {
        let session = Actor::Session { account: account(1) };
        let own = Resource::Key { id: KeyId(uuid::Uuid::from_u128(9)), owner: account(1) };
        let foreign = Resource::Key { id: KeyId(uuid::Uuid::from_u128(8)), owner: account(2) };
        assert!(can(&session, Action::KeysManage, own));
        assert!(can(&session, Action::KeysManage, Resource::Account(account(1))));
        assert!(!can(&session, Action::KeysManage, foreign));
        assert!(!can(&session, Action::KeysManage, Resource::Account(account(2))));
        assert!(!can(&key(&["transcriptions:read", "transcriptions:create"]), Action::KeysManage, own));
    }
}
