//! Soniox tokens to lines, ported from `frontend/src/utils/soniox/transcript-processor.ts`
//! (TRX-024..029, TRL-004). Arithmetic stays in f64 milliseconds like the source,
//! including its quirks: the last-pass zero-duration fix re-applies the 500 ms
//! offset to an already offset time.

use media_ports::SonioxToken;
use serde_json::Value;

use super::time::{parse_hms_ms, soniox_time};
use super::{NewLine, TranslationStatus};
use crate::languages::Language;

const VERY_LONG_GAP_MS: f64 = 2000.0;
const CLAUSE_GAP_MS: f64 = 300.0;
const MIN_DURATION_MS: f64 = 1000.0;
const MIN_WORDS: usize = 3;

/// A speaker as JavaScript sees it: strict equality distinguishes numbers and strings.
#[derive(Clone, Debug, PartialEq)]
enum Speaker {
    Number(f64),
    Text(String),
}

fn speaker_of(token: &SonioxToken) -> Option<Speaker> {
    match &token.speaker {
        Some(Value::Number(n)) => n.as_f64().map(Speaker::Number),
        Some(Value::String(s)) => Some(Speaker::Text(s.clone())),
        _ => None,
    }
}

/// `SPEAKER_${(speaker - 1).toString().padStart(2, '0')}`. JavaScript coerces a
/// numeric string in the subtraction.
fn speaker_name(speaker: &Speaker) -> Option<String> {
    let n = match speaker {
        Speaker::Number(n) => *n,
        Speaker::Text(s) => s.trim().parse::<f64>().ok()?,
    };
    let v = n - 1.0;
    let text = if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") };
    Some(format!("SPEAKER_{text:0>2}"))
}

fn start(token: &SonioxToken) -> f64 {
    token.start_ms.unwrap_or(0.0)
}

fn end(token: &SonioxToken) -> f64 {
    token.end_ms.unwrap_or(0.0)
}

fn is_event(token: &SonioxToken) -> bool {
    token.is_audio_event.unwrap_or(false)
}

fn status_of(token: &SonioxToken) -> Option<&str> {
    token.translation_status.as_deref()
}

/// `[.!?؟。！？]$`
fn ends_sentence(text: &str) -> bool {
    text.trim().chars().last().is_some_and(|c| matches!(c, '.' | '!' | '?' | '؟' | '。' | '！' | '？'))
}

/// `[،,;؛]$`
fn ends_clause(text: &str) -> bool {
    text.trim().chars().last().is_some_and(|c| matches!(c, '،' | ',' | ';' | '؛'))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug)]
struct Segment {
    start_time: String,
    end_time: String,
    line: String,
    speaker: Option<String>,
    split: bool,
    language: Option<Language>,
    status: Option<TranslationStatus>,
}

fn segment(
    start_ms: f64,
    end_ms: f64,
    line: String,
    speaker: Option<&Speaker>,
    split: bool,
    language: Option<&str>,
    status: Option<TranslationStatus>,
) -> Segment {
    Segment {
        start_time: soniox_time(start_ms),
        end_time: soniox_time(end_ms),
        line,
        speaker: speaker.and_then(speaker_name),
        split,
        language: language.and_then(Language::from_code),
        status,
    }
}

/// `processTokenGroup` for plain transcription.
fn process_token_group(tokens: &[SonioxToken], speakers: bool) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current_start: Option<f64> = None;
    let mut current_end: Option<f64> = None;
    let mut parts: Vec<String> = Vec::new();
    let mut current_speaker: Option<Speaker> = None;

    let finalize = |segments: &mut Vec<Segment>, s: f64, e: f64, parts: &[String], sp: &Option<Speaker>| {
        let line = parts.concat().trim().to_string();
        if !line.is_empty() {
            segments.push(segment(s, e, line, sp.as_ref(), speakers, None, None));
        }
    };

    for token in tokens {
        if is_event(token) {
            continue;
        }
        let gap = current_end.map(|e| start(token) - e).unwrap_or(0.0);
        let last = parts.last();
        let previous_sentence = last.is_some_and(|p| ends_sentence(p));
        let previous_clause = last.is_some_and(|p| ends_clause(p));
        let very_long = current_end.is_some() && gap > VERY_LONG_GAP_MS;
        let token_speaker = speaker_of(token);
        let speaker_changed = speakers && current_speaker.is_some() && token_speaker != current_speaker;
        let new_segment = previous_sentence || (previous_clause && gap > CLAUSE_GAP_MS) || very_long || speaker_changed;

        if new_segment && !parts.is_empty() {
            finalize(&mut segments, current_start.unwrap_or(0.0), current_end.unwrap_or(0.0), &parts, &current_speaker);
            current_start = None;
            current_end = None;
            parts.clear();
        }
        if current_start.is_none() {
            current_start = Some(start(token));
        }
        current_end = Some(current_end.unwrap_or(0.0).max(end(token)));
        parts.push(token.text.clone());
        if token_speaker.is_some() {
            current_speaker = token_speaker;
        }
    }
    if let (false, Some(s), Some(e)) = (parts.is_empty(), current_start, current_end) {
        finalize(&mut segments, s, e, &parts, &current_speaker);
    }
    segments
}

/// `isPunctuationOnly`
fn punctuation_only(line: &str) -> bool {
    line.chars().all(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '،' | '.' | '؟' | '!' | '؛' | ',' | ';' | ':' | '-' | '–' | '—' | '…' | '·' | '•' | '"' | '\'' | '`'
                    | '«' | '»' | '(' | ')' | '[' | ']' | '{' | '}'
            )
    })
}

/// `^[،.؟!؛,;:\s]+`
fn starts_with_punctuation(line: &str) -> bool {
    line.chars().next().is_some_and(|c| c.is_whitespace() || matches!(c, '،' | '.' | '؟' | '!' | '؛' | ',' | ';' | ':'))
}

fn word_count(line: &str) -> usize {
    line.split_whitespace().count()
}

/// `mergeShortSegments`
fn merge_short(segments: Vec<Segment>) -> Vec<Segment> {
    if segments.len() <= 1 {
        return segments;
    }
    let mut merged: Vec<Segment> = Vec::new();
    for seg in segments {
        let duration = parse_hms_ms(&seg.end_time) - parse_hms_ms(&seg.start_time);
        let words = word_count(&seg.line);
        let starts_punct = starts_with_punctuation(&seg.line);
        let only_punct = punctuation_only(&seg.line);

        if let Some(prev) = merged.last_mut() {
            if prev.language != seg.language || prev.status != seg.status {
                if only_punct {
                    prev.end_time = seg.end_time.clone();
                    prev.line = collapse_whitespace(&format!("{}{}", prev.line, seg.line));
                    continue;
                }
                merged.push(seg);
                continue;
            }
            let prev_duration = parse_hms_ms(&prev.end_time) - parse_hms_ms(&prev.start_time);
            let prev_words = word_count(&prev.line);
            let merge = only_punct
                || starts_punct
                || prev_duration == 0.0
                || duration == 0.0
                || (prev_duration < MIN_DURATION_MS && prev_words < MIN_WORDS)
                || (duration < MIN_DURATION_MS && words < MIN_WORDS);
            if merge {
                prev.end_time = seg.end_time.clone();
                prev.line = collapse_whitespace(&format!("{} {}", prev.line, seg.line));
                continue;
            }
        }
        if only_punct && merged.is_empty() {
            continue;
        }
        merged.push(seg);
    }

    for i in 0..merged.len() {
        let start_ms = parse_hms_ms(&merged[i].start_time);
        let end_ms = parse_hms_ms(&merged[i].end_time);
        if start_ms == end_ms {
            if i > 0 {
                let prev_end_ms = parse_hms_ms(&merged[i - 1].end_time);
                merged[i].start_time = merged[i - 1].end_time.clone();
                merged[i].end_time = soniox_time((prev_end_ms + 1000.0).max(end_ms));
            } else if i + 1 < merged.len() {
                merged[i].end_time = merged[i + 1].start_time.clone();
            } else {
                merged[i].end_time = soniox_time(start_ms + 1000.0);
            }
        }
    }
    merged
}

struct Group<'a> {
    tokens: Vec<&'a SonioxToken>,
    status: Option<String>,
    language: Option<String>,
    start_ms: f64,
    end_ms: f64,
}

/// `groupTokensByTranslationStatus`
fn group_tokens(tokens: &[SonioxToken]) -> Vec<Group<'_>> {
    let mut groups: Vec<Group<'_>> = Vec::new();
    for token in tokens.iter().filter(|t| !is_event(t)) {
        let status = status_of(token).map(str::to_string);
        let new_group = groups.last().is_none_or(|g| g.status != status);
        if new_group {
            groups.push(Group {
                tokens: Vec::new(),
                status: status.clone(),
                language: token.language.clone(),
                start_ms: start(token),
                end_ms: end(token),
            });
        }
        let g = groups.last_mut().expect("group exists");
        g.tokens.push(token);
        let s = start(token);
        if s > 0.0 && (g.start_ms == 0.0 || s < g.start_ms) {
            g.start_ms = s;
        }
        let e = end(token);
        if e > 0.0 && e > g.end_ms {
            g.end_ms = e;
        }
        if g.language.is_none() && let Some(l) = &token.language {
            g.language = Some(l.clone());
        }
    }
    groups
}

/// `createSegmentFromGroup`
fn group_segment(group: &Group<'_>, speakers: bool, range: (f64, f64), status: TranslationStatus) -> Option<Segment> {
    if group.tokens.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut speaker: Option<Speaker> = None;
    for token in &group.tokens {
        text.push_str(&token.text);
        if let Some(s) = speaker_of(token) {
            speaker = Some(s);
        }
    }
    let line = text.trim().to_string();
    if line.is_empty() {
        return None;
    }
    Some(segment(range.0, range.1, collapse_whitespace(&line), speaker.as_ref(), speakers, group.language.as_deref(), Some(status)))
}

fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `processTranslationTokens` (TRX-028)
fn process_translation(tokens: &[SonioxToken], speakers: bool) -> Vec<Segment> {
    let groups = group_tokens(tokens);
    let originals: Vec<&Group<'_>> = groups.iter().filter(|g| g.status.as_deref() == Some("original")).collect();
    if originals.is_empty() {
        return vec![];
    }
    let mut ranges: Vec<(f64, f64)> = Vec::new();
    let mut i = 0;
    while i < originals.len() {
        let current = originals[i].start_ms;
        let mut j = i;
        while j < originals.len() && originals[j].start_ms == current {
            j += 1;
        }
        let same = (j - i) as f64;
        let mut window_end = if j < originals.len() {
            originals[j].start_ms
        } else {
            let last = originals[j - 1];
            if last.end_ms > current { last.end_ms } else { current + same * 2000.0 }
        };
        if window_end <= current {
            window_end = current + same * 2000.0;
        }
        let step = (window_end - current) / same;
        for k in 0..(j - i) {
            let k = k as f64;
            ranges.push((js_round(current + k * step), js_round(current + (k + 1.0) * step)));
        }
        i = j;
    }

    let mut out: Vec<Segment> = Vec::new();
    let mut original_index = 0;
    let mut idx = 0;
    while idx < groups.len() {
        let g = &groups[idx];
        if g.status.as_deref() == Some("original") {
            let range = ranges[original_index];
            match group_segment(g, speakers, range, TranslationStatus::Original) {
                Some(seg) => {
                    out.push(seg);
                    match groups.get(idx + 1) {
                        Some(next) if next.status.as_deref() == Some("translation") => {
                            if let Some(t) = group_segment(next, speakers, range, TranslationStatus::Translation) {
                                out.push(t);
                            }
                            idx += 2;
                        }
                        _ => idx += 1,
                    }
                }
                None => idx += 1,
            }
            original_index += 1;
        } else {
            idx += 1;
        }
    }
    // Stable sort by start, original before translation at the same start.
    out.sort_by(|a, b| {
        let t = parse_hms_ms(&a.start_time) - parse_hms_ms(&b.start_time);
        if t != 0.0 {
            return t.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
        }
        match (a.status, b.status) {
            (Some(TranslationStatus::Original), Some(TranslationStatus::Translation)) => std::cmp::Ordering::Less,
            (Some(TranslationStatus::Translation), Some(TranslationStatus::Original)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
    out
}

/// `processSonioxTokens`: a payload with both original and translation tokens is
/// a translation payload; anything else is plain transcription with short-line
/// merging. Plain lines carry no language and no translation status.
pub fn normalize(tokens: &[SonioxToken], speakers: bool) -> Vec<NewLine> {
    if tokens.is_empty() {
        return vec![];
    }
    let has_original = tokens.iter().any(|t| status_of(t) == Some("original"));
    let has_translation = tokens.iter().any(|t| status_of(t) == Some("translation"));
    let segments = if has_original && has_translation {
        process_translation(tokens, speakers)
    } else {
        merge_short(process_token_group(tokens, speakers))
    };
    segments
        .into_iter()
        .map(|s| NewLine {
            start_time: s.start_time,
            end_time: s.end_time,
            line: s.line,
            speaker: s.speaker,
            corrected_text: None,
            split_on_speaker_change: Some(s.split),
            language: s.language,
            translation_status: s.status,
        })
        .collect()
}

/// The largest token `end_ms` (translation tokens carry 0), for the duration fallback.
pub fn max_token_end_ms(tokens: &[SonioxToken]) -> Option<f64> {
    tokens.iter().map(end).fold(None, |acc, e| Some(acc.map_or(e, |a: f64| a.max(e))))
}

#[cfg(test)]
mod tests;
