//! Calendar arithmetic on UTC instants, delegated to the `time` crate.
//!
//! The legacy database evaluated `timestamp + interval 'N months'` in a UTC
//! session: add whole months to the civil date, clamp the day to the end of
//! the target month, keep the time of day. Instants are independent of any
//! session zone here.

use std::fmt;

use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use crate::types::Micros;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interval {
    Month,
    Year,
}

impl Interval {
    /// Only the two lowercase spellings the legacy package metadata used.
    pub fn parse(text: &str) -> Option<Interval> {
        match text {
            "month" => Some(Interval::Month),
            "year" => Some(Interval::Year),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Interval::Month => "month",
            Interval::Year => "year",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarError(pub String);

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "calendar: {}", self.0)
    }
}

impl std::error::Error for CalendarError {}

fn err(e: impl fmt::Display) -> CalendarError {
    CalendarError(e.to_string())
}

fn to_datetime(at: Micros) -> Result<OffsetDateTime, CalendarError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(at.0) * 1000).map_err(err)
}

fn from_datetime(dt: OffsetDateTime) -> Micros {
    Micros((dt.unix_timestamp_nanos() / 1000) as i64)
}

pub fn add_months(at: Micros, months: i64) -> Result<Micros, CalendarError> {
    let dt = to_datetime(at)?;
    let total = i64::from(dt.year()) * 12 + (i64::from(u8::from(dt.month())) - 1) + months;
    let year = i32::try_from(total.div_euclid(12)).map_err(err)?;
    let month = Month::try_from((total.rem_euclid(12) + 1) as u8).map_err(err)?;
    let day = dt.day().min(month.length(year));
    let date = Date::from_calendar_date(year, month, day).map_err(err)?;
    Ok(from_datetime(dt.replace_date(date)))
}

/// A year is twelve months, so Feb 29 plus one year is Feb 28, as in Postgres.
pub fn add_interval(at: Micros, interval: Interval, count: i64) -> Result<Micros, CalendarError> {
    match interval {
        Interval::Month => add_months(at, count),
        Interval::Year => add_months(at, count.checked_mul(12).ok_or_else(|| err("interval overflow"))?),
    }
}

/// Builds an instant from UTC civil parts. Panics on an impossible date: use it
/// for literals in fixtures and tests, not for untrusted input.
pub fn utc(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8, micro: u32) -> Micros {
    let date = Date::from_calendar_date(year, Month::try_from(month).expect("month"), day).expect("date");
    let time = Time::from_hms_micro(hour, minute, second, micro).expect("time");
    from_datetime(PrimitiveDateTime::new(date, time).assume_utc())
}

/// `dd/MM/yyyy` in UTC, the billing history date format (BIL-075).
pub fn format_dmy(at: Micros) -> String {
    match to_datetime(at) {
        Ok(dt) => format!("{:02}/{:02}/{:04}", dt.day(), u8::from(dt.month()), dt.year()),
        Err(_) => String::new(),
    }
}

/// The instant as `YYYY-MM-DDTHH:MM:SS.ffffffZ` in UTC, the timestamp form analytics
/// destinations take. An instant outside the calendar's range yields an empty string.
pub fn format_rfc3339(at: Micros) -> String {
    match to_datetime(at) {
        Ok(dt) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
            dt.year(),
            u8::from(dt.month()),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
            dt.microsecond()
        ),
        Err(_) => String::new(),
    }
}

/// Parses an RFC 3339 timestamp (`2026-01-15T12:30:45.123456+03:00`, `Z` or `z`) into an
/// instant. Fractions beyond microseconds are truncated. Anything that is not exactly
/// that shape, including a missing zone, a leap second or trailing text, is `None`.
pub fn parse_rfc3339(text: &str) -> Option<Micros> {
    let b = text.as_bytes();
    if b.len() < 20 || !text.is_ascii() {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(range)?;
        if part.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(part).ok()?.parse().ok()
        } else {
            None
        }
    };
    if b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut at = 19;
    let mut micro = 0u32;
    if b[at] == b'.' {
        let start = at + 1;
        let mut end = start;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        if end == start || end - start > 9 {
            return None;
        }
        let mut scaled = std::str::from_utf8(&b[start..end.min(start + 6)]).ok()?.to_string();
        while scaled.len() < 6 {
            scaled.push('0');
        }
        micro = scaled.parse().ok()?;
        at = end;
    }
    let offset_secs: i64 = match b.get(at)? {
        b'Z' | b'z' if at + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if at + 6 == b.len() && b[at + 3] == b':' => {
            let (oh, om) = (digits(at + 1..at + 3)?, digits(at + 4..at + 6)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let magnitude = i64::from(oh) * 3600 + i64::from(om) * 60;
            if *sign == b'-' { -magnitude } else { magnitude }
        }
        _ => return None,
    };
    let date = Date::from_calendar_date(i32::try_from(year).ok()?, Month::try_from(u8::try_from(month).ok()?).ok()?, u8::try_from(day).ok()?).ok()?;
    let time = Time::from_hms_micro(u8::try_from(hour).ok()?, u8::try_from(minute).ok()?, u8::try_from(second).ok()?, micro).ok()?;
    let local = from_datetime(PrimitiveDateTime::new(date, time).assume_utc());
    Some(Micros(local.0 - offset_secs * Micros::PER_SECOND))
}

#[cfg(test)]
mod tests;
