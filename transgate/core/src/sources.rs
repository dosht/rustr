//! Job sources: uploads, API audio URLs and signed-in YouTube captions
//! (UPL-009..012, UPL-022..031, LNG-026/027).

use serde_json::{Value, json};

use crate::ids::{AccountId, JobId};
use crate::languages::Language;

/// UPL-010: `userId={user}/jobId={job}/{file}` with the user from the session.
pub fn upload_key(account: AccountId, job: JobId, file_name: &str) -> String {
    format!("userId={account}/jobId={}/{file_name}", job.0)
}

/// UPL-011/016, JOB-043: capability lifetimes in seconds.
pub const UPLOAD_URL_TTL_SECONDS: u32 = 900;
pub const PLAYER_URL_TTL_SECONDS: u32 = 900;
pub const PROVIDER_URL_TTL_SECONDS: u32 = 3600;

/// UPL-022: watch, youtu.be, embed, v, shorts, live, or a bare 11-character id.
pub fn youtube_video_id(input: &str) -> Option<String> {
    fn word(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    }
    fn take_id(rest: &str) -> Option<String> {
        let id: String = rest.chars().take_while(|c| word(*c)).collect();
        (!id.is_empty()).then_some(id)
    }
    // youtube.com/watch?(...&)?v=ID
    if let Some(pos) = input.find("youtube.com/watch?") {
        let query = &input[pos + "youtube.com/watch?".len()..];
        // `(?:.*&)?v=` is greedy: the last `&v=` with an id wins, else a leading `v=`.
        let parts: Vec<&str> = query.split('&').collect();
        let id_of = |part: &str| part.strip_prefix("v=").and_then(take_id);
        if let Some(id) = parts.iter().skip(1).rev().find_map(|p| id_of(p)) {
            return Some(id);
        }
        if let Some(id) = parts.first().and_then(|p| id_of(p)) {
            return Some(id);
        }
    }
    for prefix in ["youtu.be/", "youtube.com/embed/", "youtube.com/v/", "youtube.com/shorts/", "youtube.com/live/"] {
        if let Some(pos) = input.find(prefix) && let Some(id) = take_id(&input[pos + prefix.len()..]) {
            return Some(id);
        }
    }
    if input.chars().count() == 11 && input.chars().all(word) {
        return Some(input.to_string());
    }
    None
}

/// The canonical watch URL stored in `file_path` and `youtube_url` (UPL-024).
pub fn youtube_watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// LNG-026: the user's choice, else the video's declared language (base of the
/// BCP-47 tag through the initials table), else English.
pub fn youtube_language(requested: Option<Language>, declared: Option<&str>) -> Language {
    requested
        .or_else(|| declared.and_then(|tag| Language::from_code(tag.split('-').next().unwrap_or(""))))
        .unwrap_or_else(|| Language::from_name("English").expect("English is offered"))
}

/// UPL-024: `round(seconds / 60)`, NULL when the API reports zero (the queue
/// shows `ceil`, UPL-025; the two disagree by design).
pub fn youtube_duration_minutes(seconds: Option<f64>) -> Option<i32> {
    match seconds {
        Some(s) if s > 0.0 => Some(js_round(s / 60.0) as i32),
        _ => None,
    }
}

/// UPL-025: the queue's displayed minutes for a YouTube file.
pub fn youtube_display_minutes(seconds: f64) -> i64 {
    (seconds / 60.0).ceil() as i64
}

fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// The legacy `whisper_params` written for YouTube drafts (jobs-transcription 2.2).
pub fn youtube_whisper_params(video_id: &str, detected_language: Option<&str>) -> Value {
    json!({
        "youtube_video_id": video_id,
        "transcript_source": "transcriptapi",
        "detected_language": detected_language,
    })
}

/// UPL-027 error codes, with the stored job message.
pub fn caption_failure(error: &media_ports::CaptionError, video_id: &str) -> (&'static str, String) {
    use media_ports::CaptionError as E;
    match error {
        E::NoCaptions | E::LanguageUnavailable => ("no_captions", format!("No transcript available for video {video_id}")),
        E::AgeRestricted => ("age_restricted", format!("Age-restricted video {video_id}")),
        E::RateLimited => ("rate_limited", format!("Rate limit exceeded for video {video_id}")),
        E::Network => ("network_error", format!("Network error fetching transcript for {video_id}")),
        E::InvalidVideo | E::VideoUnavailable | E::Unknown => ("unknown_error", format!("Failed to fetch transcript for {video_id}")),
    }
}

/// UPL-031: API URL jobs.
pub const API_JOB_NAME: &str = "API Transcription";

/// URL validation result (UPL-030). Destination checks against resolved
/// addresses and redirects happen at fetch time in the controlled stager;
/// this is the syntax and literal-address check the source ran at submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlCheck {
    Ok,
    Invalid(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlKind {
    Audio,
    Callback,
}

impl UrlKind {
    fn label(self) -> &'static str {
        match self {
            UrlKind::Audio => "Audio URL",
            UrlKind::Callback => "Callback URL",
        }
    }
}

struct ParsedUrl<'a> {
    scheme: String,
    userinfo: bool,
    host: &'a str,
}

fn parse_url(url: &str) -> Option<ParsedUrl<'_>> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((_, hp)) => (true, hp),
        None => (false, authority),
    };
    let host = if let Some(stripped) = hostport.strip_prefix('[') {
        &stripped[..stripped.find(']')?]
    } else {
        hostport.rsplit_once(':').map(|(h, _)| h).unwrap_or(hostport)
    };
    if host.is_empty() {
        return None;
    }
    Some(ParsedUrl { scheme: scheme.to_ascii_lowercase(), userinfo, host })
}

/// The source's private ranges (`URL_VALIDATION_CONFIG.PRIVATE_IP_RANGES`) and IPv6 rules.
pub fn is_private_literal(host: &str) -> bool {
    if host.contains(':') {
        let lower = host.to_ascii_lowercase();
        return lower == "::1"
            || lower == "0:0:0:0:0:0:0:1"
            || lower.starts_with("fc")
            || lower.starts_with("fd")
            || lower.starts_with("fe80");
    }
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() != 4 || !octets.iter().all(|o| !o.is_empty() && o.len() <= 3 && o.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let n: Vec<u32> = octets.iter().map(|o| o.parse().unwrap_or(999)).collect();
    n[0] == 127
        || n[0] == 10
        || (n[0] == 192 && n[1] == 168)
        || (n[0] == 172 && (16..=31).contains(&n[1]))
        || (n[0] == 169 && n[1] == 254)
        || n[0] == 0
        || n[0] == 224
        || n[0] == 240
        || host == "255.255.255.255"
}

/// UPL-030 at submission. `ssrf_protection` is off only in development
/// (`DISABLE_SSRF_PROTECTION`). Embedded credentials are refused (a recorded
/// correction under the implementation contract's destination checks).
pub fn check_url(url: &str, kind: UrlKind, ssrf_protection: bool) -> UrlCheck {
    let label = kind.label();
    let Some(parsed) = parse_url(url) else {
        return UrlCheck::Invalid(format!("{label} must be a valid URL"));
    };
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return UrlCheck::Invalid(format!("{label} must use HTTP or HTTPS protocol"));
    }
    if !ssrf_protection {
        return UrlCheck::Ok;
    }
    if parsed.userinfo {
        return UrlCheck::Invalid(format!("{label} must be a valid URL"));
    }
    if is_private_literal(parsed.host) {
        return UrlCheck::Invalid(format!("{label} cannot point to private IP addresses for security reasons"));
    }
    UrlCheck::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    // UPL-022
    #[test]
    fn youtube_ids_are_extracted_from_every_accepted_form() {
        for (input, id) in [
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            ("https://youtube.com/watch?feature=x&v=abc-DEF_123", "abc-DEF_123"),
            ("https://youtu.be/dQw4w9WgXcQ?t=3", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/embed/xyz", "xyz"),
            ("https://www.youtube.com/v/xyz", "xyz"),
            ("https://www.youtube.com/shorts/xyz", "xyz"),
            ("https://www.youtube.com/live/xyz", "xyz"),
            ("dQw4w9WgXcQ", "dQw4w9WgXcQ"),
        ] {
            assert_eq!(youtube_video_id(input).as_deref(), Some(id), "{input}");
        }
        assert_eq!(youtube_video_id("https://example.com/watch?v=1"), None);
        assert_eq!(youtube_video_id("short"), None);
    }

    // LNG-026, UPL-024
    #[test]
    fn youtube_language_and_duration() {
        assert_eq!(youtube_language(None, Some("tr")).name(), "Turkish");
        assert_eq!(youtube_language(None, Some("en-US")).name(), "English");
        assert_eq!(youtube_language(None, Some("bn")).name(), "English");
        assert_eq!(youtube_language(Language::from_name("German"), Some("tr")).name(), "German");
        assert_eq!(youtube_duration_minutes(Some(89.0)), Some(1));
        assert_eq!(youtube_duration_minutes(Some(90.0)), Some(2));
        assert_eq!(youtube_duration_minutes(Some(0.0)), None);
        assert_eq!(youtube_display_minutes(61.0), 2);
    }

    // UPL-030
    #[test]
    fn urls_are_checked_like_the_source() {
        assert_eq!(check_url("https://example.com/a.mp3", UrlKind::Audio, true), UrlCheck::Ok);
        assert_eq!(check_url("ftp://example.com/a", UrlKind::Audio, true), UrlCheck::Invalid("Audio URL must use HTTP or HTTPS protocol".into()));
        assert_eq!(check_url("not a url", UrlKind::Callback, true), UrlCheck::Invalid("Callback URL must be a valid URL".into()));
        for host in ["127.0.0.1", "10.1.2.3", "192.168.0.1", "172.20.0.1", "169.254.1.1", "0.0.0.0", "224.0.0.1", "240.0.0.1", "[::1]", "[fd00::1]", "[fe80::1]"] {
            assert!(matches!(check_url(&format!("http://{host}/x"), UrlKind::Audio, true), UrlCheck::Invalid(_)), "{host}");
        }
        assert_eq!(check_url("http://172.32.0.1/x", UrlKind::Audio, true), UrlCheck::Ok);
        assert_eq!(check_url("http://10.0.0.1/x", UrlKind::Audio, false), UrlCheck::Ok);
        assert!(matches!(check_url("https://user:pw@example.com/x", UrlKind::Audio, true), UrlCheck::Invalid(_)));
    }

    #[test]
    fn upload_keys_use_the_session_user() {
        let account = AccountId(uuid::Uuid::nil());
        assert_eq!(upload_key(account, JobId(7), "a b.mp3"), "userId=00000000-0000-0000-0000-000000000000/jobId=7/a b.mp3");
    }
}
