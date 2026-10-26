use media_ports::{DeepgramWord, ProviderPayload};

use super::*;
use crate::jobs::testing::deepgram;

fn transcript(sentences: &[(&str, f64, f64, Option<i64>)]) -> DeepgramTranscript {
    match deepgram("req", Some(10.0), sentences).payload {
        ProviderPayload::Deepgram(t) => t,
        ProviderPayload::Soniox { .. } => unreachable!(),
    }
}

fn word(w: &str, start: f64, end: f64, speaker: Option<i64>) -> DeepgramWord {
    DeepgramWord { word: w.into(), start, end, punctuated_word: None, speaker, confidence: None, extra: Default::default() }
}

// TRX-030: one line per paragraph sentence, floored seconds, paragraph speaker.
#[test]
fn one_line_per_sentence() {
    let t = transcript(&[("Hello there.", 0.4, 2.9, Some(0)), ("Second speaker.", 3661.2, 3663.0, Some(1))]);
    let lines = normalize(&t, true);
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].start_time.as_str(), lines[0].end_time.as_str()), ("00:00:00", "00:00:02"));
    assert_eq!(lines[0].speaker.as_deref(), Some("SPEAKER_00"));
    assert_eq!((lines[1].start_time.as_str(), lines[1].speaker.as_deref()), ("01:01:01", Some("SPEAKER_01")));
    assert_eq!(lines[1].split_on_speaker_change, Some(true));
    assert_eq!(lines[1].language, None);
    assert_eq!(lines[1].translation_status, None);
}

// JOB-D03 kept: the word fallback measures the gap against parseFloat("HH:MM:SS"),
// which is the hours field, so any word after two seconds starts a new line.
#[test]
fn word_fallback_keeps_the_parse_float_quirk() {
    let mut t = transcript(&[]);
    let alt = &mut t.results.channels[0].alternatives[0];
    alt.paragraphs = None;
    alt.words = vec![word("one", 0.0, 0.5, None), word("two", 0.6, 1.0, None), word("three", 2.5, 2.9, None), word("four", 3.0, 3.2, None)];
    let lines = normalize(&t, false);
    let texts: Vec<&str> = lines.iter().map(|l| l.line.as_str()).collect();
    assert_eq!(texts, ["one two", "three", "four"]);
}

// TRX-030: a speaker change splits the word fallback; punctuated_word wins.
#[test]
fn word_fallback_splits_on_speaker() {
    let mut t = transcript(&[]);
    let alt = &mut t.results.channels[0].alternatives[0];
    alt.paragraphs = None;
    let mut hi = word("hi", 0.0, 0.5, Some(0));
    hi.punctuated_word = Some("Hi,".into());
    alt.words = vec![hi, word("there", 0.6, 1.0, Some(0)), word("yes", 1.1, 1.5, Some(1))];
    let lines = normalize(&t, true);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].line, "Hi, there");
    assert_eq!(lines[1].speaker.as_deref(), Some("SPEAKER_01"));
}

// No alternative means no lines; the caller fails with "No transcription segments found".
#[test]
fn empty_results_give_no_lines() {
    let mut t = transcript(&[]);
    t.results.channels.clear();
    assert!(normalize(&t, false).is_empty());
}
