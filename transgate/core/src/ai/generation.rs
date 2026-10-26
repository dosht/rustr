//! The generation lifecycle and its fences, pure.
//!
//! accepted -> running (claim) -> completed | interrupted | cancelled. Only the
//! execution token that claimed a running generation may checkpoint, stage or
//! complete it. Every write is fenced by the admission consent epoch, the
//! job's content revision and the erasure barrier, read fresh under the
//! account lock by the caller.

use uuid::Uuid;

use crate::ai::GenerationId;
use crate::ai::ports::{Draft, Execution, FinalContent, Generation, GenerationInput, GenerationKind, GenerationStatus, Interruption};
use crate::ids::{AccountId, JobId, Micros};

/// What every claim, checkpoint, stage and completion re-reads under the account lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fences {
    pub erasing: bool,
    /// The job is visible to its owner (not soft deleted) and has lines.
    pub source_present: bool,
    pub source_revision: i64,
    /// `consent::may_write(record, generation.consent_epoch)`.
    pub consent_current: bool,
}

/// The first fence a generation fails, if any.
pub fn fenced(g: &Generation, f: &Fences) -> Option<Interruption> {
    if f.erasing {
        return Some(Interruption::Erased);
    }
    if !f.consent_current {
        return Some(Interruption::Revoked);
    }
    if !f.source_present || f.source_revision != g.source_revision {
        return Some(Interruption::SourceChanged);
    }
    None
}

/// The request digest: kind, job and the payload that decides the answer.
/// The same request key with another digest is a conflict, never a replay.
pub fn digest(kind: GenerationKind, job: JobId, input: &GenerationInput) -> String {
    let payload = serde_json::to_string(input).unwrap_or_default();
    format!("{kind:?}:{}:{}", job.0, fnv64(payload.as_bytes()))
}

fn fnv64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub struct NewGeneration {
    pub account: AccountId,
    pub job: JobId,
    pub kind: GenerationKind,
    pub request_key: Uuid,
    pub source_revision: i64,
    pub consent_epoch: i64,
    pub input: GenerationInput,
}

pub fn accepted(n: NewGeneration, now: Micros) -> Generation {
    let request_digest = digest(n.kind, n.job, &n.input);
    Generation {
        id: GenerationId::new(),
        account: n.account,
        job: n.job,
        kind: n.kind,
        request_key: n.request_key,
        request_digest,
        source_revision: n.source_revision,
        consent_epoch: n.consent_epoch,
        input: n.input,
        status: GenerationStatus::Accepted,
        sequence: 0,
        draft: Draft::Empty,
        staged: None,
        execution: None,
        quota: None,
        interruption: None,
        remote_outcome_unknown: false,
        remote_id: None,
        tokens_used: None,
        created_at: now,
        updated_at: now,
        finished_at: None,
    }
}

/// Why a write by an execution token was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stale {
    /// The generation already ended.
    Terminal(GenerationStatus),
    /// Another execution holds it, or none does.
    NotOwner,
}

/// Claims an accepted generation for one execution.
pub fn claim(g: &mut Generation, token: Uuid, lease_until: Micros, now: Micros) -> Result<(), Stale> {
    match g.status {
        GenerationStatus::Accepted => {
            g.status = GenerationStatus::Running;
            g.execution = Some(Execution { token, lease_until });
            g.updated_at = now;
            Ok(())
        }
        s if s.is_terminal() => Err(Stale::Terminal(s)),
        _ => Err(Stale::NotOwner),
    }
}

/// Whether a running generation's worker lease expired (recovery may act).
pub fn lease_expired(g: &Generation, now: Micros) -> bool {
    g.status == GenerationStatus::Running && g.execution.is_none_or(|e| now.0 >= e.lease_until.0)
}

pub fn owns(g: &Generation, token: Uuid) -> Result<(), Stale> {
    if g.status.is_terminal() {
        return Err(Stale::Terminal(g.status));
    }
    match g.execution {
        Some(e) if e.token == token && g.status == GenerationStatus::Running => Ok(()),
        _ => Err(Stale::NotOwner),
    }
}

/// A sanitized cumulative checkpoint. The sequence only grows and never falls
/// below `seq_floor`, the last live frame sequence of this execution.
pub fn checkpoint(g: &mut Generation, token: Uuid, draft: Draft, seq_floor: i64, lease_until: Micros, now: Micros) -> Result<i64, Stale> {
    owns(g, token)?;
    g.draft = draft;
    g.sequence = (g.sequence + 1).max(seq_floor);
    g.execution = Some(Execution { token, lease_until });
    g.updated_at = now;
    Ok(g.sequence)
}

/// The sanitized final, durable before the completing transaction (D2).
pub fn stage(g: &mut Generation, token: Uuid, content: FinalContent, now: Micros) -> Result<(), Stale> {
    owns(g, token)?;
    g.staged = Some(content);
    g.updated_at = now;
    Ok(())
}

/// Completes with the staged final. The caller saves the artifact (and the
/// settlement) in the same transaction. No token check: recovery may finish
/// a staged final of a lost worker without another provider call.
pub fn complete(g: &mut Generation, now: Micros) -> Result<(), Stale> {
    if g.status.is_terminal() {
        return Err(Stale::Terminal(g.status));
    }
    if g.staged.is_none() {
        return Err(Stale::NotOwner);
    }
    g.status = GenerationStatus::Completed;
    g.sequence += 1;
    g.quota = None;
    g.interruption = None;
    g.updated_at = now;
    g.finished_at = Some(now);
    Ok(())
}

/// Ends without a durable final; valid partial content stays in the draft.
/// The caller releases any reservation in the same transaction.
pub fn interrupt(g: &mut Generation, reason: Interruption, remote_outcome_unknown: bool, now: Micros) -> bool {
    if g.status.is_terminal() {
        return false;
    }
    g.status = GenerationStatus::Interrupted;
    g.sequence += 1;
    g.quota = None;
    g.staged = None;
    g.interruption = Some(reason);
    g.remote_outcome_unknown |= remote_outcome_unknown;
    g.updated_at = now;
    g.finished_at = Some(now);
    true
}

/// The owner cancels. A completed result stays completed.
pub fn cancel(g: &mut Generation, now: Micros) -> bool {
    if g.status.is_terminal() {
        return false;
    }
    let dispatched = g.status == GenerationStatus::Running;
    g.status = GenerationStatus::Cancelled;
    g.sequence += 1;
    g.quota = None;
    g.staged = None;
    g.remote_outcome_unknown |= dispatched;
    g.updated_at = now;
    g.finished_at = Some(now);
    true
}
