//! Transcript lines: the stored shape, speaker labels, display fallback,
//! ordering, edit rules and the provider normalizers.
//!
//! One row per line, times as zero-padded `HH:MM:SS` text exactly as the source
//! stores and sorts them (EDT-011). No words, confidence or paragraphs are kept
//! (TRX-031).

pub mod captions;
pub mod deepgram;
pub mod soniox;
pub mod time;

use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, JobId, LineId, Micros};
use crate::languages::Language;

/// `translation_status` (TRL-004).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TranslationStatus {
    Original,
    Translation,
}

impl TranslationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TranslationStatus::Original => "original",
            TranslationStatus::Translation => "translation",
        }
    }

    pub fn parse(text: &str) -> Option<TranslationStatus> {
        match text {
            "original" => Some(TranslationStatus::Original),
            "translation" => Some(TranslationStatus::Translation),
            _ => None,
        }
    }
}

/// A stored line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: LineId,
    pub job: JobId,
    pub account: AccountId,
    pub start_time: String,
    pub end_time: String,
    pub line: String,
    pub speaker: Option<String>,
    /// Legacy RunPod correction; never shown, kept for the API (TRX-021).
    pub corrected_text: Option<String>,
    pub split_on_speaker_change: Option<bool>,
    pub language: Option<Language>,
    pub translation_status: Option<TranslationStatus>,
    pub created_at: Micros,
}

/// A line a normalizer produced, before it has an id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewLine {
    pub start_time: String,
    pub end_time: String,
    pub line: String,
    pub speaker: Option<String>,
    pub corrected_text: Option<String>,
    pub split_on_speaker_change: Option<bool>,
    pub language: Option<Language>,
    pub translation_status: Option<TranslationStatus>,
}

/// `SPEAKER_NN` with at least two digits.
pub fn speaker_tag(n: i64) -> String {
    format!("SPEAKER_{n:02}")
}

/// EDT-004: `SPEAKER_NN` reads "Speaker N+1" (the word is translated by the
/// shell's locale table); anything else is shown as stored; none is "N/A".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpeakerLabel {
    Numbered {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        number: i64,
    },
    Named { name: String },
    None,
}

pub fn speaker_label(speaker: Option<&str>) -> SpeakerLabel {
    let Some(s) = speaker.filter(|s| !s.is_empty()) else { return SpeakerLabel::None };
    // `formatSpeakerLabel`: exactly `^SPEAKER_(\d{2})$`.
    if let Some(digits) = s.strip_prefix("SPEAKER_") && digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()) && let Ok(n) = digits.parse::<i64>() {
        return SpeakerLabel::Numbered { number: n + 1 };
    }
    SpeakerLabel::Named { name: s.to_string() }
}

/// EDT-009 (`applySpeakerFallback`): a line whose speaker is null shows the
/// previous line's speaker. An empty string is a value, not null. Display only.
pub fn display_speakers(lines: &[Line]) -> Vec<Option<String>> {
    let mut previous: Option<String> = None;
    lines
        .iter()
        .map(|l| match &l.speaker {
            Some(s) => {
                previous = Some(s.clone());
                Some(s.clone())
            }
            None => previous.clone(),
        })
        .collect()
}

/// EDT-011: ordered by `start_time` as text; equal times keep id order.
pub fn sort_lines(lines: &mut [Line]) {
    lines.sort_by(|a, b| a.start_time.cmp(&b.start_time).then(a.id.cmp(&b.id)));
}

/// EDT-011: a repeated id replaces the line, then the list is re-sorted.
pub fn merge_lines(existing: &mut Vec<Line>, incoming: Vec<Line>) {
    for line in incoming {
        match existing.iter_mut().find(|l| l.id == line.id) {
            Some(slot) => *slot = line,
            None => existing.push(line),
        }
    }
    sort_lines(existing);
}

/// EDT-007: the distinct non-empty speaker values of a job, first-seen order.
pub fn distinct_speakers(lines: &[Line]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for s in lines.iter().filter_map(|l| l.speaker.as_deref()).filter(|s| !s.is_empty()) {
        if !seen.iter().any(|x| x == s) {
            seen.push(s.to_string());
        }
    }
    seen
}

/// EDT-002/005/006: the user edits a transcript can receive. Last write wins
/// (D7): no revision, no validation, an empty text is saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineEdit {
    /// Writes only `line` for that line id; `corrected_text` is untouched.
    Text { line: LineId, text: String },
    /// Changes the speaker of that one line.
    Speaker { line: LineId, speaker: String },
    /// Renames every line of the job whose speaker equals `from`.
    RenameSpeaker { job: JobId, from: String, to: String },
}

/// EDT-012 (`extractLanguageOptions`): tabs from lines that carry both a
/// language and a translation status; originals first, then by language name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LanguageTab {
    pub language: Language,
    pub translation_status: TranslationStatus,
}

pub fn language_tabs(lines: &[Line]) -> Vec<LanguageTab> {
    let mut tabs: Vec<LanguageTab> = Vec::new();
    for l in lines {
        if let (Some(language), Some(translation_status)) = (l.language, l.translation_status) {
            let tab = LanguageTab { language, translation_status };
            if !tabs.contains(&tab) {
                tabs.push(tab);
            }
        }
    }
    tabs.sort_by(|a, b| {
        let a_orig = a.translation_status == TranslationStatus::Original;
        let b_orig = b.translation_status == TranslationStatus::Original;
        b_orig.cmp(&a_orig).then_with(|| a.language.name().cmp(b.language.name()))
    });
    tabs
}

#[cfg(test)]
mod tests;
