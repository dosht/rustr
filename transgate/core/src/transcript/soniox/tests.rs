use super::*;
use crate::jobs::testing::{token, translated};

fn lines(tokens: &[SonioxToken], speakers: bool) -> Vec<(String, String, String, Option<String>)> {
    normalize(tokens, speakers).into_iter().map(|l| (l.start_time, l.end_time, l.line, l.speaker)).collect()
}

// TRX-024: sentence punctuation ends a line; times carry the 500 ms offset (TRX-026).
#[test]
fn sentence_punctuation_splits_lines() {
    let t = [
        token("Hello", 0.0, 400.0, None),
        token(" world.", 400.0, 1600.0, None),
        token(" Next", 1700.0, 2100.0, None),
        token(" part", 2100.0, 2600.0, None),
        token(" here", 2600.0, 3600.0, None),
    ];
    let out = lines(&t, false);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0], ("00:00:00".into(), "00:00:02".into(), "Hello world.".into(), None));
    assert_eq!(out[1].2, "Next part here");
    assert_eq!(out[1].0, "00:00:02");
}

// TRX-024: Arabic and CJK sentence marks, clause marks only with a gap over 300 ms.
#[test]
fn clause_marks_need_a_gap() {
    let near = [token("one,", 0.0, 1000.0, None), token(" two three four", 1100.0, 3000.0, None)];
    assert_eq!(lines(&near, false).len(), 1);
    let far = [token("one two three,", 0.0, 1500.0, None), token(" four five six", 1900.0, 4000.0, None)];
    assert_eq!(lines(&far, false).len(), 2);
    let cjk = [token("你好。", 0.0, 1500.0, None), token("再见吧朋友", 1500.0, 3500.0, None)];
    assert_eq!(lines(&cjk, false).len(), 2);
}

// TRX-024: a gap over two seconds splits even mid-sentence.
#[test]
fn long_gap_splits() {
    let t = [token("one two three", 0.0, 1500.0, None), token(" four five six", 3600.0, 5000.0, None)];
    assert_eq!(lines(&t, false).len(), 2);
}

// TRX-024/027: a speaker change splits only with speakers on; Soniox 1 is SPEAKER_00.
#[test]
fn speaker_changes_split_when_enabled() {
    let t = [token("one two three", 0.0, 1500.0, Some(1)), token(" four five six", 1600.0, 3000.0, Some(2))];
    let on = lines(&t, true);
    assert_eq!(on.len(), 2);
    assert_eq!(on[0].3.as_deref(), Some("SPEAKER_00"));
    assert_eq!(on[1].3.as_deref(), Some("SPEAKER_01"));
    let off = lines(&t, false);
    assert_eq!(off.len(), 1);
    // TRX-027: the last speaker seen in the line.
    assert_eq!(off[0].3.as_deref(), Some("SPEAKER_01"));
}

// TRX-024: audio events are skipped.
#[test]
fn audio_events_are_skipped() {
    let mut event = token("[music]", 0.0, 2000.0, None);
    event.is_audio_event = Some(true);
    let t = [event, token("hello there friend", 2000.0, 4000.0, None)];
    let out = lines(&t, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].2, "hello there friend");
}

// TRX-025: a line that rounds to zero seconds joins the previous one.
#[test]
fn short_lines_merge() {
    let t = [
        token("This is the first sentence.", 0.0, 3000.0, None),
        token(" Yes.", 3100.0, 3300.0, None),
        token(" And then a long second sentence follows.", 3600.0, 7000.0, None),
    ];
    let out = lines(&t, false);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].2, "This is the first sentence. Yes.");
}

// TRX-025: a punctuation-only first line is dropped until content exists.
#[test]
fn leading_punctuation_only_line_is_dropped() {
    let t = [token(".", 0.0, 100.0, None), token(" Hello there everyone today.", 4000.0, 7000.0, None)];
    let out = lines(&t, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].2, "Hello there everyone today.");
}

// TRX-028/TRL-004: paired original and translation lines share the time window.
#[test]
fn translation_pairs_share_windows() {
    let t = [
        translated("Hello everyone", 1000.0, 2500.0, "en", "original"),
        translated("Hallo zusammen", 0.0, 0.0, "de", "translation"),
        translated(" Good morning", 3000.0, 4000.0, "en", "original"),
        translated("Guten Morgen", 0.0, 0.0, "de", "translation"),
    ];
    let out = normalize(&t, false);
    assert_eq!(out.len(), 4);
    assert_eq!(out[0].translation_status, Some(TranslationStatus::Original));
    assert_eq!(out[0].language.map(|l| l.name()), Some("English"));
    assert_eq!(out[1].translation_status, Some(TranslationStatus::Translation));
    assert_eq!(out[1].language.map(|l| l.name()), Some("German"));
    // The first window ends at the next original's start: 1000 to 3000 ms, +500 offset.
    assert_eq!((out[0].start_time.as_str(), out[0].end_time.as_str()), ("00:00:01", "00:00:03"));
    assert_eq!((out[1].start_time.as_str(), out[1].end_time.as_str()), ("00:00:01", "00:00:03"));
    // The last window ends at its group end.
    assert_eq!(out[3].end_time, "00:00:04");
}

// TRX-024: the gap thresholds are strict ("over"): a clause mark splits at 301 ms, not at
// exactly 300; any gap splits at 2001 ms, not at exactly 2000. One case per mark class.
#[test]
fn trx_024_gap_boundaries_and_every_punctuation_class() {
    let pair = |first: &str, gap: f64| {
        let t = [token(&format!("one two {first}"), 0.0, 1500.0, None), token(" four five six", 1500.0 + gap, 3500.0 + gap, None)];
        lines(&t, false).len()
    };
    for clause in [",", ";", "،", "؛"] {
        assert_eq!((pair(&format!("three{clause}"), 300.0), pair(&format!("three{clause}"), 301.0)), (1, 2), "{clause}");
    }
    for sentence in [".", "!", "?", "؟", "。", "！", "？"] {
        assert_eq!(pair(&format!("three{sentence}"), 0.0), 2, "{sentence}");
    }
    assert_eq!((pair("three", 2000.0), pair("three", 2001.0)), (1, 2), "no punctuation: only a gap over two seconds");
}

fn seg(start_ms: f64, end_ms: f64, line: &str, language: Option<&str>, status: Option<TranslationStatus>) -> Segment {
    segment(start_ms, end_ms, line.into(), None, false, language, status)
}

fn spans(segments: &[Segment]) -> Vec<(&str, &str, &str)> {
    segments.iter().map(|s| (s.start_time.as_str(), s.end_time.as_str(), s.line.as_str())).collect()
}

// TRX-025: merging never crosses a language or translation status change, except that a
// punctuation-only line still joins the previous line.
#[test]
fn trx_025_merges_stop_at_language_and_status_changes() {
    let english = Some("en");
    let original = Some(TranslationStatus::Original);
    let short_other = merge_short(vec![
        seg(0.0, 3000.0, "Hello there everyone", english, original),
        seg(3000.0, 3400.0, "Hallo", Some("de"), Some(TranslationStatus::Translation)),
    ]);
    assert_eq!(short_other.len(), 2, "a short line in another language stays its own line");
    let punctuation = merge_short(vec![seg(0.0, 3000.0, "Hello there everyone", english, original), seg(3000.0, 3400.0, ".", Some("fr"), original)]);
    assert_eq!(spans(&punctuation), vec![("00:00:00", "00:00:03", "Hello there everyone.")]);
}

// TRX-025: a zero-length line left over takes its neighbour's edge: the first one ends at
// the next line's start; a later one starts at the previous end and lasts at least a second.
#[test]
fn trx_025_zero_length_remainders_take_the_neighbour_edge() {
    let de = Some("de");
    let translation = Some(TranslationStatus::Translation);
    let original = Some(TranslationStatus::Original);
    let first = merge_short(vec![seg(0.0, 100.0, "Hello there friend", Some("en"), original), seg(4000.0, 7000.0, "Hallo da Freund jetzt", de, translation)]);
    assert_eq!(spans(&first)[0], ("00:00:00", "00:00:04", "Hello there friend"));
    let middle = merge_short(vec![
        seg(0.0, 3000.0, "one two three four", Some("en"), original),
        seg(5000.0, 5100.0, "Hallo da Freund", de, translation),
        seg(8000.0, 11000.0, "five six seven eight", Some("en"), original),
    ]);
    assert_eq!(spans(&middle)[1], ("00:00:03", "00:00:05", "Hallo da Freund"));
    let tight = merge_short(vec![
        seg(0.0, 3000.0, "one two three four", Some("en"), original),
        seg(3000.0, 3100.0, "Hallo da Freund", de, translation),
        seg(8000.0, 11000.0, "five six seven eight", Some("en"), original),
    ]);
    assert_eq!(spans(&tight)[1], ("00:00:03", "00:00:04", "Hallo da Freund"), "at least one second after the previous end");
}

// TRX-028, TRL-004: originals with the same start share their window evenly; the last
// window with no usable group end defaults to 2 s per group; each translation reuses its
// original's range and sorts right after it.
#[test]
fn trx_028_trl_004_same_start_groups_split_a_default_two_second_window() {
    let t = [
        translated("First", 1000.0, 1500.0, "en", "original"),
        translated("Erste", 0.0, 0.0, "de", "translation"),
        translated(" Second", 1000.0, 0.0, "en", "original"),
        translated("Zweite", 0.0, 0.0, "de", "translation"),
    ];
    let out: Vec<_> = normalize(&t, false)
        .into_iter()
        .map(|l| (l.start_time, l.end_time, l.line, l.translation_status))
        .collect();
    let o = Some(TranslationStatus::Original);
    let tr = Some(TranslationStatus::Translation);
    assert_eq!(
        out,
        vec![
            ("00:00:01".into(), "00:00:03".into(), "First".into(), o),
            ("00:00:01".into(), "00:00:03".into(), "Erste".into(), tr),
            ("00:00:03".into(), "00:00:05".into(), "Second".into(), o),
            ("00:00:03".into(), "00:00:05".into(), "Zweite".into(), tr),
        ]
    );
}

// TRX-029: plain transcription lines carry no language or translation status.
#[test]
fn plain_lines_have_no_language() {
    let out = normalize(&[token("hello there friend.", 0.0, 2000.0, None)], false);
    assert_eq!(out[0].language, None);
    assert_eq!(out[0].translation_status, None);
    assert_eq!(out[0].split_on_speaker_change, Some(false));
}

// BIL-035 fallback: the largest end, not the last token's (translation tokens carry 0).
#[test]
fn max_token_end_ignores_order() {
    let t = [token("a", 0.0, 9000.0, None), translated("b", 0.0, 0.0, "de", "translation")];
    assert_eq!(max_token_end_ms(&t), Some(9000.0));
    assert_eq!(max_token_end_ms(&[]), None);
}
