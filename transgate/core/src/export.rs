//! Transcript exports (EXP-001..016), ported from
//! `frontend/src/utils/transcriptFileSaver.ts`. Core decides every file, name
//! and text; the host's byte adapter only writes the zip and the OOXML parts
//! for `WordDocument`. File names are sanitized here; the 30-second speaker
//! grouping, subtitle splitting and CSV quoting are done here.

use serde::{Deserialize, Serialize};

use crate::transcript::time::js_parse_int;
use crate::transcript::{Line, SpeakerLabel, speaker_label};

/// Translated labels the shell supplies from its locale table; English otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportLabels {
    pub full_text: String,
    pub speaker: String,
    pub speakers: String,
    pub timestamps: String,
    pub text: String,
    pub start_time: String,
    pub end_time: String,
}

impl Default for ExportLabels {
    fn default() -> Self {
        ExportLabels {
            full_text: "Full Text".into(),
            speaker: "Speaker".into(),
            speakers: "Speakers".into(),
            timestamps: "Timestamps".into(),
            text: "Text".into(),
            start_time: "Start Time".into(),
            end_time: "End Time".into(),
        }
    }
}

/// The editor toggles that shape an export (EXP-012).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOptions {
    pub show_timestamps: bool,
    pub show_speakers: bool,
    /// Forces SRT/VTT in (EXP-011).
    pub video_source: bool,
    /// Legacy `excludeTimestampFormats`; no caller sets it, kept for the shape.
    pub exclude_timestamp_formats: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordRun {
    pub text: String,
    pub font: String,
    pub bold: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordParagraph {
    pub runs: Vec<WordRun>,
    /// Spacing in twips (the `docx` package's units).
    pub spacing_before: Option<u32>,
    pub spacing_after: Option<u32>,
}

/// An OOXML document. The source writes it under a `.doc` name (JOB-D09 kept).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordDocument {
    pub creator: String,
    pub paragraphs: Vec<WordParagraph>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryContent {
    Text { text: String },
    Word { document: WordDocument },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportEntry {
    /// File name inside `folder`.
    pub name: String,
    pub content: EntryContent,
}

/// One zip download: `zip_name` holding `folder/` with `entries` (EXP-002).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportBundle {
    pub zip_name: String,
    pub folder: String,
    pub entries: Vec<ExportEntry>,
}

/// EXP-003 `removeFileExtension`: everything before the last dot.
pub fn remove_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// EXP-003 `sanitizeFilename`: drop `/ \ : * ? " < > |`, collapse whitespace, trim.
pub fn sanitize_filename(name: &str) -> String {
    let kept: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// EXP-007: the last dot-separated part, lower case, is a video extension.
pub fn is_video_source(job_name: &str) -> bool {
    let ext = job_name.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "flv" | "wmv" | "ts" | "3gp")
}

pub fn base_name(job_name: Option<&str>) -> String {
    sanitize_filename(remove_extension(job_name.unwrap_or("")))
}

/// `formatSpeakerLabel` with the translated word.
pub fn speaker_text(speaker: Option<&str>, labels: &ExportLabels) -> String {
    match speaker_label(speaker) {
        SpeakerLabel::Numbered { number } => format!("{} {number}", labels.speaker),
        SpeakerLabel::Named { name } => name,
        SpeakerLabel::None => speaker.unwrap_or("").to_string(),
    }
}

fn trimmed(line: &Line) -> String {
    line.line.trim().to_string()
}

/// EXP-004: all lines trimmed and joined by one space.
pub fn full_text(lines: &[Line]) -> String {
    lines.iter().map(trimmed).collect::<Vec<_>>().join(" ")
}

/// A subtitle cue: stored times, or `HH:MM:SS,mmm` for pieces of split lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub start: String,
    pub end: String,
    pub text: String,
}

/// `secondsToTimestamp`: `HH:MM:SS,mmm`.
pub fn seconds_to_timestamp(seconds: f64) -> String {
    let hours = (seconds / 3600.0).floor() as i64;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as i64;
    let secs = (seconds % 60.0).floor() as i64;
    let millis = ((seconds % 1.0) * 1000.0 + 0.5).floor() as i64;
    format!("{hours:02}:{minutes:02}:{secs:02},{millis:03}")
}

/// `timestampToSeconds`: `HH:MM:SS[,mmm]`; anything else is 0.
pub fn timestamp_to_seconds(text: &str) -> f64 {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 3 {
        return 0.0;
    }
    let (secs, millis) = parts[2].split_once(',').unwrap_or((parts[2], "0"));
    let millis = js_parse_int(if millis.is_empty() { "0" } else { millis });
    js_parse_int(parts[0]) * 3600.0 + js_parse_int(parts[1]) * 60.0 + js_parse_int(secs) + millis / 1000.0
}

/// EXP-008 `splitLongTranscriptLines`: lines over 6 s become pieces of about
/// 4.5 s, words split proportionally, or UTF-16 units when the text has no space.
pub fn subtitle_cues(lines: &[Line]) -> Vec<Cue> {
    let mut cues = Vec::new();
    for line in lines {
        let whole = Cue { start: line.start_time.clone(), end: line.end_time.clone(), text: line.line.clone() };
        if line.start_time.is_empty() || line.end_time.is_empty() || line.line.is_empty() {
            cues.push(whole);
            continue;
        }
        let start = timestamp_to_seconds(&line.start_time);
        let end = timestamp_to_seconds(&line.end_time);
        let duration = end - start;
        if duration <= 6.0 {
            cues.push(whole);
            continue;
        }
        let text = line.line.trim();
        let n = (duration / 4.5).ceil() as usize;
        let piece = duration / n as f64;
        let has_spaces = text.contains(' ');
        let words: Vec<&str> = text.split(' ').collect();
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut produced = Vec::new();
        for i in 0..n {
            let seg_start = start + i as f64 * piece;
            let seg_end = (start + (i + 1) as f64 * piece).min(end);
            let seg_text = if has_spaces {
                let a = (i * words.len()) / n;
                let b = ((i + 1) * words.len()) / n;
                words[a..b].join(" ")
            } else {
                let a = (i * units.len()) / n;
                let b = ((i + 1) * units.len()) / n;
                String::from_utf16_lossy(&units[a..b])
            };
            if !seg_text.trim().is_empty() {
                produced.push(Cue {
                    start: seconds_to_timestamp(seg_start),
                    end: seconds_to_timestamp(seg_end),
                    text: seg_text.trim().to_string(),
                });
            }
        }
        if produced.is_empty() {
            cues.push(whole);
        } else {
            cues.extend(produced);
        }
    }
    cues
}

fn cue_text(text: &str) -> String {
    text.trim().replace("-->", "->")
}

/// EXP-008 `toSRT` joined by newlines.
pub fn srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, c)| format!("{}\n{} --> {}\n{}\n", i + 1, c.start, c.end, cue_text(&c.text)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// EXP-009 `WEBVTT` header, then `toVTT` cues.
pub fn vtt(cues: &[Cue]) -> String {
    let body = cues.iter().map(|c| format!("{} --> {}\n{}\n", c.start, c.end, cue_text(&c.text))).collect::<Vec<_>>().join("\n");
    format!("WEBVTT\n\n{body}")
}

fn csv_value(value: &str) -> String {
    if value.contains('"') || value.contains(',') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// EXP-010 `toCSV`.
pub fn csv(lines: &[Line], labels: &ExportLabels) -> String {
    let header = format!("{},{},{},{}\n", labels.speaker, labels.start_time, labels.end_time, labels.text);
    let rows = lines
        .iter()
        .map(|l| {
            let speaker = speaker_text(l.speaker.as_deref(), labels);
            format!("{},{},{},{}", csv_value(&speaker), csv_value(&l.start_time), csv_value(&l.end_time), csv_value(&trimmed(l)))
        })
        .collect::<Vec<_>>()
        .join("\n");
    header + &rows
}

/// EXP-014 `toFormattedText` for the clipboard: `[start - end] Speaker N: text`.
pub fn clipboard_text(lines: &[Line], labels: &ExportLabels) -> String {
    lines
        .iter()
        .map(|l| {
            let mut parts: Vec<String> = Vec::new();
            let start = l.start_time.split(',').next().unwrap_or("");
            let end = l.end_time.split(',').next().unwrap_or("");
            if !start.is_empty() {
                if end.is_empty() {
                    parts.push(format!("[{start}]"));
                } else {
                    parts.push(format!("[{start} - {end}]"));
                }
            }
            if let Some(s) = l.speaker.as_deref().filter(|s| !s.is_empty()) {
                parts.push(format!("{}:", speaker_text(Some(s), labels)));
            }
            parts.push(trimmed(l));
            parts.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const CREATOR: &str = "Transgate";

fn run(text: String, font: &str, bold: bool) -> WordRun {
    WordRun { text, font: font.into(), bold }
}

/// `toDocPlain`
pub fn word_plain(lines: &[Line]) -> WordDocument {
    WordDocument {
        creator: CREATOR.into(),
        paragraphs: lines
            .iter()
            .map(|l| WordParagraph { runs: vec![run(trimmed(l), "Arial", false)], spacing_before: None, spacing_after: Some(120) })
            .collect(),
    }
}

/// `toDoc`: a bold Consolas time, then the Arial text.
pub fn word_timestamped(lines: &[Line]) -> WordDocument {
    WordDocument {
        creator: CREATOR.into(),
        paragraphs: lines
            .iter()
            .map(|l| WordParagraph {
                runs: vec![
                    run(format!("{} ", l.start_time.split(',').next().unwrap_or("")), "Consolas", true),
                    run(trimmed(l), "Arial", false),
                ],
                spacing_before: None,
                spacing_after: Some(120),
            })
            .collect(),
    }
}

/// EXP-006 speaker blocks: a new block when the speaker changes or 30 seconds
/// passed since the block started.
pub fn speaker_blocks(lines: &[Line], labels: &ExportLabels) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut current_speaker = String::new();
    let mut current_text = String::new();
    let mut block_start = 0.0;
    for (i, l) in lines.iter().enumerate() {
        let speaker = speaker_text(l.speaker.as_deref(), labels);
        let start = timestamp_to_seconds(&l.start_time);
        let text = trimmed(l);
        let new_block = !current_text.is_empty() && (speaker != current_speaker || start - block_start >= 30.0);
        if new_block {
            blocks.push((current_speaker.clone(), current_text.trim().to_string()));
            current_speaker = speaker;
            current_text = text;
            block_start = start;
        } else {
            if current_speaker.is_empty() {
                current_speaker = speaker;
                block_start = start;
            }
            if !current_text.is_empty() {
                current_text.push(' ');
            }
            current_text.push_str(&text);
        }
        if i == lines.len() - 1 && !current_text.is_empty() {
            blocks.push((current_speaker.clone(), current_text.trim().to_string()));
        }
    }
    blocks
}

/// `toDocWithSpeakers`
pub fn word_speakers(lines: &[Line], labels: &ExportLabels) -> WordDocument {
    let mut paragraphs = Vec::new();
    for (i, (speaker, text)) in speaker_blocks(lines, labels).into_iter().enumerate() {
        paragraphs.push(WordParagraph {
            runs: vec![run(format!("[{speaker}]"), "Arial", true)],
            spacing_before: Some(if i > 0 { 200 } else { 0 }),
            spacing_after: Some(60),
        });
        paragraphs.push(WordParagraph { runs: vec![run(text, "Arial", false)], spacing_before: None, spacing_after: Some(200) });
    }
    WordDocument { creator: CREATOR.into(), paragraphs }
}

/// `toZip` (EXP-002..010, EXP-012). `job_speakers` is `jobs.speakers`.
pub fn bundle(base: &str, zip_stem: &str, lines: &[Line], job_speakers: bool, options: &ExportOptions, labels: &ExportLabels) -> ExportBundle {
    let mut entries = vec![ExportEntry { name: format!("{}.txt", labels.full_text), content: EntryContent::Text { text: full_text(lines) } }];
    if !options.exclude_timestamp_formats {
        let document = if options.show_speakers && job_speakers {
            word_speakers(lines, labels)
        } else if options.show_timestamps {
            word_timestamped(lines)
        } else {
            word_plain(lines)
        };
        entries.push(ExportEntry { name: format!("{base}.doc"), content: EntryContent::Word { document } });
    }
    if options.video_source && !options.exclude_timestamp_formats {
        let cues = subtitle_cues(lines);
        entries.push(ExportEntry { name: format!("{base}.srt"), content: EntryContent::Text { text: srt(&cues) } });
        entries.push(ExportEntry { name: format!("{base}.vtt"), content: EntryContent::Text { text: vtt(&cues) } });
    }
    if job_speakers && !options.exclude_timestamp_formats {
        entries.push(ExportEntry {
            name: format!("{} {} {}.csv", labels.speakers, labels.timestamps, labels.text),
            content: EntryContent::Text { text: csv(lines, labels) },
        });
    }
    ExportBundle { zip_name: format!("{zip_stem}_transcript.zip"), folder: format!("{base}_transcript"), entries }
}

/// The header "Download" (EXP-001/012): the editor's toggles, video by extension.
pub fn wysiwyg(job_name: Option<&str>, job_speakers: bool, lines: &[Line], show_timestamps: bool, show_speakers: bool, labels: &ExportLabels) -> ExportBundle {
    let base = base_name(job_name);
    let options = ExportOptions {
        show_timestamps,
        show_speakers,
        video_source: is_video_source(job_name.unwrap_or("")),
        exclude_timestamp_formats: false,
    };
    bundle(&base, &base, lines, job_speakers, &options, labels)
}

/// EXP-011 "Download all formats": time on, subtitles forced, the job's speaker flag.
pub fn all_formats(job_name: Option<&str>, job_speakers: bool, lines: &[Line], labels: &ExportLabels) -> ExportBundle {
    let base = base_name(job_name);
    let options = ExportOptions { show_timestamps: true, show_speakers: job_speakers, video_source: true, exclude_timestamp_formats: false };
    bundle(&base, &format!("{base}_all_formats"), lines, job_speakers, &options, labels)
}

/// EXP-016: `transcript_downloaded` properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadEvent {
    pub job_id: String,
    /// `wysiwyg` or `all`.
    pub download_format: String,
    pub word_count: usize,
    /// `video` or `audio`.
    pub media_type: String,
    pub timestamps_enabled: bool,
    pub speakers_enabled: bool,
}

pub fn word_count(lines: &[Line]) -> usize {
    lines.iter().map(|l| l.line.split_whitespace().count()).sum()
}

pub fn download_event(job_id: i64, all: bool, job_name: Option<&str>, lines: &[Line], timestamps: bool, speakers: bool) -> DownloadEvent {
    DownloadEvent {
        job_id: job_id.to_string(),
        download_format: if all { "all" } else { "wysiwyg" }.into(),
        word_count: word_count(lines),
        media_type: if is_video_source(job_name.unwrap_or("")) { "video" } else { "audio" }.into(),
        timestamps_enabled: timestamps,
        speakers_enabled: speakers,
    }
}

#[cfg(test)]
mod tests;
