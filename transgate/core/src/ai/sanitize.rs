//! Exact path-specific ContentProcessor rules. These compatibility filters are
//! not SQL escaping, an HTML renderer, or a zero-PII guarantee. Apply to complete
//! cumulative prose, never independently to provider tokens.
use std::sync::LazyLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::transcript::Line;

struct Patterns {
    script_open: Regex,
    script_close: Regex,
    html: Regex,
    sql: Regex,
    javascript: Regex,
    handler: Regex,
    whitespace: Regex,
    dangerous: Regex,
    sensitive: [(Regex, &'static str); 5],
}

fn patterns() -> &'static Patterns {
    static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
        // ECMAScript \s and ASCII word boundaries, not Rust's Unicode \b/\w.
        let space = r"[\x09-\x0d\x20\u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";
        let re = |s: &str| Regex::new(s).expect("static source sanitation regex");
        Patterns {
            script_open: re(r"(?i-u:<script\b)"),
            script_close: re(r"(?i-u:</script>)"),
            html: re(r"<[^>]*>"),
            sql: re(r"(?i-u:\b(SELECT|INSERT|UPDATE|DELETE|DROP|CREATE|ALTER|EXEC|UNION)\b)"),
            javascript: re(r"(?i-u:javascript:)"),
            handler: re(&format!(r"(?i-u:on[A-Za-z0-9_]+){space}*=")),
            whitespace: re(&format!("{space}+")),
            dangerous: re(&format!(r"(?i-u:<script|javascript:|data:text/html|vbscript:)|(?i-u:union|select|insert|update|delete|drop|create|alter|exec){space}+")),
            sensitive: [
                (re(&format!(r"(?-u:\b)[0-9]{{4}}(?:{space}|-)?[0-9]{{4}}(?:{space}|-)?[0-9]{{4}}(?:{space}|-)?[0-9]{{4}}(?-u:\b)")), "[CREDIT_CARD_REDACTED]"),
                (re(r"(?-u:\b)[0-9]{3}-?[0-9]{2}-?[0-9]{4}(?-u:\b)"), "[SSN_REDACTED]"),
                (re(r"(?-u:\b)[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}(?-u:\b)"), "[EMAIL_REDACTED]"),
                (re(&format!(r"(?-u:\b)\+?1?(?:[-.]|{space})?\(?[0-9]{{3}}\)?(?:[-.]|{space})?[0-9]{{3}}(?:[-.]|{space})?[0-9]{{4}}(?-u:\b)")), "[PHONE_REDACTED]"),
                // The source's second key predicate always accepts this first
                // alphanumeric >=32 match (it is case-insensitive).
                (re(r"(?-u:\b)[A-Za-z0-9]{32,}(?-u:\b)"), "[API_KEY_REDACTED]"),
            ],
        }
    });
    &PATTERNS
}

fn without_scripts<'a>(content: &'a str, p: &Patterns) -> std::borrow::Cow<'a, str> {
    let mut result: Option<String> = None;
    let mut at = 0;
    for open in p.script_open.find_iter(content) {
        if open.start() < at { continue; }
        let Some(close) = p.script_close.find_at(content, open.end()) else { break; };
        result.get_or_insert_with(|| String::with_capacity(content.len())).push_str(&content[at..open.start()]);
        at = close.end();
    }
    match result {
        Some(mut result) => {
            result.push_str(&content[at..]);
            std::borrow::Cow::Owned(result)
        }
        None => std::borrow::Cow::Borrowed(content),
    }
}

fn sensitive(mut text: String, p: &Patterns) -> String {
    for (pattern, replacement) in &p.sensitive {
        if let std::borrow::Cow::Owned(next) = pattern.replace_all(&text, *replacement) { text = next; }
    }
    text
}

/// Chat question/history/context and general-summary input. Analysis deliberately
/// does not use this path: its source template receives raw canonical lines.
pub fn input(content: &str) -> String {
    let p = patterns();
    let scripts = without_scripts(content, p);
    let html = p.html.replace_all(&scripts, "");
    let sql = p.sql.replace_all(&html, "[FILTERED]");
    let javascript = p.javascript.replace_all(&sql, "[FILTERED]");
    let handlers = p.handler.replace_all(&javascript, "[FILTERED]");
    let normalized = p.whitespace.replace_all(&handlers, " ");
    sensitive(normalized.trim_matches(' ').to_owned(), p)
}

/// Prose final/checkpoint sanitation. Script tag *contents* and SQL words remain
/// as in source sanitizeOutput; plain-text rendering is the shell's obligation.
pub fn output(content: &str) -> String {
    let p = patterns();
    let html = p.html.replace_all(content, "");
    let javascript = p.javascript.replace_all(&html, "");
    let normalized = p.whitespace.replace_all(&javascript, " ");
    sensitive(normalized.trim_matches(' ').to_owned(), p)
}

pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| matches!(c,
        '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
        '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

/// Source ContentProcessor: trim each nonempty line, retain the original speaker
/// spelling (including surrounding whitespace), and join with one space.
pub fn transcript_text(lines: &[Line], include_speakers: bool) -> String {
    let mut text = String::new();
    for line in lines {
        let content = js_trim(&line.line);
        if content.is_empty() { continue; }
        if !text.is_empty() { text.push(' '); }
        if include_speakers && let Some(speaker) = line.speaker.as_deref().filter(|s| !js_trim(s).is_empty()) {
            text.push_str(speaker);
            text.push_str(": ");
        }
        text.push_str(content);
    }
    text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentError { Empty, TooShort, TooLong }

/// Source chat context bounds use JS string.length (UTF-16), not UTF-8 bytes.
pub fn validate_chat_context(content: &str) -> Result<(), ContentError> {
    if js_trim(content).is_empty() { return Err(ContentError::Empty); }
    match content.encode_utf16().count() {
        0..50 => Err(ContentError::TooShort),
        50..=512_000 => Ok(()),
        _ => Err(ContentError::TooLong),
    }
}

pub fn validate_input_security(content: &str) -> Result<(), ContentError> {
    if content.encode_utf16().count() > 512_000 { Err(ContentError::TooLong) } else { Ok(()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionError { Required, RawTooLong, Unsafe, Empty, TooLong, NoTranscript }

impl QuestionError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Required => "Message is required",
            Self::RawTooLong => "Message is too long. Please keep it under 100000 characters.",
            Self::Unsafe => "Message contains potentially unsafe content",
            Self::Empty => "Message cannot be empty.",
            Self::TooLong => "Message is too long.",
            Self::NoTranscript => "No transcript content available.",
        }
    }
    pub fn suggestion(self) -> Option<&'static str> {
        match self {
            Self::Empty => Some("Please enter a question about the transcript."),
            Self::TooLong => Some("Please keep your question under 10000 characters."),
            Self::NoTranscript => Some("Please wait for the transcript to be processed before asking questions."),
            _ => None,
        }
    }
}
impl std::fmt::Display for QuestionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message()) }
}
impl std::error::Error for QuestionError {}

/// AI-010: unconditional raw route checks, before ContentProcessor changes the
/// question. Source checks raw UTF-16 length, then tests the trimmed text.
pub fn validate_raw_question(raw: &str) -> Result<(), QuestionError> {
    let trimmed = js_trim(raw);
    if trimmed.is_empty() { return Err(QuestionError::Required); }
    if raw.encode_utf16().count() > 100_000 { return Err(QuestionError::RawTooLong); }
    if patterns().dangerous.is_match(trimmed) { return Err(QuestionError::Unsafe); }
    Ok(())
}

/// AI-012: run only when message validation is enabled, after source quota-check
/// precedence. The caller supplies sanitized question and raw canonical context.
pub fn validate_question(sanitized_question: &str, raw_context: &str) -> Result<(), QuestionError> {
    if js_trim(sanitized_question).is_empty() { return Err(QuestionError::Empty); }
    if sanitized_question.encode_utf16().count() > 10_000 { return Err(QuestionError::TooLong); }
    if js_trim(raw_context).is_empty() { return Err(QuestionError::NoTranscript); }
    Ok(())
}
