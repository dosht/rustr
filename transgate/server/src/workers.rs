//! The background work: core's schedules, each claimed once per UTC slot through media-postgres (so
//! replicas never repeat a slot and an interrupted claim is reclaimed when its lease ends), and the
//! delivery of job event mails and ordered support mails from transactional outboxes.
//!
//! The passes themselves are core's (`Jobs::*_pass`, `Privacy::*`); this module only decides when
//! to call them and records each run's outcome.

mod support;

use std::sync::Arc;
use std::time::Duration;

use billing_core::ports::{CustomerDirectory, Mailer};
use billing_core::types::Clock;
use media_postgres::SlotClaim;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use transgate_billing::directory::PgCustomerDirectory;
use transgate_core::jobs::app::Jobs;
use transgate_core::jobs::events::{JobEvent, mails_for};
use transgate_core::jobs::ports::Locks;
use transgate_core::privacy::app::Privacy;
use transgate_core::privacy::retention::{HISTORY_RETENTION_DAYS, HistoryOutcome, RetentionDays, RetentionOutcome};
use transgate_core::schedule::Schedule;
use transgate_core::{AccountId, Micros};
use transgate_jobs_postgres::{ClaimedEvent, PgJobStore};

use crate::mail::{self, Owner};

/// How long a claimed slot may run before another replica may reclaim it.
const SLOT_LEASE_SECONDS: i64 = 600;
/// How long a claimed event may be held while its mails are sent.
const EVENT_LEASE_SECONDS: i64 = 120;
const EVENT_BATCH: i64 = 20;
/// The transcript retention the schedule runs with (CRN-005, PRV-029).
const TRANSCRIPT_RETENTION_DAYS: i64 = 30;

pub struct Workers {
    pub jobs: Arc<Jobs>,
    pub privacy: Arc<Privacy>,
    pub ai: Arc<transgate_core::ai::Ai>,
    pub store: PgJobStore,
    pub pool: PgPool,
    pub mailer: Arc<dyn Mailer>,
    pub directory: PgCustomerDirectory,
    pub clock: Arc<dyn Clock>,
    pub locks: Arc<dyn Locks>,
}

fn event_account(event: &JobEvent) -> AccountId {
    match event {
        JobEvent::Completed { account, .. } | JobEvent::Failed { account, .. } => *account,
    }
}

/// Seconds to wait before the next try of an event: a minute, doubling, at most 32 minutes.
pub fn retry_delay_seconds(attempts: i32) -> i64 {
    60_i64 << (attempts.max(1) - 1).min(5)
}

impl Workers {
    /// Independent from the mail/media loop: slow/failed AI cannot hold completion mail.
    pub async fn ai_tick(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if let Err(e) = self.ai.run_pass().await { errors.push(format!("AI recovery: {e}")); }
        if let Err(e) = self.ai.summary_pass().await { errors.push(format!("AI completion summary: {e}")); }
        if let Err(e) = self.ai.purge_pass().await { errors.push(format!("AI purge: {e}")); }
        errors
    }

    /// One tick: every schedule, then the event mails.
    pub async fn tick(&self) -> Vec<String> {
        let mut errors = self.run_schedules(&Schedule::ALL).await;
        if let Err(e) = self.deliver_events().await {
            errors.push(format!("event mail: {e}"));
        }
        errors
    }

    /// Every listed schedule whose slot is unclaimed. A pass runs only while its schedule's
    /// advisory lock is held, taken before the slot is claimed: a slot claim fences one slot, but
    /// the Jobs slot (180 s) is shorter than its lease and a pass can outlast it, so two replicas
    /// would otherwise run the same pass at once (and spend a provider budget twice). A held lock
    /// leaves the slot unclaimed; a later tick takes it once the pass ends.
    async fn run_schedules(&self, schedules: &[Schedule]) -> Vec<String> {
        let mut errors = Vec::new();
        for &schedule in schedules {
            let guard = match self.locks.try_lock(schedule.name()).await {
                Ok(Some(guard)) => guard,
                Ok(None) => continue,
                Err(e) => {
                    errors.push(format!("{}: {e}", schedule.name()));
                    continue;
                }
            };
            let now = self.clock.now();
            let slot = schedule.slot(now);
            match media_postgres::claim_slot(&self.pool, schedule.name(), slot.0, now.0, SLOT_LEASE_SECONDS).await {
                Ok(Some(claim)) => {
                    let failures = self.run(schedule).await;
                    let outcome = if failures.is_empty() { "succeeded" } else { "failed" };
                    errors.extend(failures.into_iter().map(|f| format!("{}: {f}", schedule.name())));
                    self.finish(&claim, outcome, &mut errors).await;
                }
                Ok(None) => {}
                Err(e) => errors.push(format!("{}: {e}", schedule.name())),
            }
            guard.release().await;
        }
        errors
    }

    async fn finish(&self, claim: &SlotClaim, outcome: &str, errors: &mut Vec<String>) {
        if let Err(e) = media_postgres::finish_slot(&self.pool, claim, self.clock.now().0, outcome).await {
            errors.push(format!("{}: {e}", claim.schedule));
        }
    }

    async fn run(&self, schedule: Schedule) -> Vec<String> {
        let mut failures = Vec::new();
        let mut note = |step: &str, result: Result<(), String>| {
            if let Err(e) = result {
                failures.push(format!("{step}: {e}"));
            }
        };
        match schedule {
            Schedule::Jobs => {
                note("dispatch", self.jobs.dispatch_pass().await.map(|_| ()).map_err(|e| e.to_string()));
                note("reconcile", self.jobs.reconcile_pass().await.map(|_| ()).map_err(|e| e.to_string()));
                note("stuck", self.jobs.stuck_pass().await.map(|_| ()).map_err(|e| e.to_string()));
                note("ingest", self.jobs.ingest_pass().await.map(|_| ()).map_err(|e| e.to_string()));
            }
            Schedule::RemoteCleanup => {
                note("remote cleanup", self.jobs.remote_cleanup_pass().await.map(|_| ()).map_err(|e| e.to_string()));
                note("erasure", self.privacy.erasure_pass().await.map(|_| ()).map_err(|e| e.to_string()));
            }
            Schedule::TranscriptRetention => {
                let result = match RetentionDays::new(Some(TRANSCRIPT_RETENTION_DAYS)) {
                    Ok(days) => match self.privacy.retention_run(days, "scheduler").await {
                        Ok(RetentionOutcome::Success { .. } | RetentionOutcome::Skipped) => Ok(()),
                        Ok(RetentionOutcome::Aborted { would_delete }) => Err(format!("aborted: it would delete {would_delete} rows")),
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                note("retention", result);
            }
            Schedule::HistoryRetention => {
                let result = match self.privacy.history_retention_run(HISTORY_RETENTION_DAYS, "scheduler").await {
                    Ok(HistoryOutcome::Success { .. } | HistoryOutcome::Skipped) => Ok(()),
                    Ok(HistoryOutcome::Aborted { would_delete }) => Err(format!("aborted: it would delete {would_delete} rows")),
                    Err(e) => Err(e.to_string()),
                };
                note("history retention", result);
            }
        }
        failures
    }

    /// Sends the mails of each undelivered event. An event is acknowledged only when every mail it
    /// derives was sent; otherwise it is released to be tried again later. Every mail carries a
    /// stable idempotency key, so a retry after a partial failure does not send what was sent.
    pub async fn deliver_events(&self) -> Result<usize, String> {
        let now = self.clock.now();
        let claimed = self.store.claim_events(EVENT_BATCH, EVENT_LEASE_SECONDS, now).await.map_err(|e| e.to_string())?;
        let count = claimed.len();
        for event in claimed {
            match self.deliver(&event).await {
                Ok(()) => {
                    self.store.ack_event(&event, self.clock.now()).await.map_err(|e| e.to_string())?;
                }
                Err(why) => {
                    let retry_at = Micros(self.clock.now().0 + retry_delay_seconds(event.attempts) * 1_000_000);
                    self.store.release_event(&event, &why, retry_at).await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(count)
    }

    async fn deliver(&self, claimed: &ClaimedEvent) -> Result<(), String> {
        let account = event_account(&claimed.event);
        let owner = match self.directory.by_account(account).await {
            Ok(Some(customer)) => Owner { email: Some(customer.email), full_name: customer.full_name },
            // An owner the directory no longer holds has no address: the user mail is skipped.
            Ok(None) => Owner::default(),
            Err(e) => return Err(format!("the customer directory could not answer: {e}")),
        };
        for mail in mails_for(&claimed.event) {
            let Some(message) = mail::render(&mail, claimed.event.job(), account, &owner) else { continue };
            self.mailer.send(&message, &mail::idempotency_key(claimed.id, &mail)).await.map_err(|e| e.0)?;
        }
        Ok(())
    }
}

/// Ticks every `every` until aborted; a tick's errors are printed and never stop the loop.
pub fn spawn(workers: Arc<Workers>, every: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let media = async {
            let mut interval = tokio::time::interval(every);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                for error in workers.tick().await { eprintln!("transgate: {error}"); }
            }
        };
        let ai = async {
            let mut interval = tokio::time::interval(every);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                for error in workers.ai_tick().await { eprintln!("transgate: {error}"); }
            }
        };
        let support = async {
            let mut interval = tokio::time::interval(every);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Err(error) = workers.deliver_support().await { eprintln!("transgate: support mail: {error}"); }
            }
        };
        // All futures belong to this task: abort/shutdown drops them together. A held support
        // provider never holds the media scheduler, job mail, or AI recovery loop.
        tokio::join!(media, ai, support);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_retried_after_a_minute_then_longer_and_never_beyond_an_hour() {
        let delays: Vec<i64> = (1..=9).map(retry_delay_seconds).collect();
        assert_eq!(delays, vec![60, 120, 240, 480, 960, 1920, 1920, 1920, 1920]);
        assert_eq!(retry_delay_seconds(0), 60, "an unclaimed count still waits the first minute");
        assert!(delays.iter().all(|d| *d <= 3600));
    }
}

#[cfg(test)]
mod pg_tests;
