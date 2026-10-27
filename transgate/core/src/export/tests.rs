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

// EXP-008: no split at exactly 6 s, a split just over it, and `-->` never reaches a cue.
#[test]
fn exp_008_subtitle_split_at_exactly_six_seconds_and_arrow_escaping() {
    let at = |end: &str| subtitle_cues(&[line(1, 1, "00:00:00", end, "one two three four", None)]);
    assert_eq!(at("00:00:06").len(), 1, "6 s is not split");
    assert_eq!(at("00:00:05,999").len(), 1);
    let over = at("00:00:06,001");
    assert_eq!(over.len(), 2, "just over 6 s splits");
    assert_eq!(over.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["one two", "three four"]);
    assert_eq!((over[0].start.as_str(), over[1].end.as_str()), ("00:00:00,000", "00:00:06,001"));
    // Unsplit cues keep the stored HH:MM:SS, split ones switch to HH:MM:SS,mmm.
    assert_eq!((at("00:00:06")[0].start.as_str(), at("00:00:06")[0].end.as_str()), ("00:00:00", "00:00:06"));
    let half = at("00:00:06,500");
    assert_eq!((half[0].end.as_str(), half[1].start.as_str()), ("00:00:03,250", "00:00:03,250"));

    let cues = subtitle_cues(&[
        line(1, 1, "00:00:00", "00:00:02", " a --> b -->> c ", None),
        line(2, 1, "00:00:02", "00:00:09", "x --> y z --> w", None),
    ]);
    for out in [srt(&cues), vtt(&cues)] {
        assert!(out.contains("a -> b ->> c"), "{out}");
        assert!(out.contains("x -> y") && out.contains("z -> w"), "{out}");
        // The only `-->` left are the timing separators: one per cue.
        assert_eq!(out.matches("-->").count(), cues.len(), "{out}");
    }
}

// EXP-007/009/010: every export extension, translated headers and file names, CSV quoting of newlines.
#[test]
fn exp_007_009_010_translated_headers_and_newline_csv_in_every_extension() {
    let labels = ExportLabels {
        full_text: "Tam Metin".into(),
        speaker: "Konuşmacı".into(),
        speakers: "Konuşmacılar".into(),
        timestamps: "Zaman Damgaları".into(),
        text: "Metin".into(),
        start_time: "Başlangıç".into(),
        end_time: "Bitiş".into(),
    };
    let lines = vec![
        line(1, 1, "00:00:00", "00:00:02", "iki\nsatır, \"tırnak\"", Some("SPEAKER_00")),
        line(2, 1, "00:00:02", "00:00:04", "düz", Some("Ayşe\nB")),
    ];
    let b = wysiwyg(Some("Konuşma.MP4"), true, &lines, true, true, &labels);
    // Each extension of the bundle is present, named with the translated words (EXP-010).
    assert_eq!(names(&b), ["Tam Metin.txt", "Konuşma.doc", "Konuşma.srt", "Konuşma.vtt", "Konuşmacılar Zaman Damgaları Metin.csv"]);
    let csv = text(&b, "Konuşmacılar Zaman Damgaları Metin.csv");
    assert_eq!(
        csv,
        "Konuşmacı,Başlangıç,Bitiş,Metin\n\
         Konuşmacı 1,00:00:00,00:00:02,\"iki\nsatır, \"\"tırnak\"\"\"\n\
         \"Ayşe\nB\",00:00:02,00:00:04,düz"
    );
    // EXP-009: the VTT starts with WEBVTT and an empty line; the cue text keeps its newline.
    let vtt_text = text(&b, "Konuşma.vtt");
    assert!(vtt_text.starts_with("WEBVTT\n\n00:00:00 --> 00:00:02\niki\nsatır, \"tırnak\"\n"), "{vtt_text}");
    assert!(text(&b, "Konuşma.srt").starts_with("1\n00:00:00 --> 00:00:02\niki\nsatır"));

    // EXP-007: SRT and VTT exist only for every listed video extension (any case).
    for ext in ["mp4", "MOV", "avi", "mkv", "webm", "m4v", "flv", "wmv", "ts", "3GP"] {
        let name = format!("c.{ext}");
        let with = wysiwyg(Some(&name), false, &lines, false, false, &labels);
        assert_eq!(names(&with), ["Tam Metin.txt", "c.doc", "c.srt", "c.vtt"], "{ext}");
    }
    for ext in ["mp3", "wav", "m4a", "ogg", "tsx", ""] {
        let name = format!("c.{ext}");
        let without = wysiwyg(Some(&name), false, &lines, false, false, &labels);
        assert_eq!(names(&without), ["Tam Metin.txt", "c.doc"], "{ext:?}");
    }
    // The CSV is only for jobs with speakers (EXP-010).
    assert!(!names(&wysiwyg(Some("c.mp4"), false, &lines, true, true, &labels)).iter().any(|n| n.ends_with(".csv")));
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
