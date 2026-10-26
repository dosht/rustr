//! The daily transcript cleanup (PRV-014..025, CRN-005/007).

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::{AccountId, JobId, MICROS_PER_DAY, Micros};

/// PRV-018: 7..=365 or `Parameter validation failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RetentionDays(u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidRetentionDays(pub String);

impl fmt::Display for InvalidRetentionDays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidRetentionDays {}

impl RetentionDays {
    /// The cron argument (PRV-029, CRN-005).
    pub const SCHEDULED: RetentionDays = RetentionDays(30);
    /// The function default when called without an argument.
    pub const FUNCTION_DEFAULT: RetentionDays = RetentionDays(31);

    pub fn new(days: Option<i64>) -> Result<RetentionDays, InvalidRetentionDays> {
        let Some(days) = days else {
            return Err(InvalidRetentionDays("Parameter validation failed: retention_days cannot be NULL".into()));
        };
        if !(7..=365).contains(&days) {
            return Err(InvalidRetentionDays(format!(
                "Parameter validation failed: retention_days must be between 7 and 365, got: {days}"
            )));
        }
        Ok(RetentionDays(days as u16))
    }

    pub fn days(self) -> u16 {
        self.0
    }

    /// PRV-015: `now() - retention_days - 1 day`; a job created at or before it qualifies.
    pub fn cutoff(self, now: Micros) -> Micros {
        Micros(now.0 - (i64::from(self.0) + 1) * MICROS_PER_DAY)
    }
}

/// PRV-019: more than this many lines aborts the run without deleting anything.
pub const SAFETY_THRESHOLD: u64 = 100_000;

pub const TRANSCRIPT_JOB_NAME: &str = "cleanup-old-transcripts";
pub const TRANSCRIPT_TABLE: &str = "transcripts";
pub const HISTORY_JOB_NAME: &str = "cleanup-cron-job-history";
/// The scheduler's own run history: one row per claimed slot (media-postgres).
pub const HISTORY_TABLE: &str = "media.schedule_slots";
/// PRV-025/CRN-007: the scheduler's own run history keeps 90 days.
pub const HISTORY_RETENTION_DAYS: RetentionDays = RetentionDays(90);
/// PRV-020: the advisory lock names, the source's `hashtext(<function name>)` keys.
pub const TRANSCRIPT_LOCK: &str = "cleanup_old_transcripts";
pub const HISTORY_LOCK: &str = "cleanup_cron_job_history";
/// The honest end of a run whose process stopped before it finished. The deletion and the
/// success audit commit in one transaction, so such a run deleted nothing.
pub const INTERRUPTED_MESSAGE: &str = "Run interrupted before completion; its deletion did not commit";

/// A completed job old enough for the sweep, with its owner's preference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionCandidate {
    pub account: AccountId,
    pub job: JobId,
    /// `auto_delete_transcripts` is NOT NULL DEFAULT true (PRV-013): a missing
    /// settings row behaves as true, as the settings read answers.
    pub auto_delete: bool,
    pub line_count: u64,
}

/// The dry run's answer (PRV-023): the same predicate, nothing deleted, no audit row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPlan {
    pub retention_days: RetentionDays,
    pub cutoff: Micros,
    pub estimated_deletions: u64,
    pub users_opted_out: u64,
    pub would_exceed_threshold: bool,
    pub jobs: Vec<JobId>,
}

pub fn plan(days: RetentionDays, now: Micros, candidates: &[RetentionCandidate], users_opted_out: u64) -> RetentionPlan {
    let cutoff = days.cutoff(now);
    let deleting: Vec<&RetentionCandidate> = candidates.iter().filter(|c| c.auto_delete).collect();
    let estimated_deletions = deleting.iter().map(|c| c.line_count).sum();
    RetentionPlan {
        retention_days: days,
        cutoff,
        estimated_deletions,
        users_opted_out,
        would_exceed_threshold: estimated_deletions > SAFETY_THRESHOLD,
        jobs: deleting.iter().map(|c| c.job).collect(),
    }
}

/// PRV-021 statuses, including the two the source's CHECK rejected (C10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Recorded before work; a crash leaves it to be resolved as `failure`.
    Running,
    Success,
    Failure,
    Partial,
    Skipped,
    Aborted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Success => "success",
            RunStatus::Failure => "failure",
            RunStatus::Partial => "partial",
            RunStatus::Skipped => "skipped",
            RunStatus::Aborted => "aborted",
        }
    }
}

/// One audit row per run (PRV-021). Never readable by ordinary users (C10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionRun {
    pub id: Uuid,
    pub job_name: String,
    pub table_name: String,
    pub retention_days: u16,
    pub cutoff: Micros,
    pub rows_deleted: u64,
    pub execution_time_ms: u64,
    pub status: RunStatus,
    pub error_message: Option<String>,
    pub users_opted_out_count: Option<u64>,
    pub executed_at: Micros,
    /// `scheduler` for the 02:00 run, `operator` for an authenticated manual run.
    pub executed_by: String,
}

impl RetentionRun {
    pub fn started(days: RetentionDays, now: Micros, executed_by: &str) -> RetentionRun {
        RetentionRun::begin(TRANSCRIPT_JOB_NAME, TRANSCRIPT_TABLE, days, now, executed_by)
    }

    /// PRV-025: the scheduler-history cleanup; it has no opt-out count.
    pub fn history_started(days: RetentionDays, now: Micros, executed_by: &str) -> RetentionRun {
        RetentionRun::begin(HISTORY_JOB_NAME, HISTORY_TABLE, days, now, executed_by)
    }

    fn begin(job_name: &str, table_name: &str, days: RetentionDays, now: Micros, executed_by: &str) -> RetentionRun {
        RetentionRun {
            id: Uuid::new_v4(),
            job_name: job_name.into(),
            table_name: table_name.into(),
            retention_days: days.days(),
            cutoff: days.cutoff(now),
            rows_deleted: 0,
            execution_time_ms: 0,
            status: RunStatus::Running,
            error_message: None,
            users_opted_out_count: None,
            executed_at: now,
            executed_by: executed_by.into(),
        }
    }

    /// PRV-020: another run holds the lock.
    pub fn skipped(mut self) -> RetentionRun {
        self.status = RunStatus::Skipped;
        self.error_message = Some("Cleanup already running (could not acquire advisory lock)".into());
        self.users_opted_out_count = None;
        self
    }

    /// PRV-019: the threshold refused the run; nothing was deleted. `users_opted_out`
    /// is the transcript count; the history cleanup has none.
    pub fn aborted(mut self, would_delete: u64, users_opted_out: Option<u64>) -> RetentionRun {
        self.status = RunStatus::Aborted;
        self.error_message =
            Some(format!("SAFETY THRESHOLD EXCEEDED: Would delete {would_delete} rows (limit: {SAFETY_THRESHOLD})"));
        self.users_opted_out_count = users_opted_out;
        self
    }

    pub fn succeeded(mut self, rows_deleted: u64, users_opted_out: Option<u64>, elapsed_ms: u64) -> RetentionRun {
        self.status = RunStatus::Success;
        self.rows_deleted = rows_deleted;
        self.users_opted_out_count = users_opted_out;
        self.execution_time_ms = elapsed_ms;
        self
    }

    pub fn failed(mut self, error: &str, elapsed_ms: u64) -> RetentionRun {
        self.status = RunStatus::Failure;
        self.rows_deleted = 0;
        self.error_message = Some(error.into());
        self.execution_time_ms = elapsed_ms;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetentionOutcome {
    Success { rows_deleted: u64, users_opted_out: u64 },
    Skipped,
    Aborted { would_delete: u64 },
}

/// The scheduler-history cleanup's result (PRV-025): no opt-out count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryOutcome {
    Success { rows_deleted: u64 },
    Skipped,
    Aborted { would_delete: u64 },
}

impl RetentionOutcome {
    /// The status of the audit row this run wrote, for a slot outcome.
    pub fn status(&self) -> RunStatus {
        match self {
            RetentionOutcome::Success { .. } => RunStatus::Success,
            RetentionOutcome::Skipped => RunStatus::Skipped,
            RetentionOutcome::Aborted { .. } => RunStatus::Aborted,
        }
    }
}

impl HistoryOutcome {
    /// The status of the audit row this run wrote, for a slot outcome.
    pub fn status(&self) -> RunStatus {
        match self {
            HistoryOutcome::Success { .. } => RunStatus::Success,
            HistoryOutcome::Skipped => RunStatus::Skipped,
            HistoryOutcome::Aborted { .. } => RunStatus::Aborted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(job: i64, auto_delete: bool, lines: u64) -> RetentionCandidate {
        RetentionCandidate { account: AccountId(uuid::Uuid::nil()), job: JobId(job), auto_delete, line_count: lines }
    }

    // PRV-018
    #[test]
    fn retention_days_are_validated() {
        assert!(RetentionDays::new(None).unwrap_err().0.starts_with("Parameter validation failed"));
        assert!(RetentionDays::new(Some(6)).is_err());
        assert!(RetentionDays::new(Some(366)).is_err());
        assert_eq!(RetentionDays::new(Some(7)).unwrap().days(), 7);
        assert_eq!(RetentionDays::new(Some(365)).unwrap().days(), 365);
    }

    // PRV-015: the cron's 30 removes jobs 31 days old; exactly 30 days old is kept.
    #[test]
    fn cutoff_adds_one_day() {
        let now = Micros(100 * MICROS_PER_DAY);
        let cutoff = RetentionDays::SCHEDULED.cutoff(now);
        assert_eq!(cutoff, Micros(69 * MICROS_PER_DAY));
        let thirty_days_old = Micros(70 * MICROS_PER_DAY);
        assert!(thirty_days_old.0 > cutoff.0);
    }

    // PRV-016, PRV-019, PRV-023: only explicit opt-in deletes; exactly 100000 is allowed.
    #[test]
    fn plan_counts_only_opted_in_lines() {
        let now = Micros(100 * MICROS_PER_DAY);
        let p = plan(RetentionDays::SCHEDULED, now, &[candidate(1, true, 60_000), candidate(2, false, 50), candidate(3, true, 40_000)], 1);
        assert_eq!(p.estimated_deletions, 100_000);
        assert!(!p.would_exceed_threshold);
        assert_eq!(p.jobs, vec![JobId(1), JobId(3)]);
        let over = plan(RetentionDays::SCHEDULED, now, &[candidate(1, true, 100_001)], 0);
        assert!(over.would_exceed_threshold);
    }

    #[test]
    fn abort_message_matches_the_source() {
        let run = RetentionRun::started(RetentionDays::SCHEDULED, Micros(0), "scheduler").aborted(100_001, Some(2));
        assert_eq!(run.status, RunStatus::Aborted);
        assert_eq!(run.error_message.unwrap(), "SAFETY THRESHOLD EXCEEDED: Would delete 100001 rows (limit: 100000)");
    }

    // PRV-025/031, CRN-007: the history cleanup keeps 90 days plus one and has its own names.
    #[test]
    fn history_runs_have_their_own_audit_identity() {
        let now = Micros(200 * MICROS_PER_DAY);
        let run = RetentionRun::history_started(HISTORY_RETENTION_DAYS, now, "scheduler");
        assert_eq!((run.job_name.as_str(), run.table_name.as_str(), run.retention_days), ("cleanup-cron-job-history", "media.schedule_slots", 90));
        assert_eq!(run.cutoff, Micros(109 * MICROS_PER_DAY));
        assert_eq!(run.succeeded(5, None, 1).users_opted_out_count, None);
    }
}
