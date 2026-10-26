//! One JSON object per line, like the source's standalone logger
//! (`src/lib/logger.ts`): errors on stderr, everything else on stdout.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub fn info(message: &str, meta: Value) {
    write(Level::Info, message, meta, None);
}

pub fn warn(message: &str, meta: Value) {
    write(Level::Warn, message, meta, None);
}

pub fn error(message: &str, meta: Value, err: &dyn std::fmt::Display) {
    write(Level::Error, message, meta, Some(err.to_string()));
}

fn write(level: Level, message: &str, meta: Value, err: Option<String>) {
    let mut entry = Map::new();
    entry.insert(
        "level".into(),
        json!(match level {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }),
    );
    entry.insert("message".into(), json!(message));
    entry.insert("time".into(), json!(iso8601(SystemTime::now())));
    if let Value::Object(fields) = meta {
        entry.extend(fields);
    }
    if let Some(err) = err {
        entry.insert("error".into(), json!(err));
    }
    let line = Value::Object(entry).to_string();
    match level {
        Level::Error | Level::Warn => eprintln!("{line}"),
        Level::Info => println!("{line}"),
    }
}

/// Civil UTC date and time of an instant: (year, month, day, hour, minute, second, millis).
pub fn civil(at: SystemTime) -> (i64, u32, u32, u32, u32, u32, u32) {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    // Howard Hinnant's days-from-civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rem / 3600, rem / 60 % 60, rem % 60, since.subsec_millis())
}

pub fn iso8601(at: SystemTime) -> String {
    let (y, mo, d, h, mi, s, ms) = civil(at);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn civil_time_matches_known_instants() {
        assert_eq!(iso8601(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2024-02-29T12:34:56.789Z
        assert_eq!(iso8601(UNIX_EPOCH + Duration::from_millis(1_709_210_096_789)), "2024-02-29T12:34:56.789Z");
    }
}
