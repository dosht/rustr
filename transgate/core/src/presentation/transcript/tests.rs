use super::*;
use crate::presentation::fixtures::{self, line};

fn loaded(job: Job, lines: Vec<Line>) -> TranscriptState {
    let mut s = TranscriptState::default();
    s.update(Event::Loaded { job, lines, show_original: false, is_freemium: false });
    s
}

// JOB-F09: running jobs poll; completion loads lines after two seconds.
#[test]
fn running_polls_then_loads_lines() {
    let mut s = TranscriptState::default();
    let effects = s.update(Event::Loaded { job: fixtures::job(1, JobStatus::Running, "a.mp3"), lines: vec![], show_original: false, is_freemium: false });
    assert_eq!(
        effects,
        vec![Effect::LoadMediaUrl { job: JobId(1), file_name: "a.mp3".into() }, Effect::PollJob { after_ms: RUNNING_POLL_MS }]
    );
    let effects = s.update(Event::JobPolled { job: fixtures::job(1, JobStatus::Completed, "a.mp3") });
    assert_eq!(effects, vec![Effect::LoadLines { after_ms: COMPLETION_DELAY_MS }]);
    assert!(s.view().download_enabled);
}

// UPL-016: the signed URL is requested once per shown job, reused 14 minutes, then renewed;
// a failure shows "Audio is not available." for five seconds. A late answer for another job is dropped.
#[test]
fn the_player_url_is_loaded_renewed_and_its_failure_shown_briefly() {
    let mut s = TranscriptState::default();
    s.update(Event::Loaded { job: fixtures::job(1, JobStatus::Completed, "a.mp3"), lines: vec![], show_original: false, is_freemium: false });
    assert!(s.update(Event::Loaded { job: fixtures::job(1, JobStatus::Completed, "a.mp3"), lines: vec![], show_original: false, is_freemium: false }).is_empty());
    assert_eq!(s.view().media, Media::Audio { url: None, loading: true, unavailable: false });
    assert!(s.update(Event::MediaUrlLoaded { job: JobId(2), result: Ok("https://other".into()) }).is_empty());
    let effects = s.update(Event::MediaUrlLoaded { job: JobId(1), result: Ok("https://signed/1".into()) });
    assert_eq!(effects, vec![Effect::ScheduleMediaUrl { job: JobId(1), after_ms: MEDIA_URL_REUSE_MS }]);
    let vm = s.view();
    assert_eq!(vm.media, Media::Audio { url: Some("https://signed/1".into()), loading: false, unavailable: false });
    assert_eq!(vm.rows.len(), 0);
    assert_eq!(s.update(Event::MediaUrlDue { job: JobId(1) }), vec![Effect::LoadMediaUrl { job: JobId(1), file_name: "a.mp3".into() }]);
    let effects = s.update(Event::MediaUrlLoaded { job: JobId(1), result: Err(crate::presentation::failure::ApiFailure::http(404, None)) });
    assert_eq!(effects, vec![Effect::ScheduleNoAudioAlert { after_ms: NO_AUDIO_ALERT_MS }]);
    assert_eq!(s.view().no_audio_alert, Some(NO_AUDIO));
    s.update(Event::NoAudioAlertExpired);
    assert_eq!(s.view().no_audio_alert, None);
    assert!(s.update(Event::MediaUrlDue { job: JobId(1) }).is_empty(), "no renewal without a URL");
}

// Transcript.tsx: a YouTube job plays its video in the iframe player; no URL is requested.
#[test]
fn a_youtube_job_plays_its_video_and_rows_carry_seconds() {
    let mut j = fixtures::job(3, JobStatus::Completed, "Talk");
    j.source = crate::jobs::domain::JobSource::YouTube { video_id: Some("abc".into()), youtube_url: None, transcript_source: None };
    let mut s = TranscriptState::default();
    let effects = s.update(Event::Loaded { job: j, lines: vec![fixtures::line(1, 3, "00:01:02", "00:01:04", "hi", None)], show_original: false, is_freemium: false });
    assert!(effects.is_empty());
    let vm = s.view();
    assert_eq!(vm.media, Media::YouTube { video_id: "abc".into() });
    assert_eq!((vm.rows[0].start_seconds, vm.rows[0].end_seconds), (62.0, 64.0));
}

// EDT-001/002: pause while editing, write only the text, resume if it was playing.
#[test]
fn edit_flow() {
    let mut s = loaded(fixtures::job(1, JobStatus::Completed, "a.mp3"), vec![line(4, 1, "00:00:01", "00:00:02", "old", None)]);
    assert_eq!(s.update(Event::EditStarted { line: LineId(4) }), vec![Effect::PausePlayer]);
    s.update(Event::PlayerState { playing: true });
    assert!(s.view().rows[0].editing);
    let effects = s.update(Event::EditSubmitted { text: String::new() });
    assert_eq!(effects, vec![Effect::SaveEdit { edit: LineEdit::Text { line: LineId(4), text: String::new() } }, Effect::ResumePlayer]);
    assert_eq!(s.view().rows[0].text, "");
}

// EDT-006: renaming a speaker changes every matching line of the job.
#[test]
fn rename_speaker() {
    let lines = vec![line(1, 1, "00:00:00", "x", "a", Some("SPEAKER_00")), line(2, 1, "00:00:01", "x", "b", Some("SPEAKER_01"))];
    let mut s = loaded(fixtures::job(1, JobStatus::Completed, "a.mp3"), lines);
    let effects = s.update(Event::SpeakerRenamed { from: "SPEAKER_00".into(), to: "Ann".into() });
    assert_eq!(effects, vec![Effect::SaveEdit { edit: LineEdit::RenameSpeaker { job: JobId(1), from: "SPEAKER_00".into(), to: "Ann".into() } }]);
    assert_eq!(s.view().rows[0].speaker, Some(SpeakerLabel::Named { name: "Ann".into() }));
}

// EDT-008/009: labels hidden while in progress; null speakers show the previous one.
#[test]
fn speaker_display() {
    let lines = vec![line(1, 1, "00:00:00", "x", "a", Some("SPEAKER_00")), line(2, 1, "00:00:01", "x", "b", None)];
    let s = loaded(fixtures::job(1, JobStatus::Completed, "a.mp3"), lines.clone());
    let rows = s.view().rows;
    assert_eq!(rows[1].speaker, Some(SpeakerLabel::Numbered { number: 1 }));
    let running = loaded(fixtures::job(1, JobStatus::Running, "a.mp3"), lines);
    assert_eq!(running.view().rows[0].speaker, None);
}

// EDT-012, EXP-013, LNG-007: translation tab by default, translation-only download, RTL by tab.
#[test]
fn translation_page() {
    let vm = fixtures::transcript_translation();
    assert_eq!(vm.tabs.len(), 1);
    assert!(!vm.time_toggle);
    let mut j = fixtures::job(13, JobStatus::Completed, "talk.mp3");
    j.translate_to = Language::from_name("Arabic").into_iter().collect();
    let mut original = line(1, 13, "00:00:00", "00:00:03", "Hello", None);
    original.language = Language::from_name("English");
    original.translation_status = Some(TranslationStatus::Original);
    let mut translated = line(2, 13, "00:00:00", "00:00:03", "مرحبا", None);
    translated.language = Language::from_name("Arabic");
    translated.translation_status = Some(TranslationStatus::Translation);
    let mut s = loaded(j, vec![original, translated]);
    let vm = s.view();
    assert!(vm.rtl && vm.show_tabs && vm.is_translation);
    assert_eq!((vm.tabs[0].name.as_str(), vm.lines_tab_label.clone()), ("Arabic", crate::presentation::msg::app("Translation")));
    // EXP-012/013: the toggles go to the server export, whose page surface keeps only translation lines.
    let request = ExportRequest {
        kind: ExportKind::Wysiwyg { show_timestamps: Some(false), show_speakers: Some(true) },
        surface: ExportSurface::TranscriptPage,
        labels: crate::export::ExportLabels::default(),
    };
    assert_eq!(s.update(Event::DownloadPressed { all_formats: false }), vec![Effect::Export { job: JobId(13), request }]);
}

// EDT-008, EDT-010: the speaker display matrix. Labels are hidden while the job has not
// finished transcribing; speaker_labeling shows a pulsing placeholder for unlabeled lines;
// a YouTube caption transcript and a job without speakers offer no speaker handling.
#[test]
fn edt_008_010_speaker_display_matrix() {
    let lines = vec![line(1, 1, "00:00:00", "00:00:02", "a", Some("SPEAKER_00")), line(2, 1, "00:00:02", "00:00:04", "b", None)];
    for status in [JobStatus::NotStarted, JobStatus::Pending, JobStatus::Running] {
        let vm = loaded(fixtures::job(1, status, "a.mp3"), lines.clone()).view();
        assert!(vm.rows.iter().all(|r| r.speaker.is_none() && !r.speaker_pending), "{status:?}");
    }
    let labeling = loaded(fixtures::job(1, JobStatus::SpeakerLabeling, "a.mp3"), lines.clone()).view();
    assert_eq!(
        labeling.rows.iter().map(|r| r.speaker_pending).collect::<Vec<_>>(),
        vec![false, true],
        "only the unlabeled line pulses"
    );
    let mut youtube = fixtures::job(1, JobStatus::Completed, "Talk");
    youtube.speakers = true;
    youtube.source = crate::jobs::domain::JobSource::YouTube { video_id: Some("abc".into()), youtube_url: None, transcript_source: Some("transcriptapi".into()) };
    let vm = loaded(youtube, lines.clone()).view();
    assert!(!vm.speaker_toggle && vm.rows.iter().all(|r| r.speaker.is_none()), "captions have no speakers");
    let mut silent = fixtures::job(1, JobStatus::Completed, "a.mp3");
    silent.speakers = false;
    let vm = loaded(silent, lines).view();
    assert!(!vm.speaker_toggle && !vm.phone_speaker_toggle && vm.time_toggle, "no speakers: only the time toggle");
}

// EDT-012, EDT-013, LNG-007: while a translation job streams, direction follows the job
// language; once it completes and its lines load, the selected (translation) tab decides.
#[test]
fn edt_013_lng_007_direction_switches_when_a_streaming_job_completes() {
    let mut j = fixtures::job(13, JobStatus::Running, "talk.mp3");
    j.language = Language::from_name("Arabic");
    j.translate_to = Language::from_name("English").into_iter().collect();
    let mut s = loaded(j.clone(), vec![]);
    assert!(s.view().rtl, "streaming: the Arabic job language");
    j.status = JobStatus::Completed;
    assert_eq!(s.update(Event::JobPolled { job: j }), vec![Effect::LoadLines { after_ms: COMPLETION_DELAY_MS }]);
    let mut original = line(1, 13, "00:00:00", "00:00:03", "مرحبا", None);
    original.language = Language::from_name("Arabic");
    original.translation_status = Some(TranslationStatus::Original);
    let mut translated = line(2, 13, "00:00:00", "00:00:03", "Hello", None);
    translated.language = Language::from_name("English");
    translated.translation_status = Some(TranslationStatus::Translation);
    s.update(Event::LinesArrived { lines: vec![original, translated] });
    let vm = s.view();
    assert_eq!(vm.tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["English"], "originals hidden without the flag");
    assert!(!vm.rtl, "completed: the selected English translation tab");
    assert_eq!(vm.rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), vec!["Hello"]);
}

// `transcript-header/views/statusConfig.tsx`: the header badge's hue and icon by status, as
// semantic tones (no shell maps a status to a colour): grey dot not started, yellow pending,
// sky running, purple speaker labeling, green check completed, red cross failed.
#[test]
fn the_header_badge_carries_its_tone_and_icon() {
    let cases = [
        (JobStatus::NotStarted, Tone::Neutral, Some(BadgeIcon::Dot), false),
        (JobStatus::Pending, Tone::Accent, None, true),
        (JobStatus::Running, Tone::Progress, None, true),
        (JobStatus::SpeakerLabeling, Tone::Accent, None, true),
        (JobStatus::Completed, Tone::Success, Some(BadgeIcon::Check), false),
        (JobStatus::Failed, Tone::Danger, Some(BadgeIcon::Cross), false),
    ];
    for (status, tone, icon, spinning) in cases {
        let badge = loaded(fixtures::job(1, status, "a.mp3"), vec![]).view().status_badge.unwrap();
        assert_eq!((badge.tone, badge.icon, badge.spinning), (tone, icon, spinning), "{status:?}");
    }
}

// EXP-001: no download before completion.
#[test]
fn download_requires_completion() {
    let mut s = loaded(fixtures::job(1, JobStatus::Running, "a.mp3"), vec![]);
    assert!(s.update(Event::DownloadPressed { all_formats: true }).is_empty());
    assert!(!s.view().download_enabled);
}

// FRE-020: a freemium viewer of a long job sees the cut and the banner; downloads stay allowed (FRE-022).
#[test]
fn freemium_cut() {
    let mut j = fixtures::job(1, JobStatus::Completed, "a.mp3");
    j.duration_minutes = Some(40);
    let lines: Vec<Line> = (0..40).map(|m| line(m + 1, 1, &format!("00:{m:02}:00"), "x", "t", None)).collect();
    let mut s = TranscriptState::default();
    s.update(Event::Loaded { job: j, lines, show_original: false, is_freemium: true });
    let vm = s.view();
    assert!(vm.rows.len() < 40);
    assert!(vm.upgrade_banner);
    assert_eq!(vm.freemium, Some(FreemiumBanner { current_minutes: 40, max_minutes: 20, percent: 100 }));
    assert!(vm.download_enabled);
    assert!(matches!(s.update(Event::DownloadPressed { all_formats: true }).as_slice(), [Effect::Export { request: ExportRequest { kind: ExportKind::AllFormats, .. }, .. }]));
}

// TranscriptLinesView / TranscriptLineView / SpeakerLabel: empty, failure, time text and speaker options.
#[test]
fn lines_view_states_follow_the_source() {
    let mut s = loaded(fixtures::job(1, JobStatus::Completed, "a.mp3"), vec![]);
    assert_eq!(s.view().empty, Some(NO_LINES));
    s.update(Event::LinesFailed { detail: "timeout".into() });
    let vm = s.view();
    assert_eq!((vm.load_error, vm.load_error_detail.as_deref()), (Some(LOAD_ERROR), Some("timeout")));
    s.update(Event::LinesArrived { lines: vec![line(1, 1, "00:00:01,500", "00:00:03,000", "hi", Some("SPEAKER_00"))] });
    let vm = s.view();
    assert_eq!((vm.empty, vm.load_error, vm.can_copy), (None, None, true));
    assert_eq!(vm.rows[0].time_text, "00:00:01 - 00:00:03");
    assert_eq!(vm.rows[0].speaker_value.as_deref(), Some("SPEAKER_00"));
    assert_eq!(vm.speaker_options, vec![SpeakerOption { value: "SPEAKER_00".into(), label: SpeakerLabel::Numbered { number: 1 } }]);
    assert_eq!(vm.status_badge.map(|b| (b.label, b.spinning)), Some((common("completed"), false)));
    let running = loaded(fixtures::job(2, JobStatus::Running, "b.mp3"), vec![]).view();
    assert_eq!((running.empty, running.status_badge.map(|b| b.spinning)), (None, Some(true)), "still running: the skeleton, not the empty text");
}

// EDT-014: clicking a time seeks the player.
#[test]
fn time_click_seeks() {
    let mut s = loaded(fixtures::job(1, JobStatus::Completed, "a.mp3"), vec![line(1, 1, "01:00:05", "x", "a", None)]);
    assert_eq!(s.update(Event::TimeClicked { line: LineId(1) }), vec![Effect::Seek { seconds: 3605.0 }]);
}
