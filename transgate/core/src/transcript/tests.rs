use super::*;
use crate::presentation::fixtures::line;

fn tagged(id: i64, start: &str, language: &str, status: TranslationStatus) -> Line {
    let mut l = line(id, 1, start, start, "x", None);
    l.language = Language::from_name(language);
    l.translation_status = Some(status);
    l
}

// EDT-004: exactly SPEAKER_ and two digits; anything else shows as stored.
#[test]
fn speaker_labels() {
    assert_eq!(speaker_label(Some("SPEAKER_00")), SpeakerLabel::Numbered { number: 1 });
    assert_eq!(speaker_label(Some("SPEAKER_11")), SpeakerLabel::Numbered { number: 12 });
    for named in ["SPEAKER_1", "SPEAKER_100", "speaker_00", "Alice", "SPEAKER_0a"] {
        assert_eq!(speaker_label(Some(named)), SpeakerLabel::Named { name: named.into() }, "{named}");
    }
    assert_eq!(speaker_label(Some("")), SpeakerLabel::None);
    assert_eq!(speaker_label(None), SpeakerLabel::None);
}

// EDT-009: only null falls back to the previous speaker; an empty string is kept.
#[test]
fn display_fallback_only_for_null() {
    let lines = vec![
        line(1, 1, "00:00:00", "00:00:01", "a", None),
        line(2, 1, "00:00:01", "00:00:02", "b", Some("SPEAKER_00")),
        line(3, 1, "00:00:02", "00:00:03", "c", None),
        line(4, 1, "00:00:03", "00:00:04", "d", Some("")),
        line(5, 1, "00:00:04", "00:00:05", "e", None),
    ];
    let shown = display_speakers(&lines);
    assert_eq!(shown, [None, Some("SPEAKER_00".into()), Some("SPEAKER_00".into()), Some("".into()), Some("".into())]);
}

// EDT-011: text order of HH:MM:SS, ties by id; merging replaces by id.
#[test]
fn ordering_and_merge() {
    let mut lines = vec![line(3, 1, "00:00:10", "x", "c", None), line(1, 1, "00:00:10", "x", "a", None), line(2, 1, "00:00:02", "x", "b", None)];
    sort_lines(&mut lines);
    assert_eq!(lines.iter().map(|l| l.id.0).collect::<Vec<_>>(), [2, 1, 3]);
    merge_lines(&mut lines, vec![line(1, 1, "00:00:01", "x", "moved", None), line(4, 1, "00:00:05", "x", "new", None)]);
    assert_eq!(lines.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(), ["moved", "b", "new", "c"]);
}

// EDT-007
#[test]
fn distinct_speakers_first_seen() {
    let lines = vec![
        line(1, 1, "a", "a", "", Some("SPEAKER_01")),
        line(2, 1, "a", "a", "", Some("")),
        line(3, 1, "a", "a", "", Some("SPEAKER_00")),
        line(4, 1, "a", "a", "", Some("SPEAKER_01")),
    ];
    assert_eq!(distinct_speakers(&lines), ["SPEAKER_01", "SPEAKER_00"]);
}

// EDT-012: originals first, then translations, each by language name.
#[test]
fn language_tabs_order() {
    let lines = vec![
        tagged(1, "00:00:00", "Spanish", TranslationStatus::Translation),
        tagged(2, "00:00:00", "English", TranslationStatus::Original),
        tagged(3, "00:00:01", "German", TranslationStatus::Translation),
        tagged(4, "00:00:01", "English", TranslationStatus::Original),
        line(5, 1, "00:00:02", "00:00:03", "plain", None),
    ];
    let tabs: Vec<(&str, TranslationStatus)> = language_tabs(&lines).iter().map(|t| (t.language.name(), t.translation_status)).collect();
    assert_eq!(
        tabs,
        [("English", TranslationStatus::Original), ("German", TranslationStatus::Translation), ("Spanish", TranslationStatus::Translation)]
    );
}
