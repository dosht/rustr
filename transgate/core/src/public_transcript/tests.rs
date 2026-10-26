use std::collections::BTreeMap;
use std::sync::Mutex;

use billing_core::types::BoxFuture;
use media_ports::CaptionDocument;

use super::*;
use crate::jobs::testing::{CountingLimiter, TestClock};

const VIDEO: &str = "dQw4w9WgXcQ";

struct Captions {
    answers: Mutex<Vec<Result<CaptionDocument, CaptionError>>>,
    calls: Mutex<Vec<(String, Option<String>)>>,
}

impl PublicCaptions for Captions {
    fn fetch<'a>(&'a self, video_id: &'a str, language: Option<&'a str>) -> BoxFuture<'a, Result<CaptionDocument, CaptionError>> {
        self.calls.lock().unwrap().push((video_id.into(), language.map(str::to_string)));
        let next = self.answers.lock().unwrap().remove(0);
        Box::pin(async move { next })
    }
}

fn cue(text: &str, start: f64, duration: f64) -> CaptionCue {
    CaptionCue { text: text.into(), start, duration }
}

fn doc(cues: Vec<CaptionCue>) -> CaptionDocument {
    CaptionDocument { video_id: VIDEO.into(), language: Some("en".into()), cues, metadata: serde_json::Value::Null }
}

fn tool(answers: Vec<Result<CaptionDocument, CaptionError>>) -> (PublicTranscript, Arc<Captions>, Arc<TestClock>) {
    let captions = Arc::new(Captions { answers: Mutex::new(answers), calls: Mutex::new(vec![]) });
    let clock = Arc::new(TestClock(Mutex::new(Micros(1_800_000_000_000_000))));
    let limiter = Arc::new(CountingLimiter { hits: Mutex::new(BTreeMap::new()) });
    (PublicTranscript::new(captions.clone(), limiter, clock.clone()), captions, clock)
}

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

// EXP-017: the four formats with millisecond times and short stamps.
#[test]
fn formats() {
    let s = [cue("Hello  there", 0.0, 1.25), cue("general\nKenobi", 3725.5, 2.0)];
    assert_eq!(format_transcript(&s, TranscriptFormat::Txt), "Hello there general Kenobi");
    assert_eq!(format_transcript(&s, TranscriptFormat::Timestamped), "[0:00] Hello  there\n[1:02:05] general\nKenobi");
    assert_eq!(
        format_transcript(&s, TranscriptFormat::Srt),
        "1\n00:00:00,000 --> 00:00:01,250\nHello  there\n\n2\n01:02:05,500 --> 01:02:07,500\ngeneral\nKenobi\n"
    );
    assert_eq!(
        format_transcript(&s[..1], TranscriptFormat::Vtt),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.250\nHello  there\n"
    );
    assert_eq!(TranscriptFormat::Timestamped.extension(), "txt");
}

// UPL-029: validation order and messages after the limiter.
#[test]
fn validation() {
    let (t, captions, _) = tool(vec![]);
    let err = |url: &str, format: Option<&str>, lang: Option<&str>| run(t.fetch("1.2.3.4", url, format, lang)).unwrap().result.unwrap_err().code();
    assert_eq!(err("", None, None), "INVALID_URL");
    assert_eq!(err("https://youtu.be/dQw4w9WgXcQ", Some("pdf"), None), "INVALID_FORMAT");
    assert_eq!(err("https://youtu.be/dQw4w9WgXcQ", None, Some("e")), "INVALID_LANG");
    assert_eq!(err("https://youtu.be/dQw4w9WgXcQ", None, Some("en_US")), "INVALID_LANG");
    assert_eq!(err("https://example.com/x", None, Some("pt-BR")), "INVALID_URL");
    assert!(captions.calls.lock().unwrap().is_empty());
}

// RTL-004/005: ten per IP per minute; the eleventh is refused before validation.
#[test]
fn ten_per_ip_per_minute() {
    let (t, _, _) = tool(vec![]);
    for _ in 0..10 {
        let reply = run(t.fetch("9.9.9.9", "", None, None)).unwrap();
        assert!(reply.rate.allowed);
    }
    let reply = run(t.fetch("9.9.9.9", "", None, None)).unwrap();
    assert_eq!(reply.rate.remaining, 0);
    assert_eq!(reply.result.unwrap_err(), PublicTranscriptError::RateLimited);
    assert!(run(t.fetch("8.8.8.8", "", None, None)).unwrap().rate.allowed);
}

// UPL-029: successes are cached per video and language for six hours; failures are not.
#[test]
fn successes_are_cached_six_hours() {
    let (t, captions, clock) = tool(vec![
        Err(CaptionError::NoCaptions),
        Ok(doc(vec![cue(" hi ", 0.0, 1.0), cue("  ", 1.0, 1.0)])),
        Ok(doc(vec![cue("again", 0.0, 1.0)])),
    ]);
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let first = run(t.fetch("ip", url, None, None)).unwrap().result.unwrap_err();
    assert_eq!((first.http_status(), first.code()), (422, "NO_TRANSCRIPT"));
    let ok = run(t.fetch("ip", url, Some("srt"), None)).unwrap().result.unwrap();
    assert_eq!((ok.segments.len(), ok.segments[0].text.as_str(), ok.cached), (1, "hi", false));
    let hit = run(t.fetch("ip", url, Some("txt"), None)).unwrap().result.unwrap();
    assert!(hit.cached);
    assert_eq!(hit.text, "hi");
    clock.advance_minutes(CACHE_TTL_MINUTES + 1);
    let fresh = run(t.fetch("ip", url, None, None)).unwrap().result.unwrap();
    assert_eq!((fresh.text.as_str(), fresh.cached), ("again", false));
    assert_eq!(captions.calls.lock().unwrap().len(), 3);
}

// UPL-029: at most 500 entries, least recently used evicted first.
#[test]
fn cache_evicts_least_recently_used() {
    let mut cache = TranscriptCache::default();
    let now = Micros(0);
    let value = |id: &str| PublicTranscriptResult { video_id: id.into(), language: None, segments: vec![] };
    for i in 0..CACHE_MAX_ENTRIES {
        cache.set(&format!("v{i}"), None, value("x"), now);
    }
    assert!(cache.get("v0", None, now).is_some());
    cache.set("new", Some("tr"), value("y"), now);
    assert_eq!(cache.len(), CACHE_MAX_ENTRIES);
    assert!(cache.get("v0", None, now).is_some());
    assert!(cache.get("v1", None, now).is_none());
    assert!(cache.get("new", None, now).is_none());
    assert!(cache.get("new", Some("tr"), now).is_some());
}

// The source's status table for public caption failures.
#[test]
fn upstream_statuses() {
    let status = |e| PublicTranscriptError::Upstream(e).http_status();
    assert_eq!(status(CaptionError::LanguageUnavailable), 422);
    assert_eq!(status(CaptionError::VideoUnavailable), 404);
    assert_eq!(status(CaptionError::RateLimited), 503);
    assert_eq!(status(CaptionError::Network), 502);
}
