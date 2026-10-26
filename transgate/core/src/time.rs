//! Calendar arithmetic over UTC microseconds, without a time-zone database.
//!
//! The source computes some boundaries in the browser's local time (the job
//! list's one-month window uses `Date.setMonth`, PRV-032). Shells supply the
//! UTC offsets for that computation; nothing here assumes UTC wall time.

use crate::ids::{MICROS_PER_DAY, MICROS_PER_SECOND, Micros};

/// Days since 1970-01-01 to a proleptic Gregorian (year, month 1..=12, day).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month 1..=12, day) to days since 1970-01-01. Day and month may
/// overflow; the result normalizes like JavaScript's `Date`.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let total_months = year * 12 + (month - 1);
    let y = total_months.div_euclid(12);
    let m = total_months.rem_euclid(12) + 1;
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + (day - 1)
}

/// `Date.toISOString()`: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso8601(at: Micros) -> String {
    let days = at.0.div_euclid(MICROS_PER_DAY);
    let rem = at.0.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let secs = rem / MICROS_PER_SECOND;
    let millis = (rem % MICROS_PER_SECOND) / 1000;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The calendar date of an instant: (year, month, day).
pub fn utc_date(at: Micros) -> (i64, u32, u32) {
    civil_from_days(at.0.div_euclid(MICROS_PER_DAY))
}

/// Day of week of an instant in UTC, 0 = Sunday.
pub fn utc_weekday(at: Micros) -> u32 {
    // 1970-01-01 was a Thursday.
    ((at.0.div_euclid(MICROS_PER_DAY) + 4).rem_euclid(7)) as u32
}

/// `d.setMonth(d.getMonth() - 1)` evaluated in a local zone.
///
/// `offset_now_minutes` is the zone's offset from UTC at `now` (east positive);
/// `offset_then_minutes` the offset at the resulting local time. A browser with
/// daylight saving between the two supplies different values; local wall time
/// is kept, as `setMonth` keeps it. Month-end overflow follows JavaScript:
/// March 31 minus one month is March 3 (or 2 in a leap year).
pub fn minus_one_calendar_month_local(now: Micros, offset_now_minutes: i32, offset_then_minutes: i32) -> Micros {
    let local = now.0 + i64::from(offset_now_minutes) * 60 * MICROS_PER_SECOND;
    let days = local.div_euclid(MICROS_PER_DAY);
    let time_of_day = local.rem_euclid(MICROS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let shifted_days = days_from_civil(y, i64::from(m) - 1, i64::from(d));
    let local_then = shifted_days * MICROS_PER_DAY + time_of_day;
    Micros(local_then - i64::from(offset_then_minutes) * 60 * MICROS_PER_SECOND)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i64, m: i64, d: i64, hh: i64, mm: i64, ss: i64) -> Micros {
        Micros(days_from_civil(y, m, d) * MICROS_PER_DAY + (hh * 3600 + mm * 60 + ss) * MICROS_PER_SECOND)
    }

    #[test]
    fn iso_formats_like_javascript() {
        assert_eq!(iso8601(at(2026, 10, 24, 9, 5, 7)), "2026-10-24T09:05:07.000Z");
        assert_eq!(iso8601(Micros(1_500)), "1970-01-01T00:00:00.001Z");
    }

    // PRV-032: setMonth overflow and local time.
    #[test]
    fn one_month_back_overflows_like_set_month() {
        assert_eq!(minus_one_calendar_month_local(at(2026, 3, 31, 12, 0, 0), 0, 0), at(2026, 3, 3, 12, 0, 0));
        assert_eq!(minus_one_calendar_month_local(at(2024, 3, 31, 12, 0, 0), 0, 0), at(2024, 3, 2, 12, 0, 0));
        assert_eq!(minus_one_calendar_month_local(at(2026, 1, 15, 0, 0, 0), 0, 0), at(2025, 12, 15, 0, 0, 0));
        // 00:30 local in UTC+3 on Nov 1 is Oct 31 21:30 UTC; one month back is Oct 1 00:30 local.
        assert_eq!(minus_one_calendar_month_local(at(2026, 10, 31, 21, 30, 0), 180, 180), at(2026, 9, 30, 21, 30, 0));
        // A daylight-saving change keeps local wall time: offsets differ by an hour.
        assert_eq!(minus_one_calendar_month_local(at(2026, 11, 15, 12, 0, 0), 60, 120), at(2026, 10, 15, 11, 0, 0));
    }

    #[test]
    fn weekday_of_epoch_is_thursday() {
        assert_eq!(utc_weekday(Micros(0)), 4);
        assert_eq!(utc_weekday(at(2026, 10, 25, 4, 0, 0)), 0);
    }
}
