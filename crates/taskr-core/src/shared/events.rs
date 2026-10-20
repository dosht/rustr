//! Domain events: facts the business cares about, written to an outbox in the
//! same transaction as the change. Analytics, ETL and realtime read the outbox
//! later, so no use case has to be revisited to feed them.

use crate::shared::access::{UserId, WorkspaceId};
use crate::shared::clock::Millis;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainEvent {
    pub name: &'static str,
    pub workspace: WorkspaceId,
    pub actor: UserId,
    pub at: Millis,
    pub fields: Vec<(&'static str, String)>,
}

impl DomainEvent {
    pub fn field(mut self, key: &'static str, value: impl ToString) -> Self {
        self.fields.push((key, value.to_string()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }
}
