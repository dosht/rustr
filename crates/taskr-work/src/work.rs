//! The audio-work aggregate: one uploaded audio object, at most one live
//! provider attempt at a time, a fallback from one provider to the other,
//! lease reconciliation after a lost answer, retention of the transcript and
//! the resource manifest erasure needs.
//!
//! Pure decisions only. No network, SQL or clock: the caller passes instants
//! and applies every answer it observed. A stale answer (an attempt that is no
//! longer the live one) is refused and changes nothing, so a late
//! acknowledgement can never undo a completion or revive a superseded attempt.

use media_ports::{ObjectRef, Provider, RemoteIdentity};
pub use media_ports::Micros;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SECOND: i64 = 1_000_000;
const DAY: i64 = 86_400 * SECOND;

/// Every provider the sample can use, in default preference order.
pub const PROVIDERS: [Provider; 2] = [Provider::Soniox, Provider::Deepgram];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    AwaitingUpload,
    Uploaded,
    Pending,
    Running,
    Completed,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::AwaitingUpload => "awaiting_upload",
            Status::Uploaded => "uploaded",
            Status::Pending => "pending",
            Status::Running => "running",
            Status::Completed => "completed",
            Status::Failed => "failed",
        }
    }

    pub fn parse(text: &str) -> Option<Status> {
        Some(match text {
            "awaiting_upload" => Status::AwaitingUpload,
            "uploaded" => Status::Uploaded,
            "pending" => Status::Pending,
            "running" => Status::Running,
            "completed" => Status::Completed,
            "failed" => Status::Failed,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptPhase {
    /// Recorded before any request left the process.
    Claimed,
    /// The provider answered and the remote identity is bound.
    Accepted,
    /// The request may or may not have been accepted: nonacceptance is not proved.
    Uncertain,
    /// The provider definitely refused: nothing remote exists.
    Rejected,
    /// Given up for a new attempt. Whatever it may have started remains an erasure resource.
    Superseded,
    /// The provider reported its own failure after accepting.
    Failed,
    /// The result was applied.
    Done,
}

impl AttemptPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptPhase::Claimed => "claimed",
            AttemptPhase::Accepted => "accepted",
            AttemptPhase::Uncertain => "uncertain",
            AttemptPhase::Rejected => "rejected",
            AttemptPhase::Superseded => "superseded",
            AttemptPhase::Failed => "failed",
            AttemptPhase::Done => "done",
        }
    }

    pub fn parse(text: &str) -> Option<AttemptPhase> {
        Some(match text {
            "claimed" => AttemptPhase::Claimed,
            "accepted" => AttemptPhase::Accepted,
            "uncertain" => AttemptPhase::Uncertain,
            "rejected" => AttemptPhase::Rejected,
            "superseded" => AttemptPhase::Superseded,
            "failed" => AttemptPhase::Failed,
            "done" => AttemptPhase::Done,
            _ => return None,
        })
    }

    /// Still able to produce a result or an answer.
    pub fn is_live(self) -> bool {
        matches!(self, AttemptPhase::Claimed | AttemptPhase::Accepted | AttemptPhase::Uncertain)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: Uuid,
    pub number: u32,
    pub provider: Provider,
    pub phase: AttemptPhase,
    pub lease_until: Micros,
    pub claimed_at: Micros,
    pub accepted_at: Option<Micros>,
    pub remote: Option<RemoteIdentity>,
    pub reason: Option<String>,
    /// Digest of the callback payload whose result was applied (the first one binds).
    pub result_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// How long a claimed or unanswered submission may stay unreconciled.
    pub lease_micros: i64,
    /// How long an accepted attempt may run before it is looked at again.
    pub stuck_after_micros: i64,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy { lease_micros: 120 * SECOND, stuck_after_micros: 30 * 60 * SECOND }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkError {
    /// The work is not in a state that allows this command.
    NotReady,
    /// Another attempt is still live.
    Busy,
    /// The answer is for an attempt that is no longer the live one.
    Stale,
    /// That provider was already used up for this work.
    ProviderSpent,
    /// An authenticated observation names a different remote identity than the one bound.
    IdentityMismatch,
    /// The same attempt and identity delivered a different result payload than the one applied.
    PayloadConflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Changed,
    /// An exact repeat: nothing was touched and no revision moved.
    Unchanged,
}

/// Everything erasure must account for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resources {
    pub object: ObjectRef,
    /// Every remote identity ever bound, superseded attempts included.
    pub remotes: Vec<RemoteIdentity>,
    /// Attempts that may have started remote work but whose identity was never learned.
    pub unidentified_attempts: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Work {
    pub id: Uuid,
    pub owner: Uuid,
    pub object: ObjectRef,
    pub name: String,
    pub language: Option<String>,
    pub requested: Option<Provider>,
    pub status: Status,
    pub revision: u64,
    pub attempts: Vec<Attempt>,
    pub transcript: Option<String>,
    pub error: Option<String>,
    pub created_at: Micros,
    pub completed_at: Option<Micros>,
}

impl Work {
    pub fn create(
        id: Uuid,
        owner: Uuid,
        object: ObjectRef,
        name: String,
        language: Option<String>,
        requested: Option<Provider>,
        now: Micros,
    ) -> Work {
        Work {
            id,
            owner,
            object,
            name,
            language,
            requested,
            status: Status::AwaitingUpload,
            revision: 0,
            attempts: Vec::new(),
            transcript: None,
            error: None,
            created_at: now,
            completed_at: None,
        }
    }

    fn bump(&mut self) {
        self.revision += 1;
    }

    fn latest(&self) -> Option<&Attempt> {
        self.attempts.last()
    }

    /// The attempt an answer may still act on: the newest one, and only while it is live.
    fn live(&self, id: Uuid) -> Result<usize, WorkError> {
        let index = self.attempts.len().checked_sub(1).ok_or(WorkError::Stale)?;
        let latest = &self.attempts[index];
        if latest.id == id && latest.phase.is_live() && !matches!(self.status, Status::Completed | Status::Failed) {
            Ok(index)
        } else {
            Err(WorkError::Stale)
        }
    }

    /// The object exists in storage: submission may begin.
    pub fn uploaded(&mut self, _now: Micros) -> Result<Applied, WorkError> {
        match self.status {
            Status::AwaitingUpload => {
                self.status = Status::Uploaded;
                self.bump();
                Ok(Applied::Changed)
            }
            Status::Uploaded | Status::Pending | Status::Running | Status::Completed => Ok(Applied::Unchanged),
            Status::Failed => Err(WorkError::NotReady),
        }
    }

    /// Providers still worth trying, best first: the requested one if configured and unused,
    /// then Soniox, then Deepgram. A rejected or superseded provider is never tried again.
    pub fn candidates(&self, available: &[Provider]) -> Vec<Provider> {
        let spent = |p: Provider| {
            self.attempts.iter().any(|a| a.provider == p && matches!(a.phase, AttemptPhase::Rejected | AttemptPhase::Superseded))
        };
        let mut order: Vec<Provider> = Vec::new();
        if let Some(requested) = self.requested {
            order.push(requested);
        }
        order.extend(PROVIDERS);
        let mut out: Vec<Provider> = Vec::new();
        for provider in order {
            if available.contains(&provider) && !spent(provider) && !out.contains(&provider) {
                out.push(provider);
            }
        }
        out
    }

    /// Records an attempt and its lease. Nothing may be sent before this is stored.
    pub fn claim(&mut self, provider: Provider, attempt: Uuid, now: Micros, policy: &Policy) -> Result<(), WorkError> {
        if !matches!(self.status, Status::Uploaded | Status::Pending) {
            return Err(WorkError::NotReady);
        }
        if self.attempts.iter().any(|a| a.phase.is_live()) {
            return Err(WorkError::Busy);
        }
        if self.attempts.iter().any(|a| a.provider == provider && matches!(a.phase, AttemptPhase::Rejected | AttemptPhase::Superseded)) {
            return Err(WorkError::ProviderSpent);
        }
        self.attempts.push(Attempt {
            id: attempt,
            number: self.attempts.len() as u32 + 1,
            provider,
            phase: AttemptPhase::Claimed,
            lease_until: Micros(now.0 + policy.lease_micros),
            claimed_at: now,
            accepted_at: None,
            remote: None,
            reason: None,
            result_digest: None,
        });
        self.status = Status::Pending;
        self.bump();
        Ok(())
    }

    /// Records the identity an authenticated observation names on any attempt, live or not,
    /// without changing its phase: remote work an attempt that was given up still started must
    /// stay in the erasure manifest. A different identity than the bound one is refused.
    fn learned(&mut self, attempt: Uuid, remote: RemoteIdentity) -> Result<Applied, WorkError> {
        let index = self.attempts.iter().position(|a| a.id == attempt).ok_or(WorkError::Stale)?;
        let bound = self.attempts[index].remote.clone();
        match bound {
            Some(bound) if bound == remote => Ok(Applied::Unchanged),
            Some(_) => Err(WorkError::IdentityMismatch),
            None => {
                self.attempts[index].remote = Some(remote);
                self.bump();
                Ok(Applied::Changed)
            }
        }
    }
    /// Whether a payload digest agrees with the one already bound to the attempt, judged on its own
    /// and before any lifecycle decision: a finished or failed attempt answers `Stale` to every
    /// lifecycle call, so only the digest can tell an exact repeat from a different result.
    pub fn check_digest(&self, attempt: Uuid, digest: &str) -> Result<(), WorkError> {
        let found = self.attempts.iter().find(|a| a.id == attempt).ok_or(WorkError::Stale)?;
        match found.result_digest.as_deref() {
            Some(bound) if bound != digest => Err(WorkError::PayloadConflict),
            _ => Ok(()),
        }
    }


    /// The digest of the payload that produced this attempt's result. The first binds; the same
    /// one again is a repeat; another payload for the same attempt and identity is a conflict.
    pub fn record_digest(&mut self, attempt: Uuid, digest: &str) -> Result<Applied, WorkError> {
        let index = self.attempts.iter().position(|a| a.id == attempt).ok_or(WorkError::Stale)?;
        match self.attempts[index].result_digest.as_deref() {
            Some(bound) if bound == digest => Ok(Applied::Unchanged),
            Some(_) => Err(WorkError::PayloadConflict),
            None => {
                self.attempts[index].result_digest = Some(digest.to_string());
                self.bump();
                Ok(Applied::Changed)
            }
        }
    }

    /// An authenticated provider observation names this attempt's remote identity. The first one
    /// is adopted for a claimed or unanswered attempt; a repeat is a no-op; a different one than
    /// the bound identity is refused, never swapped in.
    pub fn observed(&mut self, attempt: Uuid, remote: RemoteIdentity, now: Micros) -> Result<Applied, WorkError> {
        // The identity is compared first: an attempt that is no longer live (finished, given up)
        // still has a bound identity, and a different one is a conflict, never a duplicate.
        let Ok(index) = self.live(attempt) else { return self.learned(attempt, remote) };
        let bound = self.attempts[index].remote.clone();
        match bound {
            Some(bound) if bound == remote => Ok(Applied::Unchanged),
            Some(_) => Err(WorkError::IdentityMismatch),
            None => self.accepted(attempt, remote, now),
        }
    }

    /// The provider accepted and named its remote identity. A late acknowledgement of the live
    /// attempt is adopted; one for any other attempt is stale.
    pub fn accepted(&mut self, attempt: Uuid, remote: RemoteIdentity, now: Micros) -> Result<Applied, WorkError> {
        let index = self.live(attempt)?;
        if !matches!(self.attempts[index].phase, AttemptPhase::Claimed | AttemptPhase::Uncertain) {
            return Err(WorkError::Stale);
        }
        let a = &mut self.attempts[index];
        a.phase = AttemptPhase::Accepted;
        a.remote = Some(remote);
        a.accepted_at = Some(now);
        self.status = Status::Running;
        self.bump();
        Ok(Applied::Changed)
    }

    /// A definite refusal. Fails the work only when no provider is left to try.
    pub fn rejected(&mut self, attempt: Uuid, reason: String, _now: Micros) -> Result<Applied, WorkError> {
        let index = self.live(attempt)?;
        if !matches!(self.attempts[index].phase, AttemptPhase::Claimed | AttemptPhase::Uncertain) {
            return Err(WorkError::Stale);
        }
        let a = &mut self.attempts[index];
        a.phase = AttemptPhase::Rejected;
        a.reason = Some(reason);
        if self.candidates(&PROVIDERS).is_empty() {
            let all_refused = self.attempts.iter().all(|a| a.phase == AttemptPhase::Rejected);
            self.status = Status::Failed;
            self.error = Some(if all_refused { "all_providers_rejected" } else { "providers_exhausted" }.into());
        } else {
            self.status = Status::Pending;
        }
        self.bump();
        Ok(Applied::Changed)
    }

    /// The answer to a submission was lost: acceptance is neither proved nor refuted.
    pub fn uncertain(&mut self, attempt: Uuid) -> Result<Applied, WorkError> {
        let index = self.live(attempt)?;
        match self.attempts[index].phase {
            AttemptPhase::Uncertain => Ok(Applied::Unchanged),
            AttemptPhase::Claimed => {
                self.attempts[index].phase = AttemptPhase::Uncertain;
                self.bump();
                Ok(Applied::Changed)
            }
            _ => Err(WorkError::Stale),
        }
    }

    /// A claimed or unanswered attempt whose lease has run out: the caller reconciles it with
    /// the provider (a listing miss is not proof of nonacceptance), then supersedes or adopts it.
    pub fn expired(&self, now: Micros) -> Option<&Attempt> {
        self.latest().filter(|a| matches!(a.phase, AttemptPhase::Claimed | AttemptPhase::Uncertain) && a.lease_until <= now)
    }

    /// An accepted attempt that has run longer than the policy window without a result.
    pub fn stuck(&self, now: Micros, policy: &Policy) -> Option<&Attempt> {
        if self.status != Status::Running {
            return None;
        }
        self.latest().filter(|a| a.phase == AttemptPhase::Accepted && a.accepted_at.is_some_and(|at| at.0 + policy.stuck_after_micros <= now.0))
    }

    /// Gives the live attempt up so another provider can be claimed. Any remote identity it
    /// already has stays in the manifest.
    pub fn supersede(&mut self, attempt: Uuid, _now: Micros) -> Result<Applied, WorkError> {
        let index = self.live(attempt)?;
        self.attempts[index].phase = AttemptPhase::Superseded;
        self.status = Status::Pending;
        self.bump();
        Ok(Applied::Changed)
    }

    /// Pending work with nothing live and no configured provider left to try cannot progress:
    /// it fails instead of waiting forever.
    pub fn exhausted(&mut self, available: &[Provider]) -> Applied {
        let idle = self.status == Status::Pending && !self.attempts.iter().any(|a| a.phase.is_live());
        if idle && self.candidates(available).is_empty() {
            self.status = Status::Failed;
            self.error = Some("providers_exhausted".into());
            self.bump();
            Applied::Changed
        } else {
            Applied::Unchanged
        }
    }

    /// The live attempt's result. Applied once; an exact repeat is a no-op and a different result
    /// from the attempt that already has one is a conflict, never a replay.
    pub fn complete(&mut self, attempt: Uuid, transcript: String, now: Micros) -> Result<Applied, WorkError> {
        if self.status == Status::Completed {
            return match self.attempts.iter().find(|a| a.id == attempt) {
                Some(a) if a.phase == AttemptPhase::Done => match &self.transcript {
                    Some(applied) if *applied != transcript => Err(WorkError::PayloadConflict),
                    _ => Ok(Applied::Unchanged),
                },
                _ => Err(WorkError::Stale),
            };
        }
        let index = self.live(attempt)?;
        if self.attempts[index].phase != AttemptPhase::Accepted {
            return Err(WorkError::Stale);
        }
        self.attempts[index].phase = AttemptPhase::Done;
        self.status = Status::Completed;
        self.transcript = Some(transcript);
        self.completed_at = Some(now);
        self.error = None;
        self.bump();
        Ok(Applied::Changed)
    }

    /// The provider reported its own failure after accepting. Never undoes a completion.
    pub fn fail(&mut self, attempt: Uuid, reason: String, _now: Micros) -> Result<Applied, WorkError> {
        if self.status == Status::Completed {
            return Ok(Applied::Unchanged);
        }
        let index = self.live(attempt)?;
        self.attempts[index].phase = AttemptPhase::Failed;
        self.attempts[index].reason = Some(reason.clone());
        self.status = Status::Failed;
        self.error = Some(reason);
        self.bump();
        Ok(Applied::Changed)
    }

    /// Completed work whose transcript is past the retention window: created at or before
    /// `now - days - 1 day`, the source's inclusive extra-day arithmetic.
    pub fn retention_due(&self, now: Micros, days: i64) -> bool {
        self.status == Status::Completed && self.transcript.is_some() && self.created_at.0 <= now.0 - (days + 1) * DAY
    }

    /// Removes the transcript and keeps the work row.
    pub fn purge_transcript(&mut self) -> Applied {
        if self.transcript.take().is_some() {
            self.bump();
            Applied::Changed
        } else {
            Applied::Unchanged
        }
    }

    pub fn resources(&self) -> Resources {
        Resources {
            object: self.object.clone(),
            remotes: self.attempts.iter().filter_map(|a| a.remote.clone()).collect(),
            unidentified_attempts: self
                .attempts
                .iter()
                .filter(|a| a.remote.is_none() && !matches!(a.phase, AttemptPhase::Rejected))
                .count(),
        }
    }
}

#[cfg(test)]
mod tests;
