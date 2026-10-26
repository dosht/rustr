//! Deciding whether a provider's listed job is the one an attempt submitted (C5,
//! F5). A listing miss is not proof of nonacceptance and a loose match is not
//! proof of acceptance, so a record counts only when everything the attempt sent
//! agrees: provider and project, the `job_<id>` client reference, the exact
//! `attempt` query parameter of its callback URL, the submitted audio object and
//! the creation time inside the claim window. The same rule as taskr's sample.
//! Pure; no clock, no network.

use media_ports::RemoteRecord;
use uuid::Uuid;

use crate::ids::Micros;
use crate::jobs::domain::Attempt;

/// How far a provider's clock may differ from ours around the claim window.
pub const SKEW_MICROS: i64 = 300 * 1_000_000;

/// The value of the single `attempt` query parameter; none or more than one is no value.
pub fn attempt_param(url: &str) -> Option<Uuid> {
    let query = url.split_once('?')?.1.split('#').next()?;
    let mut found = query.split('&').filter_map(|pair| pair.strip_prefix("attempt="));
    let first = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Uuid::parse_str(first).ok()
}

fn path_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let path = &rest[rest.find('/')?..];
    Some(path.split(['?', '#']).next().unwrap_or(path))
}

/// `true` only when every fact the attempt sent agrees with the listed record.
pub fn matches(record: &RemoteRecord, attempt: &Attempt) -> bool {
    if attempt.provider.transport() != Some(record.remote.provider) || record.remote.project != attempt.project {
        return false;
    }
    if record.client_reference.as_deref() != Some(attempt.request.client_reference.as_str()) {
        return false;
    }
    if record.callback_url.as_ref().and_then(|u| attempt_param(u.expose())) != Some(attempt.id.0) {
        return false;
    }
    let suffix = format!("/{}", attempt.request.audio.key);
    if !record.audio_url.as_ref().and_then(|u| path_of(u.expose())).is_some_and(|path| path.ends_with(&suffix)) {
        return false;
    }
    parse_rfc3339(&record.created_at)
        .is_some_and(|at| at.0 >= attempt.claimed_at.0 - SKEW_MICROS && at.0 <= attempt.lease_until.0 + SKEW_MICROS)
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)` to UTC microseconds.
pub fn parse_rfc3339(text: &str) -> Option<Micros> {
    let bytes = text.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = bytes.get(from..from + len)?;
        part.iter().all(u8::is_ascii_digit).then(|| part.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')))
    };
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't' | b' ') || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let (year, month, day, hour, minute, second) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?, digits(11, 2)?, digits(14, 2)?, digits(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut at = 19;
    let mut micros = 0i64;
    if bytes.get(at) == Some(&b'.') {
        let start = at + 1;
        let end = start + bytes[start..].iter().take_while(|b| b.is_ascii_digit()).count();
        if end == start {
            return None;
        }
        let fraction = &text[start..end];
        micros = format!("{:0<6}", &fraction[..fraction.len().min(6)]).parse().ok()?;
        at = end;
    }
    let offset_seconds = match bytes.get(at..)? {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), ..] if bytes.len() == at + 6 && bytes[at + 3] == b':' => {
            let seconds = digits(at + 1, 2)? * 3600 + digits(at + 4, 2)? * 60;
            if *sign == b'+' { seconds } else { -seconds }
        }
        _ => return None,
    };
    let seconds = crate::time::days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    Some(Micros(seconds * 1_000_000 + micros))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attempt_parameter_must_be_single_and_exact() {
        let id = Uuid::from_u128(7);
        assert_eq!(attempt_param(&format!("https://h/cb?job=1&attempt={id}")), Some(id));
        assert_eq!(attempt_param(&format!("https://h/cb?attempt={id}&attempt={id}")), None, "two values are no value");
        assert_eq!(attempt_param(&format!("https://h/cb?xattempt={id}")), None);
        assert_eq!(attempt_param(&format!("https://h/cb?attempt={id}x")), None, "a substring is not the attempt");
        assert_eq!(attempt_param("https://h/cb"), None);
    }

    #[test]
    fn provider_timestamps_are_read_as_utc() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:01Z"), Some(Micros(1_000_000)));
        assert_eq!(parse_rfc3339("1970-01-01T01:00:00.5+01:00"), Some(Micros(500_000)));
        assert_eq!(parse_rfc3339("1970-01-01 00:00:00"), None, "no zone is no instant");
        assert_eq!(parse_rfc3339("1970-13-01T00:00:00Z"), None);
    }
}
