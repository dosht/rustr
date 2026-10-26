//! Deepgram result to lines, ported from `mapDeepgramResponseToSegments`
//! (`frontend/src/utils/deepgram.ts:243-319`, TRX-030).
//!
//! One line per sentence of the first alternative's paragraphs, speaker from
//! the paragraph. Without paragraphs, words are grouped. The word fallback keeps
//! the captured `parseFloat("HH:MM:SS")` comparison (JOB-D03): no correction is
//! approved, so the gap is measured against the stored end time's hours.
//! Deepgram lines carry no language and no translation status.

use media_ports::DeepgramTranscript;

use super::time::{js_parse_float_prefix, seconds_time};
use super::{NewLine, speaker_tag};

fn line(start: f64, end: f64, text: String, speaker: Option<i64>, speakers: bool) -> NewLine {
    NewLine {
        start_time: seconds_time(start),
        end_time: seconds_time(end),
        line: text,
        speaker: speaker.map(speaker_tag),
        corrected_text: None,
        split_on_speaker_change: Some(speakers),
        language: None,
        translation_status: None,
    }
}

pub fn normalize(result: &DeepgramTranscript, speakers: bool) -> Vec<NewLine> {
    let Some(alternative) = result.results.channels.first().and_then(|c| c.alternatives.first()) else {
        return vec![];
    };
    let paragraphs = alternative.paragraphs.as_ref().map(|p| p.paragraphs.as_slice()).unwrap_or(&[]);
    let mut lines = Vec::new();
    if !paragraphs.is_empty() {
        for paragraph in paragraphs {
            for sentence in &paragraph.sentences {
                lines.push(line(sentence.start, sentence.end, sentence.text.clone(), paragraph.speaker, speakers));
            }
        }
        return lines;
    }
    let mut current: Option<NewLine> = None;
    for word in &alternative.words {
        // The source concatenates `punctuated_word`; the request always sets
        // `punctuate=true`, so a missing value falls back to the raw word.
        let text = word.punctuated_word.clone().unwrap_or_else(|| word.word.clone());
        match current.as_mut() {
            None => current = Some(line(word.start, word.end, text, word.speaker, speakers)),
            Some(seg) => {
                let gap = word.start - js_parse_float_prefix(&seg.end_time);
                let speaker_change = match (word.speaker, seg.speaker.as_deref()) {
                    (Some(s), Some(current)) => format!("{s:02}") != current.replace("SPEAKER_", ""),
                    _ => false,
                };
                if gap > 2.0 || speaker_change {
                    lines.push(current.take().expect("segment"));
                    current = Some(line(word.start, word.end, text, word.speaker, speakers));
                } else {
                    seg.line.push(' ');
                    seg.line.push_str(&text);
                    seg.end_time = seconds_time(word.end);
                }
            }
        }
    }
    if let Some(seg) = current {
        lines.push(seg);
    }
    lines
}

#[cfg(test)]
mod tests;
