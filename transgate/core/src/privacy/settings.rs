//! Per-user privacy flags (privacy-identity 2.2).

use serde::{Deserialize, Serialize};

use crate::ids::AccountId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub account: AccountId,
    /// PRV-013: opt-out design, default true, NOT NULL. Only this flag is
    /// user-writable (PRV-052 as built allowed every column: correction C6).
    pub auto_delete_transcripts: bool,
    /// PRV-033: operator field, default false; read-side only.
    pub unlimited_history: bool,
    /// PRV-051: operator field, NULL allowed; enforced on YouTube add/resolve only.
    pub blocked: Option<bool>,
    /// PRV-036: stored, toggled, read by nothing.
    pub deleted_monthly: bool,
}

impl UserSettings {
    pub fn defaults(account: AccountId) -> UserSettings {
        UserSettings { account, auto_delete_transcripts: true, unlimited_history: false, blocked: None, deleted_monthly: true }
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked == Some(true)
    }

    /// PRV-027: `UPDATE ... WHERE auto_delete_transcripts <> v`. Returns whether it changed.
    pub fn set_auto_delete(&mut self, value: bool) -> bool {
        if self.auto_delete_transcripts == value {
            return false;
        }
        self.auto_delete_transcripts = value;
        true
    }

    /// PRV-036: the legacy toggle, kept until mu decides its fate.
    pub fn set_deleted_monthly(&mut self, value: bool) -> bool {
        if self.deleted_monthly == value {
            return false;
        }
        self.deleted_monthly = value;
        true
    }
}
