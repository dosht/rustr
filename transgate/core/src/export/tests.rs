use super::*;
use crate::presentation::fixtures::line;

fn names(b: &ExportBundle) -> Vec<&str> {
    b.entries.iter().map(|e| e.name.as_str()).collect()
}

fn text<'a>(b: &'a ExportBundle, name: &str) -> &'a str {
    match &b.entries.iter().find(|e| e.name == name).expect(name).content {
        EntryContent::Text { text } => text,
        EntryContent::Word { .. } => panic!("{name} is a Word file"),
    }
}

// EXP-003
#[test]
fn file_names_are_sanitized() {
    assert_eq!(base_name(Some("My: <Talk>  final?.v2.mp4")), "My Talk final.v2");
    assert_eq!(base_name(None), "");
    assert!(is_video_source("clip.MOV"));
    assert!(!is_video_source("clip.mp3"));
}

// EXP-002/004/005/007/010/012: the WYSIWYG bundle of a video job with speakers.
#[test]
fn wysiwyg_bundle_for_video_with_speakers() {
    let lines = vec![
        line(1, 1, "00:00:00", "00:00:02", " hello ", Some("SPEAKER_00")),
        line(2, 1, "00:00:02", "00:00:04", "He said \"hi\", ok", Some("Alice")),
    ];
    let labels = ExportLabels::default();
    let b = wysiwyg(Some("My: Talk.mp4"), true, &lines, true, true, &labels);
    assert_eq!((b.zip_name.as_str(), b.folder.as_str()), ("My Talk_transcript.zip", "My Talk_transcript"));
    assert_eq!(names(&b), ["Full Text.txt", "My Talk.doc", "My Talk.srt", "My Talk.vtt", "Speakers Timestamps Text.csv"]);
    assert_eq!(text(&b, "Full Text.txt"), "hello He said \"hi\", ok");
    assert_eq!(
        text(&b, "Speakers Timestamps Text.csv"),
        "Speaker,Start Time,End Time,Text\nSpeaker 1,00:00:00,00:00:02,hello\nAlice,00:00:02,00:00:04,\"He said \"\"hi\"\", ok\""
    );
    assert_eq!(text(&b, "My Talk.vtt"), "WEBVTT\n\n00:00:00 --> 00:00:02\nhello\n\n00:00:02 --> 00:00:04\nHe said \"hi\", ok\n");
    match &b.entries[1].content {
        EntryContent::Word { document } => assert_eq!(document.paragraphs[0].runs[0].text, "[Speaker 1]"),
        EntryContent::Text { .. } => panic!("doc must be a Word document"),
    }
}

// EXP-011/012: all formats forces subtitles; time off gives the plain Word file.
#[test]
fn all_formats_and_plain_word() {
    let lines = vec![line(1, 1, "00:00:00", "00:00:02", "hi", None)];
    let labels = ExportLabels::default();
    let all = all_formats(Some("a.mp3"), false, &lines, &labels);
    assert_eq!(all.zip_name, "a_all_formats_transcript.zip");
    assert_eq!(all.folder, "a_transcript");
    assert_eq!(names(&all), ["Full Text.txt", "a.doc", "a.srt", "a.vtt"]);
    let plain = wysiwyg(Some("a.mp3"), false, &lines, false, false, &labels);
    assert_eq!(names(&plain), ["Full Text.txt", "a.doc"]);
    match &plain.entries[1].content {
        EntryContent::Word { document } => assert_eq!(document.paragraphs[0].runs.len(), 1),
        EntryContent::Text { .. } => panic!("doc must be a Word document"),
    }
}

// EXP-008: lines over six seconds split into ~4.5 s pieces with millisecond times.
#[test]
fn long_lines_split_for_subtitles() {
    let lines = vec![line(1, 1, "00:00:00", "00:00:09", "one two three four", None)];
    let cues = subtitle_cues(&lines);
    assert_eq!(
        srt(&cues),
        "1\n00:00:00,000 --> 00:00:04,500\none two\n\n2\n00:00:04,500 --> 00:00:09,000\nthree four\n"
    );
    let cjk = subtitle_cues(&[line(1, 1, "00:00:00", "00:00:07", "你好世界", None)]);
    assert_eq!(cjk.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["你好", "世界"]);
}

// EXP-006: a new block on speaker change or 30 seconds after the block start.
#[test]
fn speaker_blocks_group_by_speaker_and_time() {
    let lines = vec![
        line(1, 1, "00:00:00", "x", "a", Some("SPEAKER_00")),
        line(2, 1, "00:00:10", "x", "b", Some("SPEAKER_00")),
        line(3, 1, "00:00:31", "x", "c", Some("SPEAKER_00")),
        line(4, 1, "00:00:35", "x", "d", Some("SPEAKER_01")),
    ];
    let blocks = speaker_blocks(&lines, &ExportLabels::default());
    let expected = [("Speaker 1", "a b"), ("Speaker 1", "c"), ("Speaker 2", "d")];
    assert_eq!(blocks.iter().map(|(s, t)| (s.as_str(), t.as_str())).collect::<Vec<_>>(), expected);
}

// EXP-014
#[test]
fn clipboard_lines() {
    let lines = vec![
        line(1, 1, "00:00:00,500", "00:00:02", "hello ", Some("SPEAKER_00")),
        line(2, 1, "00:00:02", "", "bye", None),
    ];
    assert_eq!(clipboard_text(&lines, &ExportLabels::default()), "[00:00:00 - 00:00:02] Speaker 1: hello\n[00:00:02] bye");
}

// EXP-016
#[test]
fn download_event_properties() {
    let lines = vec![line(1, 1, "a", "b", " two  words ", None), line(2, 1, "a", "b", "", None)];
    let e = download_event(7, true, Some("v.webm"), &lines, true, false);
    assert_eq!((e.job_id.as_str(), e.download_format.as_str(), e.word_count, e.media_type.as_str()), ("7", "all", 2, "video"));
}
