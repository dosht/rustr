//! The public YouTube transcript tool (UPL-029, EXP-017, RTL-004/005), ported
//! from `pages/api/youtube/public-transcript.ts`, `publicTranscriptCache.ts` and
//! `transcriptFormatters.ts`. No account, never a job, never the paid caption
//! service. The transport (`PublicCaptions`) owns cue units and entities.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use billing_core::ports::StoreError;
use billing_core::types::Clock;
use media_ports::{CaptionCue, CaptionError, PublicCaptions};
use serde::{Deserialize, Serialize};

use crate::ids::{MICROS_PER_MINUTE, Micros};
use crate::jobs::ports::{RateDecision, RateLimiter};
use crate::sources::youtube_video_id;

/// RTL-004: 10 requests per IP per minute.
pub const RATE_LIMIT: u32 = 10;
pub const RATE_WINDOW_MS: i64 = 60_000;
/// UPL-029: successes are cached for six hours, at most 500 entries, least recently used out first.
pub const CACHE_TTL_MINUTES: i64 = 6 * 60;
pub const CACHE_MAX_ENTRIES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptFormat {
    Txt,
    Timestamped,
    Srt,
    Vtt,
}

impl TranscriptFormat {
    pub const ALL: [TranscriptFormat; 4] = [TranscriptFormat::Txt, TranscriptFormat::Timestamped, TranscriptFormat::Srt, TranscriptFormat::Vtt];

    pub fn as_str(self) -> &'static str {
        match self {
            TranscriptFormat::Txt => "txt",
            TranscriptFormat::Timestamped => "timestamped",
            TranscriptFormat::Srt => "srt",
            TranscriptFormat::Vtt => "vtt",
        }
    }

    pub fn parse(text: &str) -> Option<TranscriptFormat> {
        TranscriptFormat::ALL.into_iter().find(|f| f.as_str() == text)
    }

    /// `FORMAT_EXTENSIONS`
    pub fn extension(self) -> &'static str {
        match self {
            TranscriptFormat::Txt | TranscriptFormat::Timestamped => "txt",
            TranscriptFormat::Srt => "srt",
            TranscriptFormat::Vtt => "vtt",
        }
    }
}

/// `pad`: `Math.floor(n)` padded to the width (a rounded 1000 ms stays four digits, as in the source).
fn pad(n: f64, width: usize) -> String {
    format!("{:0width$}", n.floor() as i64, width = width)
}

/// `formatTimestamp`: `HH:MM:SS` then the separator and rounded milliseconds.
pub fn timestamp(total_seconds: f64, ms_separator: char) -> String {
    let hours = (total_seconds / 3600.0).floor();
    let minutes = ((total_seconds % 3600.0) / 60.0).floor();
    let seconds = (total_seconds % 60.0).floor();
    // Math.round on a non-negative value.
    let ms = ((total_seconds - total_seconds.floor()) * 1000.0 + 0.5).floor();
    format!("{}:{}:{}{ms_separator}{}", pad(hours, 2), pad(minutes, 2), pad(seconds, 2), pad(ms, 3))
}

/// `formatShortStamp`: `M:SS`, or `H:MM:SS` from one hour.
pub fn short_stamp(total_seconds: f64) -> String {
    let hours = (total_seconds / 3600.0).floor();
    let minutes = ((total_seconds % 3600.0) / 60.0).floor();
    let seconds = (total_seconds % 60.0).floor();
    if hours > 0.0 {
        format!("{}:{}:{}", hours as i64, pad(minutes, 2), pad(seconds, 2))
    } else {
        format!("{}:{}", minutes as i64, pad(seconds, 2))
    }
}

/// EXP-017 `formatTranscript`.
pub fn format_transcript(segments: &[CaptionCue], format: TranscriptFormat) -> String {
    match format {
        TranscriptFormat::Txt => {
            let joined = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            joined.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        TranscriptFormat::Timestamped => {
            segments.iter().map(|s| format!("[{}] {}", short_stamp(s.start), s.text)).collect::<Vec<_>>().join("\n")
        }
        TranscriptFormat::Srt => segments
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}\n{} --> {}\n{}\n", i + 1, timestamp(s.start, ','), timestamp(s.start + s.duration, ','), s.text))
            .collect::<Vec<_>>()
            .join("\n"),
        TranscriptFormat::Vtt => {
            let body = segments
                .iter()
                .map(|s| format!("{} --> {}\n{}\n", timestamp(s.start, '.'), timestamp(s.start + s.duration, '.'), s.text))
                .collect::<Vec<_>>()
                .join("\n");
            format!("WEBVTT\n\n{body}")
        }
    }
}

/// `PublicTranscriptResult`, the cached value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicTranscriptResult {
    pub video_id: String,
    pub language: Option<String>,
    pub segments: Vec<CaptionCue>,
}

/// The 200 body: `{ videoId, language, segments, text, format }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicTranscriptAnswer {
    pub video_id: String,
    pub language: Option<String>,
    pub segments: Vec<CaptionCue>,
    pub text: String,
    pub format: TranscriptFormat,
    pub cached: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PublicTranscriptError {
    /// 429 `RATE_LIMITED`.
    RateLimited,
    /// 400 `INVALID_URL`: missing or not a YouTube link.
    InvalidUrl { missing: bool },
    /// 400 `INVALID_FORMAT`.
    InvalidFormat,
    /// 400 `INVALID_LANG`.
    InvalidLanguage,
    /// The classified public caption failure.
    Upstream(CaptionError),
}

impl PublicTranscriptError {
    pub fn http_status(&self) -> u16 {
        match self {
            PublicTranscriptError::RateLimited => 429,
            PublicTranscriptError::InvalidUrl { .. } | PublicTranscriptError::InvalidFormat | PublicTranscriptError::InvalidLanguage => 400,
            PublicTranscriptError::Upstream(e) => match upstream_code(e) {
                "NO_TRANSCRIPT" | "LANGUAGE_UNAVAILABLE" => 422,
                "VIDEO_UNAVAILABLE" => 404,
                "RATE_LIMITED_UPSTREAM" => 503,
                _ => 502,
            },
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            PublicTranscriptError::RateLimited => "RATE_LIMITED",
            PublicTranscriptError::InvalidUrl { .. } => "INVALID_URL",
            PublicTranscriptError::InvalidFormat => "INVALID_FORMAT",
            PublicTranscriptError::InvalidLanguage => "INVALID_LANG",
            PublicTranscriptError::Upstream(e) => upstream_code(e),
        }
    }

    pub fn message(&self) -> String {
        match self {
            PublicTranscriptError::RateLimited => "Too many requests. Please wait a minute and try again.".into(),
            PublicTranscriptError::InvalidUrl { missing: true } => "A YouTube URL is required.".into(),
            PublicTranscriptError::InvalidUrl { missing: false } => {
                "That does not look like a YouTube URL. Paste a link like https://youtube.com/watch?v=...".into()
            }
            PublicTranscriptError::InvalidFormat => {
                format!("format must be one of: {}", TranscriptFormat::ALL.map(|f| f.as_str()).join(", "))
            }
            PublicTranscriptError::InvalidLanguage => "Invalid language code.".into(),
            PublicTranscriptError::Upstream(e) => match upstream_code(e) {
                "NO_TRANSCRIPT" => "This video has no captions available.".into(),
                "LANGUAGE_UNAVAILABLE" => "This video has no captions in the language you requested.".into(),
                "VIDEO_UNAVAILABLE" => "This video is private, deleted, or region-blocked.".into(),
                "RATE_LIMITED_UPSTREAM" => "YouTube is temporarily throttling requests. Please try again shortly.".into(),
                _ => "Could not fetch the transcript.".into(),
            },
        }
    }
}

/// `classifyError`. Age-restricted videos are unplayable to the public caption
/// reader, which the source library reports as unavailable [INFERENCE: the
/// library's playability error class]; anything untyped is `FETCH_FAILED`.
fn upstream_code(e: &CaptionError) -> &'static str {
    match e {
        CaptionError::NoCaptions => "NO_TRANSCRIPT",
        CaptionError::LanguageUnavailable => "LANGUAGE_UNAVAILABLE",
        CaptionError::VideoUnavailable | CaptionError::AgeRestricted => "VIDEO_UNAVAILABLE",
        CaptionError::RateLimited => "RATE_LIMITED_UPSTREAM",
        CaptionError::InvalidVideo | CaptionError::Network | CaptionError::Unknown => "FETCH_FAILED",
    }
}

/// `^[a-zA-Z-]{2,10}$`
fn valid_language(lang: &str) -> bool {
    (2..=10).contains(&lang.chars().count()) && lang.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// One request's outcome plus the limiter window: the source sets
/// `X-RateLimit-Limit` and `X-RateLimit-Remaining` on every answer after the
/// limiter, and `X-RateLimit-Reset` on a 429.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicReply {
    pub rate: RateDecision,
    pub result: Result<PublicTranscriptAnswer, PublicTranscriptError>,
}

struct Entry {
    key: String,
    value: PublicTranscriptResult,
    expires_at: Micros,
}

/// The bounded in-process success cache (`publicTranscriptCache.ts`).
#[derive(Default)]
pub struct TranscriptCache {
    entries: VecDeque<Entry>,
}

fn cache_key(video_id: &str, lang: Option<&str>) -> String {
    format!("{video_id}:{}", lang.unwrap_or(""))
}

impl TranscriptCache {
    /// A hit moves to the most recent end; an expired entry is dropped.
    pub fn get(&mut self, video_id: &str, lang: Option<&str>, now: Micros) -> Option<PublicTranscriptResult> {
        let key = cache_key(video_id, lang);
        let pos = self.entries.iter().position(|e| e.key == key)?;
        let entry = self.entries.remove(pos)?;
        if now.0 > entry.expires_at.0 {
            return None;
        }
        let value = entry.value.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    pub fn set(&mut self, video_id: &str, lang: Option<&str>, value: PublicTranscriptResult, now: Micros) {
        let key = cache_key(video_id, lang);
        self.entries.retain(|e| e.key != key);
        self.entries.push_back(Entry { key, value, expires_at: Micros(now.0 + CACHE_TTL_MINUTES * MICROS_PER_MINUTE) });
        while self.entries.len() > CACHE_MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct PublicTranscript {
    captions: Arc<dyn PublicCaptions>,
    rate_limiter: Arc<dyn RateLimiter>,
    clock: Arc<dyn Clock>,
    cache: Mutex<TranscriptCache>,
}

impl PublicTranscript {
    pub fn new(captions: Arc<dyn PublicCaptions>, rate_limiter: Arc<dyn RateLimiter>, clock: Arc<dyn Clock>) -> PublicTranscript {
        PublicTranscript { captions, rate_limiter, clock, cache: Mutex::new(TranscriptCache::default()) }
    }

    /// `POST /api/youtube/public-transcript`. `ip` is the client address the host
    /// trusts (the source used the first `x-forwarded-for` entry). `url` empty
    /// means missing; `format` `None` means `txt`; `lang` `None` means absent.
    pub async fn fetch(&self, ip: &str, url: &str, format: Option<&str>, lang: Option<&str>) -> Result<PublicReply, StoreError> {
        let now = self.clock.now();
        let rate = self.rate_limiter.hit(&format!("{ip}:youtube_public_transcript"), RATE_LIMIT, RATE_WINDOW_MS, now).await?;
        let result = if rate.allowed { self.answer(url, format, lang, now).await } else { Err(PublicTranscriptError::RateLimited) };
        Ok(PublicReply { rate, result })
    }

    async fn answer(&self, url: &str, format: Option<&str>, lang: Option<&str>, now: Micros) -> Result<PublicTranscriptAnswer, PublicTranscriptError> {
        if url.is_empty() {
            return Err(PublicTranscriptError::InvalidUrl { missing: true });
        }
        let format = TranscriptFormat::parse(format.unwrap_or("txt")).ok_or(PublicTranscriptError::InvalidFormat)?;
        if lang.is_some_and(|l| !valid_language(l)) {
            return Err(PublicTranscriptError::InvalidLanguage);
        }
        let video_id = youtube_video_id(url.trim()).ok_or(PublicTranscriptError::InvalidUrl { missing: false })?;
        let cached = self.cache.lock().expect("cache lock").get(&video_id, lang, now);
        let (result, was_cached) = match cached {
            Some(r) => (r, true),
            None => {
                let doc = self.captions.fetch(&video_id, lang).await.map_err(PublicTranscriptError::Upstream)?;
                let segments: Vec<CaptionCue> = doc
                    .cues
                    .into_iter()
                    .map(|c| CaptionCue { text: c.text.trim().to_string(), ..c })
                    .filter(|c| !c.text.is_empty())
                    .collect();
                if segments.is_empty() {
                    return Err(PublicTranscriptError::Upstream(CaptionError::NoCaptions));
                }
                let result = PublicTranscriptResult {
                    video_id: video_id.clone(),
                    language: doc.language.or_else(|| lang.map(str::to_string)),
                    segments,
                };
                // Only successes are cached.
                self.cache.lock().expect("cache lock").set(&video_id, lang, result.clone(), now);
                (result, false)
            }
        };
        let text = format_transcript(&result.segments, format);
        Ok(PublicTranscriptAnswer { video_id: result.video_id, language: result.language, segments: result.segments, text, format, cached: was_cached })
    }
}

#[cfg(test)]
mod tests;
