//! Deciding whether a provider's listed job is the one an attempt submitted. A
//! listing miss is not proof of nonacceptance and a loose match is not proof of
//! acceptance, so a record counts only when everything the attempt sent agrees:
//! provider and project, the `work_<id>` reference, the exact `attempt` query
//! parameter of its callback URL, the submitted audio object and the creation
//! time inside the claim window. Pure; no clock, no network.

use media_ports::{Provider, RemoteRecord};
use uuid::Uuid;

use crate::work::Micros;

/// How far a provider's clock may differ from ours around the claim window.
pub const SKEW_MICROS: i64 = 300 * 1_000_000;

/// What one attempt sent, kept in the manifest as well so erasure can correlate after the rows are gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expected {
    pub attempt: Uuid,
    pub provider: Provider,
    pub project: String,
    pub reference: String,
    /// The storage key of the audio object the signed link named.
    pub object_key: String,
    pub claimed_at: Micros,
    pub lease_until: Micros,
}

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

/// `true` only when every fact agrees.
pub fn matches(record: &RemoteRecord, expected: &Expected) -> bool {
    if record.remote.provider != expected.provider || record.remote.project != expected.project {
        return false;
    }
    if record.client_reference.as_deref() != Some(expected.reference.as_str()) {
        return false;
    }
    if record.callback_url.as_ref().and_then(|u| attempt_param(u.expose())) != Some(expected.attempt) {
        return false;
    }
    let audio_ok = record.audio_url.as_ref().and_then(|u| path_of(u.expose())).is_some_and(|path| path.ends_with(&format!("/{}", expected.object_key)));
    if !audio_ok {
        return false;
    }
    parse_rfc3339(&record.created_at).is_some_and(|at| at.0 >= expected.claimed_at.0 - SKEW_MICROS && at.0 <= expected.lease_until.0 + SKEW_MICROS)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    Some(Micros(seconds * 1_000_000 + micros))
}

/// UTC with milliseconds, the shape providers report.
pub fn format_rfc3339(at: Micros) -> String {
    let millis = at.0.div_euclid(1000);
    let (days, ms_of_day) = (millis.div_euclid(86_400_000), millis.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", ms_of_day / 3_600_000, ms_of_day / 60_000 % 60, ms_of_day / 1000 % 60, ms_of_day % 1000)
}

#[cfg(test)]
mod tests {
    use media_ports::{RemoteIdentity, SensitiveUrl};

    use super::*;

    const SEC: i64 = 1_000_000;
    const CLAIMED: Micros = Micros(1_800_000_000 * SEC);

    fn attempt() -> Uuid {
        Uuid::from_u128(0xa77e)
    }

    fn expected() -> Expected {
        Expected {
            attempt: attempt(),
            provider: Provider::Soniox,
            project: "proj".into(),
            reference: "work_1".into(),
            object_key: "work/acct/work/clip.mp3".into(),
            claimed_at: CLAIMED,
            lease_until: Micros(CLAIMED.0 + 120 * SEC),
        }
    }

    fn record() -> RemoteRecord {
        RemoteRecord {
            remote: RemoteIdentity { provider: Provider::Soniox, project: "proj".into(), id: "r1".into() },
            status: "queued".into(),
            model: "stt-async-v5".into(),
            client_reference: Some("work_1".into()),
            audio_url: Some(SensitiveUrl::new("https://bucket.example.test/work/acct/work/clip.mp3?X-Sig=abc")),
            callback_url: Some(SensitiveUrl::new(format!("https://taskr.example.test/api/audio-work/callback/soniox?work=1&attempt={}", attempt()))),
            created_at: format_rfc3339(Micros(CLAIMED.0 + 3 * SEC)),
        }
    }

    #[test]
    fn a_record_matches_only_when_every_fact_the_attempt_sent_agrees() {
        assert!(matches(&record(), &expected()));

        let mut other = record();
        other.remote.project = "elsewhere".into();
        assert!(!matches(&other, &expected()), "another project");
        let mut other = record();
        other.remote.provider = Provider::Deepgram;
        assert!(!matches(&other, &expected()), "another provider");
        let mut other = record();
        other.client_reference = Some("work_2".into());
        assert!(!matches(&other, &expected()), "another work");
        let mut other = record();
        other.client_reference = None;
        assert!(!matches(&other, &expected()), "no reference at all");
        let mut other = record();
        other.callback_url = Some(SensitiveUrl::new("https://taskr.example.test/cb?attempt=00000000-0000-0000-0000-000000000009"));
        assert!(!matches(&other, &expected()), "another attempt of the same work");
        let mut other = record();
        other.callback_url = None;
        assert!(!matches(&other, &expected()), "no callback URL");
        let mut other = record();
        other.audio_url = Some(SensitiveUrl::new("https://bucket.example.test/work/acct/work/other.mp3?X-Sig=abc"));
        assert!(!matches(&other, &expected()), "another audio object");
        let mut other = record();
        other.audio_url = None;
        assert!(!matches(&other, &expected()), "no audio URL");
    }

    #[test]
    fn the_attempt_must_be_the_one_exact_query_parameter_not_a_substring() {
        let id = attempt();
        assert_eq!(attempt_param(&format!("https://a.test/cb?work=1&attempt={id}")), Some(id));
        assert_eq!(attempt_param(&format!("https://a.test/cb?work=1&attempt={id}#frag")), Some(id));
        assert_eq!(attempt_param(&format!("https://a.test/cb?xattempt={id}")), None, "a longer parameter name is not it");
        assert_eq!(attempt_param(&format!("https://a.test/cb?note=attempt={id}")), None, "a value that merely contains it is not it");
        assert_eq!(attempt_param(&format!("https://a.test/cb?attempt={id}&attempt={id}")), None, "two values are ambiguous");
        assert_eq!(attempt_param("https://a.test/cb?attempt=not-a-uuid"), None);
        assert_eq!(attempt_param("https://a.test/cb"), None);
    }

    #[test]
    fn the_creation_time_must_fall_inside_the_claim_window_with_clock_skew() {
        let at = |micros: i64| {
            let mut r = record();
            r.created_at = format_rfc3339(Micros(micros));
            matches(&r, &expected())
        };
        let (claimed, lease) = (expected().claimed_at.0, expected().lease_until.0);
        assert!(at(claimed - SKEW_MICROS + 1000));
        assert!(!at(claimed - SKEW_MICROS - 1_000_000));
        assert!(at(lease + SKEW_MICROS - 1000));
        assert!(!at(lease + SKEW_MICROS + 1_000_000));
        let mut unreadable = record();
        unreadable.created_at = "yesterday".into();
        assert!(!matches(&unreadable, &expected()));
    }

    #[test]
    fn rfc3339_reads_utc_offsets_and_fractions_and_refuses_the_rest() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(Micros(0)));
        assert_eq!(parse_rfc3339("2000-02-29T00:00:00.5Z"), Some(Micros(951_782_400_500_000)));
        assert_eq!(parse_rfc3339("2000-02-29T01:00:00+01:00"), Some(Micros(951_782_400_000_000)), "an offset moves to UTC");
        assert_eq!(parse_rfc3339("2000-02-28T23:30:00-00:30"), Some(Micros(951_782_400_000_000)));
        assert_eq!(parse_rfc3339("2023-11-14T22:13:20.123456789Z"), Some(Micros(1_700_000_000_123_456)), "beyond microseconds is dropped");
        for bad in ["", "2023-11-14", "2023-13-14T00:00:00Z", "2023-11-14T24:00:00Z", "2023-11-14T00:00:00", "2023-11-14T00:00:00.Z", "2023-11-14T00:00:00+0100"] {
            assert_eq!(parse_rfc3339(bad), None, "{bad:?}");
        }
        assert_eq!(format_rfc3339(Micros(1_700_000_000_123_456)), "2023-11-14T22:13:20.123Z");
        assert_eq!(parse_rfc3339(&format_rfc3339(Micros(1_700_000_000_123_000))), Some(Micros(1_700_000_000_123_000)));
    }
}
