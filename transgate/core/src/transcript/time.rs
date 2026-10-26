//! Line time strings: zero-padded `HH:MM:SS`, floored to the second.

fn hms(total_seconds: i64) -> String {
    let total = total_seconds.max(0);
    format!("{:02}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// Soniox `formatTime` (TRX-026): adds the 500 ms provider offset, then floors.
pub const SONIOX_OFFSET_MS: f64 = 500.0;

pub fn soniox_time(ms: f64) -> String {
    hms(((ms + SONIOX_OFFSET_MS) / 1000.0).floor() as i64)
}

/// Deepgram and caption `formatTime`/`secondsToTimeStr` (TRX-030, TRX-038):
/// float seconds floored, no offset.
pub fn seconds_time(seconds: f64) -> String {
    let h = (seconds / 3600.0).floor() as i64;
    let m = ((seconds % 3600.0) / 60.0).floor() as i64;
    let s = (seconds % 60.0).floor() as i64;
    format!("{h:02}:{m:02}:{s:02}")
}

/// The source `parseTime`: `parseInt` of each part, back to milliseconds.
pub fn parse_hms_ms(text: &str) -> f64 {
    let mut parts = text.split(':').map(js_parse_int);
    let h = parts.next().unwrap_or(f64::NAN);
    let m = parts.next().unwrap_or(f64::NAN);
    let s = parts.next().unwrap_or(f64::NAN);
    (h * 3600.0 + m * 60.0 + s) * 1000.0
}

/// `parseInt(text, 10)`: leading whitespace, optional sign, leading digits;
/// `NaN` when there are none.
pub fn js_parse_int(text: &str) -> f64 {
    let t = text.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, &t[1..]),
        Some(b'+') => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.parse::<f64>().unwrap_or(f64::NAN)
}

/// `parseFloat(text)` on an `HH:MM:SS` string reads only the leading number,
/// that is the hours. The Deepgram word fallback relies on it (JOB-D03, kept
/// as captured; no correction was approved).
pub fn js_parse_float_prefix(text: &str) -> f64 {
    let t = text.trim_start();
    let mut end = 0;
    let bytes = t.as_bytes();
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end == digits_start {
        return f64::NAN;
    }
    t[..end].parse::<f64>().unwrap_or(f64::NAN)
}

/// Seconds of a stored `HH:MM:SS` (`timeToSeconds`), used by exports.
pub fn hms_seconds(text: &str) -> f64 {
    parse_hms_ms(text) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // TRX-026: +500 ms, then floor.
    #[test]
    fn soniox_adds_the_offset() {
        assert_eq!(soniox_time(0.0), "00:00:00");
        assert_eq!(soniox_time(499.0), "00:00:00");
        assert_eq!(soniox_time(500.0), "00:00:01");
        assert_eq!(soniox_time(3_599_600.0), "01:00:00");
    }

    // TRX-030/038: floor, no offset.
    #[test]
    fn seconds_floor_without_offset() {
        assert_eq!(seconds_time(0.99), "00:00:00");
        assert_eq!(seconds_time(125.5), "00:02:05");
        assert_eq!(seconds_time(3661.0), "01:01:01");
    }

    #[test]
    fn parse_float_reads_only_hours() {
        assert_eq!(js_parse_float_prefix("00:01:23"), 0.0);
        assert_eq!(js_parse_float_prefix("01:00:05"), 1.0);
        assert!(js_parse_float_prefix("x").is_nan());
    }

    #[test]
    fn parse_time_round_trips() {
        assert_eq!(parse_hms_ms("01:02:03"), 3_723_000.0);
    }
}
