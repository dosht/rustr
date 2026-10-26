//! Per-key rate limits (RTL-001/003, D3, API-D03/D04). Two calendar-aligned
//! UTC windows, a minute and an hour, as the source's `date_trunc` (`remote_schema.sql:306-307`).
//! The store consumes one unit in **both** windows only when **both** allow,
//! in one statement; a denied request counts nowhere. That replaces the
//! source's shared `(key, window_start)` counter and read-then-increment race
//! (API-D03). A limiter store error is 503, never fail-open (D3, RTL-002).

use serde::{Deserialize, Serialize};

use crate::ids::Micros;

/// RTL-001 defaults.
pub const PER_MINUTE: u32 = 60;
pub const PER_HOUR: u32 = 1000;

const MICROS_PER_MINUTE: i64 = 60_000_000;
const MICROS_PER_HOUR: i64 = 60 * MICROS_PER_MINUTE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimits {
    pub per_minute: u32,
    pub per_hour: u32,
}

impl Default for RateLimits {
    fn default() -> Self {
        RateLimits { per_minute: PER_MINUTE, per_hour: PER_HOUR }
    }
}

/// The windows an instant falls in: `date_trunc('minute'|'hour', now)` in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateWindows {
    pub minute_start: Micros,
    pub hour_start: Micros,
}

impl RateWindows {
    pub fn at(now: Micros) -> RateWindows {
        RateWindows {
            minute_start: Micros(now.0 - now.0.rem_euclid(MICROS_PER_MINUTE)),
            hour_start: Micros(now.0 - now.0.rem_euclid(MICROS_PER_HOUR)),
        }
    }

    pub fn minute_reset(&self) -> Micros {
        Micros(self.minute_start.0 + MICROS_PER_MINUTE)
    }

    pub fn hour_reset(&self) -> Micros {
        Micros(self.hour_start.0 + MICROS_PER_HOUR)
    }
}

/// What the store reports: the window counts after the call, and whether this
/// request was counted (both allowed). A denied call reports the unchanged counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateCounts {
    pub allowed: bool,
    pub minute: u32,
    pub hour: u32,
}

/// The decision with everything the RTL-003 headers carry
/// (`X-RateLimit-{Limit,Remaining,Reset}-{Minute,Hour}`), on success and on 429.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiRate {
    pub allowed: bool,
    pub limits: RateLimits,
    pub remaining_minute: u32,
    pub remaining_hour: u32,
    pub reset_minute: Micros,
    pub reset_hour: Micros,
}

/// Remaining is the limit minus the window's count, never below zero.
pub fn decide(windows: RateWindows, limits: RateLimits, counts: RateCounts) -> ApiRate {
    ApiRate {
        allowed: counts.allowed,
        limits,
        remaining_minute: limits.per_minute.saturating_sub(counts.minute),
        remaining_hour: limits.per_hour.saturating_sub(counts.hour),
        reset_minute: windows.minute_reset(),
        reset_hour: windows.hour_reset(),
    }
}

/// The reference semantics every limiter store must have (D3), used by the
/// in-memory fake: consume one unit in both windows only when both allow.
pub fn consume(limits: RateLimits, minute: u32, hour: u32) -> RateCounts {
    if minute < limits.per_minute && hour < limits.per_hour {
        RateCounts { allowed: true, minute: minute + 1, hour: hour + 1 }
    } else {
        RateCounts { allowed: false, minute, hour }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::iso8601;

    const NOW: Micros = Micros(1_767_225_600_000_000 + 59 * 60_000_000 + 59_999_999); // 00:59:59.999999

    // RTL-001, D3: calendar-aligned windows, not first-hit windows.
    #[test]
    fn rtl_001_windows_are_calendar_aligned_minute_and_hour() {
        let w = RateWindows::at(NOW);
        assert_eq!(iso8601(w.minute_start), "2026-01-01T00:59:00.000Z");
        assert_eq!(iso8601(w.hour_start), "2026-01-01T00:00:00.000Z");
        assert_eq!(iso8601(w.minute_reset()), "2026-01-01T01:00:00.000Z");
        assert_eq!(iso8601(w.hour_reset()), "2026-01-01T01:00:00.000Z");
        let next = RateWindows::at(Micros(NOW.0 + 1));
        assert_eq!((iso8601(next.minute_start), iso8601(next.hour_start)), ("2026-01-01T01:00:00.000Z".into(), "2026-01-01T01:00:00.000Z".into()));
    }

    // API-D03: one request costs one unit in each window; both are consumed only when
    // both allow; a denied request counts nowhere (so the hour count is not doubled).
    #[test]
    fn api_d03_both_windows_or_neither_and_denials_do_not_count() {
        let limits = RateLimits::default();
        assert_eq!(consume(limits, 0, 0), RateCounts { allowed: true, minute: 1, hour: 1 });
        assert_eq!(consume(limits, 59, 500), RateCounts { allowed: true, minute: 60, hour: 501 });
        assert_eq!(consume(limits, 60, 500), RateCounts { allowed: false, minute: 60, hour: 500 }, "minute full: hour untouched");
        assert_eq!(consume(limits, 3, 1000), RateCounts { allowed: false, minute: 3, hour: 1000 }, "hour full: minute untouched");
    }

    // RTL-003, API-D04: the header values on success and on denial.
    #[test]
    fn rtl_003_remaining_and_resets_for_the_headers() {
        let w = RateWindows::at(NOW);
        let limits = RateLimits::default();
        let ok = decide(w, limits, RateCounts { allowed: true, minute: 60, hour: 501 });
        assert_eq!((ok.allowed, ok.remaining_minute, ok.remaining_hour), (true, 0, 499));
        assert_eq!((ok.reset_minute, ok.reset_hour), (w.minute_reset(), w.hour_reset()));
        let denied = decide(w, limits, RateCounts { allowed: false, minute: 60, hour: 500 });
        assert_eq!((denied.allowed, denied.remaining_minute, denied.remaining_hour), (false, 0, 500));
        let over = decide(w, RateLimits { per_minute: 2, per_hour: 3 }, RateCounts { allowed: false, minute: 5, hour: 9 });
        assert_eq!((over.remaining_minute, over.remaining_hour), (0, 0), "never negative after a limit decrease");
    }
}
