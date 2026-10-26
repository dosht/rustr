//! The source's date texts, computed in core so no shell picks a locale or
//! time-zone policy. Times are UTC microseconds shown at the browser's UTC
//! offset (`-new Date().getTimezoneOffset()`, sent once at start).

use crate::ids::Micros;

const MICROS_PER_MINUTE: i64 = 60_000_000;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Local civil date and time: (year, month 1..=12, day, hour, minute).
fn local(at: Micros, utc_offset_minutes: i16) -> (i64, u32, u32, u32, u32) {
    let minutes = at.0.div_euclid(MICROS_PER_MINUTE) + i64::from(utc_offset_minutes);
    let days = minutes.div_euclid(24 * 60);
    let minute_of_day = minutes.rem_euclid(24 * 60);
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, (minute_of_day / 60) as u32, (minute_of_day % 60) as u32)
}

/// `formatDate` (`utils/formats.ts`): local `d-m-yyyy`, no padding.
pub fn day_month_year(at: Micros, utc_offset_minutes: i16) -> String {
    let (y, m, d, _, _) = local(at, utc_offset_minutes);
    format!("{d}-{m}-{y}")
}

/// `formatJobDate`: `toLocaleDateString("en-US", {year numeric, month short,
/// day numeric, hour 2-digit, minute 2-digit})`, the same English text in
/// every locale, e.g. `Jan 5, 2026, 03:04 PM`.
pub fn job_date(at: Micros, utc_offset_minutes: i16) -> String {
    let (y, m, d, h, min) = local(at, utc_offset_minutes);
    let (h12, half) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{} {d}, {y}, {h12:02}:{min:02} {half}", MONTHS[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-01-05T15:04:00Z
    const AT: Micros = Micros(1_767_625_440_000_000);

    #[test]
    fn dates_follow_the_source_formats_at_the_local_offset() {
        assert_eq!(job_date(AT, 0), "Jan 5, 2026, 03:04 PM");
        assert_eq!(job_date(AT, 180), "Jan 5, 2026, 06:04 PM");
        assert_eq!(job_date(AT, -16 * 60), "Jan 4, 2026, 11:04 PM");
        assert_eq!(job_date(Micros(1_767_225_600_000_000), 0), "Jan 1, 2026, 12:00 AM");
        assert_eq!(day_month_year(AT, 0), "5-1-2026");
        assert_eq!(day_month_year(Micros(951_782_400_000_000), 0), "29-2-2000", "a leap day");
        assert_eq!(day_month_year(Micros(-86_400_000_000), 0), "31-12-1969");
    }
}
