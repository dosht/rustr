//! Taskr's small summary lifecycle, independent of Transgate consent/quota policy.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// Distinct provider request identities kept per generation, first seen first (aligned with Transgate).
pub const MAX_REMOTE_IDS: usize = 8;
pub const MAX_REMOTE_ID_BYTES: usize = 256;
/// Every completion kind's total provider deadline (D4).
pub const PROVIDER_DEADLINE_US: i64 = 60_000_000;
/// Room after the provider deadline for the stage/settle transactions.
pub const SETTLE_MARGIN_US: i64 = 5_000_000;
/// How long an accepted generation may wait for its first claim; the execution lease starts there.
pub const ADMISSION_LEASE_US: i64 = 65_000_000;
const SOURCE_REMOVED: &str = "The source transcript was removed by retention.";

/// Why an observed provider identity could not be retained. Known identities are always kept and
/// nothing is truncated; an identity that does not fit is reported, never silently dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteIdLimit { TooLong, TooMany }

impl RemoteIdLimit {
    pub fn reason(self) -> &'static str {
        match self {
            Self::TooLong => "The provider request identity exceeds its byte limit.",
            Self::TooMany => "The provider reported more request identities than can be retained.",
        }
    }
}

/// Adds every observed identity that fits, first seen first, distinct and bounded.
pub fn merge_remote_ids(known: &mut Vec<String>, observed: &[String]) -> Result<(), RemoteIdLimit> {
    let mut refused = None;
    for id in observed {
        if id.is_empty() || known.contains(id) { continue; }
        if id.len() > MAX_REMOTE_ID_BYTES { refused.get_or_insert(RemoteIdLimit::TooLong); continue; }
        if known.len() >= MAX_REMOTE_IDS { refused.get_or_insert(RemoteIdLimit::TooMany); continue; }
        known.push(id.clone());
    }
    refused.map_or(Ok(()), Err)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus { Accepted, Running, Completed, Interrupted, Cancelled }

impl GenerationStatus {
    pub fn terminal(self) -> bool { matches!(self, Self::Completed | Self::Interrupted | Self::Cancelled) }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationSnapshot {
    pub generation_id: Uuid,
    pub work_id: Uuid,
    pub execution_id: Uuid,
    pub source_revision: u64,
    pub status: GenerationStatus,
    pub sequence: u64,
    pub text: String,
    pub durable: bool,
    pub remote_outcome_unknown: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSummary {
    pub generation_id: Uuid,
    pub work_id: Uuid,
    pub source_revision: u64,
    pub text: String,
    pub saved_at_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryView {
    pub summary: Option<SavedSummary>,
    pub generation: Option<GenerationSnapshot>,
}

/// One bounded current snapshot and an optional staged final, never a token log or input copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    pub id: Uuid,
    pub account: Uuid,
    pub work: Uuid,
    pub request: Uuid,
    pub execution: Uuid,
    pub source_revision: u64,
    pub status: GenerationStatus,
    pub sequence: u64,
    pub text: String,
    pub staged: Option<String>,
    /// Minimal dispatch facts survive a cascade only as unresolved erasure-resource metadata.
    pub provider: Option<String>,
    /// Distinct provider ids (e.g. the response header id and the body completion id). Required:
    /// migration 0003 converted the 0002-era scalar, so a missing list is corrupt, not empty.
    pub remote_ids: Vec<String>,
    pub remote_outcome_unknown: bool,
    pub error: Option<String>,
    pub created_us: i64,
    pub updated_us: i64,
    pub lease_until_us: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputLimit;

impl Generation {
    pub fn new(account: Uuid, work: Uuid, request: Uuid, source_revision: u64, now: i64, lease_until_us: i64) -> Self {
        Self {
            id: Uuid::new_v4(), account, work, request, execution: Uuid::new_v4(), source_revision,
            status: GenerationStatus::Accepted, sequence: 0, text: String::new(), staged: None,
            provider: None, remote_ids: Vec::new(),
            remote_outcome_unknown: false, error: None, created_us: now, updated_us: now, lease_until_us,
        }
    }

    fn changed(&mut self, now: i64) { self.sequence += 1; self.updated_us = now; }

    /// The first successful claim, recorded before HTTP dispatch. The execution lease starts here and
    /// covers the whole provider deadline plus settlement, however late the claim was admitted.
    pub fn claim(&mut self, now: i64) -> bool {
        if self.status != GenerationStatus::Accepted || now >= self.lease_until_us { return false; }
        self.status = GenerationStatus::Running;
        // A lost process cannot prove nonacceptance.
        self.remote_outcome_unknown = true;
        self.lease_until_us = now + PROVIDER_DEADLINE_US + SETTLE_MARGIN_US;
        self.changed(now);
        true
    }

    pub fn checkpoint(&mut self, text: String, now: i64) -> Result<bool, OutputLimit> {
        if text.len() > MAX_OUTPUT_BYTES { return Err(OutputLimit); }
        if self.status != GenerationStatus::Running { return Ok(false); }
        if self.text == text { return Ok(true); }
        self.text = text;
        self.changed(now);
        Ok(true)
    }

    pub fn stage(&mut self, text: String, now: i64) -> Result<bool, OutputLimit> {
        if text.len() > MAX_OUTPUT_BYTES { return Err(OutputLimit); }
        if self.status != GenerationStatus::Running { return Ok(false); }
        self.staged = Some(text);
        self.changed(now);
        Ok(true)
    }

    /// Only the store calls this, in the transaction that saves the canonical summary too.
    pub fn complete(&mut self, now: i64) -> bool {
        if self.status != GenerationStatus::Running { return false; }
        let Some(text) = self.staged.take() else { return false };
        self.text = text;
        self.status = GenerationStatus::Completed;
        self.remote_outcome_unknown = false;
        self.changed(now);
        true
    }

    /// Lead decision D10: a cancel committed before the canonical save wins, discarding a staged
    /// final that was never settled. A completed generation is canonical and cannot be cancelled.
    pub fn cancel(&mut self, now: i64) -> bool {
        if self.status.terminal() { return false; }
        self.status = GenerationStatus::Cancelled;
        self.staged = None;
        self.changed(now);
        true
    }

    pub fn interrupt(&mut self, why: String, now: i64) -> bool {
        if self.status.terminal() { return false; }
        self.status = GenerationStatus::Interrupted;
        self.staged = None;
        self.error = Some(why);
        self.changed(now);
        true
    }

    /// Lead decision D10: once retention removed the source transcript, no non-canonical derived
    /// text survives. Active work is interrupted; terminal reasons, provider facts and a completed
    /// (canonical) summary stay. Only the sequence moves so attachments see the replacement.
    pub fn purge_source_content(&mut self) -> bool {
        if self.status == GenerationStatus::Completed { return false; }
        let mut changed = false;
        if !self.status.terminal() {
            self.status = GenerationStatus::Interrupted;
            self.error = Some(SOURCE_REMOVED.into());
            changed = true;
        }
        if !self.text.is_empty() || self.staged.is_some() {
            self.text.clear();
            self.staged = None;
            changed = true;
        }
        if changed { self.sequence += 1; }
        changed
    }

    pub fn snapshot(&self) -> GenerationSnapshot {
        GenerationSnapshot {
            generation_id: self.id, work_id: self.work, execution_id: self.execution,
            source_revision: self.source_revision, status: self.status, sequence: self.sequence,
            text: self.text.clone(), durable: true, remote_outcome_unknown: self.remote_outcome_unknown,
            error: self.error.clone(),
        }
    }

    pub fn saved(&self) -> Option<SavedSummary> {
        (self.status == GenerationStatus::Completed).then(|| SavedSummary {
            generation_id: self.id, work_id: self.work, source_revision: self.source_revision,
            text: self.text.clone(), saved_at_us: self.updated_us,
        })
    }
}

#[cfg(test)]
mod tests;
