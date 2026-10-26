//! The product's schedules (CRN-002/003/005/007) and their UTC slots. The host
//! claims each slot through media-postgres (one durable claim per slot, an
//! advisory lock against overlap) and calls the named pass. Interrupted claims
//! are reclaimed by the host once their lease expires, so a slot never strands.

use serde::{Deserialize, Serialize};

use crate::ids::{MICROS_PER_DAY, MICROS_PER_MINUTE, Micros};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    /// CRN-002/003: dispatch, reconcile, stuck recovery and inbox retries, every three minutes.
    Jobs,
    /// Provider-copy and erasure remote work, every ten minutes.
    RemoteCleanup,
    /// CRN-005, PRV-029: transcript cleanup at 02:00 UTC with 30 days.
    TranscriptRetention,
    /// CRN-007, PRV-025/031 (adapted): Sunday 04:00 UTC, keep 90 days of the
    /// scheduler's own run history.
    HistoryRetention,
}

impl Schedule {
    pub const ALL: [Schedule; 4] =
        [Schedule::Jobs, Schedule::RemoteCleanup, Schedule::TranscriptRetention, Schedule::HistoryRetention];

    pub fn name(self) -> &'static str {
        match self {
            Schedule::Jobs => "jobs",
            Schedule::RemoteCleanup => "remote-cleanup",
            Schedule::TranscriptRetention => "cleanup-old-transcripts",
            Schedule::HistoryRetention => "cleanup-cron-job-history",
        }
    }

    /// The slot an instant belongs to: the most recent scheduled instant at or before `now`.
    pub fn slot(self, now: Micros) -> Micros {
        match self {
            Schedule::Jobs => floor_to(now, 3 * MICROS_PER_MINUTE),
            Schedule::RemoteCleanup => floor_to(now, 10 * MICROS_PER_MINUTE),
            Schedule::TranscriptRetention => {
                let day = floor_to(now, MICROS_PER_DAY);
                let at = Micros(day.0 + 2 * 60 * MICROS_PER_MINUTE);
                if now.0 >= at.0 { at } else { Micros(at.0 - MICROS_PER_DAY) }
            }
            Schedule::HistoryRetention => {
                let day = floor_to(now, MICROS_PER_DAY);
                let weekday = crate::time::utc_weekday(day);
                let sunday = Micros(day.0 - i64::from(weekday) * MICROS_PER_DAY);
                let at = Micros(sunday.0 + 4 * 60 * MICROS_PER_MINUTE);
                if now.0 >= at.0 { at } else { Micros(at.0 - 7 * MICROS_PER_DAY) }
            }
        }
    }

    /// The following slot, for sleeping until it.
    pub fn next_slot(self, now: Micros) -> Micros {
        let current = self.slot(now);
        let step = match self {
            Schedule::Jobs => 3 * MICROS_PER_MINUTE,
            Schedule::RemoteCleanup => 10 * MICROS_PER_MINUTE,
            Schedule::TranscriptRetention => MICROS_PER_DAY,
            Schedule::HistoryRetention => 7 * MICROS_PER_DAY,
        };
        Micros(current.0 + step)
    }
}

/// CRN-007/PRV-025: history older than `now - 90 - 1 day` is removed.
pub fn history_cutoff(now: Micros) -> Micros {
    crate::privacy::retention::HISTORY_RETENTION_DAYS.cutoff(now)
}

fn floor_to(at: Micros, step: i64) -> Micros {
    Micros(at.0.div_euclid(step) * step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::days_from_civil;

    fn at(y: i64, m: i64, d: i64, hh: i64, mm: i64) -> Micros {
        Micros(days_from_civil(y, m, d) * MICROS_PER_DAY + (hh * 60 + mm) * MICROS_PER_MINUTE)
    }

    // CRN-005: 02:00 UTC daily; before 02:00 the slot is yesterday's.
    #[test]
    fn retention_slot_is_two_am_utc() {
        assert_eq!(Schedule::TranscriptRetention.slot(at(2026, 10, 24, 2, 0)), at(2026, 10, 24, 2, 0));
        assert_eq!(Schedule::TranscriptRetention.slot(at(2026, 10, 24, 1, 59)), at(2026, 10, 23, 2, 0));
    }

    // CRN-007: Sunday 04:00 UTC.
    #[test]
    fn history_slot_is_sunday_four_am() {
        // 2026-10-25 is a Sunday.
        assert_eq!(Schedule::HistoryRetention.slot(at(2026, 10, 27, 12, 0)), at(2026, 10, 25, 4, 0));
        assert_eq!(Schedule::HistoryRetention.slot(at(2026, 10, 25, 3, 0)), at(2026, 10, 18, 4, 0));
    }

    // CRN-002/003: every three minutes.
    #[test]
    fn jobs_slot_is_three_minutes() {
        assert_eq!(Schedule::Jobs.slot(at(2026, 10, 24, 10, 5)), at(2026, 10, 24, 10, 3));
        assert_eq!(Schedule::Jobs.next_slot(at(2026, 10, 24, 10, 5)), at(2026, 10, 24, 10, 6));
    }
}
