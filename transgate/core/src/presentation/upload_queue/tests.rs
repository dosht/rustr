use super::*;
use crate::jobs::domain::JobStatus;
use crate::presentation::fixtures;

fn lang(name: &str) -> Language {
    Language::from_name(name).unwrap()
}

/// The handle is the shell's: here 100 plus a running number.
fn picked(name: &str) -> PickedFile {
    file(name, "audio/mpeg", 10)
}

fn file(name: &str, mime: &str, size: i64) -> PickedFile {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(100);
    PickedFile { handle: NEXT.fetch_add(1, Ordering::Relaxed), name: name.into(), size, mime: mime.into() }
}

fn desktop(files: Vec<PickedFile>) -> Event {
    Event::Picked { files, frame: Frame::Desktop }
}

fn draft(id: i64, name: &str) -> Job {
    let mut j = fixtures::job(id, JobStatus::Draft, name);
    j.speakers = false;
    j.language = None;
    j
}

// JOB-D07: two files with the same name stay distinct rows.
#[test]
fn same_names_get_distinct_ids() {
    let mut s = UploadQueueState::default();
    let (a, b) = (picked("a.mp3"), picked("a.mp3"));
    let effects = s.update(desktop(vec![a.clone(), b.clone()]));
    assert_eq!(effects, vec![Effect::MeasureDuration { file: 1, handle: a.handle }, Effect::MeasureDuration { file: 2, handle: b.handle }]);
    s.update(Event::Removed { file: 1 });
    assert_eq!(s.files.len(), 1);
    assert_eq!(s.files[0].id, 2);
}

// UPL-003: drops are filtered twice and ignored at ten files; the picker is not capped.
#[test]
fn drop_filters_and_cap() {
    assert!(accept_dropped("audio/ogg", "x.ogg"));
    assert!(accept_dropped("", "x.m4a"));
    assert!(!accept_dropped("", "x.M4A"));
    assert!(!accept_dropped("", "x.mp3"));
    assert!(!accept_dropped("text/plain", "x.txt"));
    let mut s = UploadQueueState::default();
    s.update(desktop((0..12).map(|i| picked(&format!("{i}.mp3"))).collect()));
    assert_eq!(s.files.len(), 12);
    assert!(s.update(Event::Dropped { files: vec![picked("x.mp3")] }).is_empty());
}

// UPL-001/006, UPL-013/014: measure, create the draft with current options, upload; only ok counts.
#[test]
fn file_flow_to_ready() {
    let mut s = UploadQueueState::default();
    let a = picked("a.mp3");
    s.update(desktop(vec![a.clone()]));
    s.update(Event::SpeakersToggled { enabled: true });
    let effects = s.update(Event::Measured { file: 1, seconds: Some(61.0) });
    assert_eq!(
        effects,
        vec![Effect::CreateDraft { file: 1, name: "a.mp3".into(), size: 10, language: None, speakers: true, duration: Some(2) }]
    );
    let effects = s.update(Event::DraftCreated { file: 1, job: draft(5, "a.mp3") });
    // The toggle changed while the draft was created: written again, then upload.
    assert_eq!(
        effects,
        vec![
            Effect::SetOptions { jobs: vec![JobId(5)], language: None, translate_to: None, speakers: Some(true) },
            Effect::Upload { file: 1, handle: a.handle, job: JobId(5), name: "a.mp3".into() },
        ]
    );
    assert!(s.view(Frame::Desktop).start_disabled);
    s.update(Event::UploadFinished { file: 1, ok: false });
    assert_eq!(s.view(Frame::Desktop).rows[0].chip, Chip::UploadFailed);
    s.update(Event::UploadFinished { file: 1, ok: true });
    assert_eq!(s.view(Frame::Desktop).rows[0].chip, Chip::Ready);
    assert!(!s.view(Frame::Desktop).start_disabled);
}

// UPL-007/017: no language choice means the alert, not a start.
#[test]
fn start_needs_a_language_choice() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![draft(5, "a.mp3")], preferred: vec![], preferred_targets: vec![] });
    assert!(s.update(Event::StartPressed).is_empty());
    assert_eq!(s.alerts, vec![Alert::Language]);
    s.update(Event::LanguageChosen { choice: Some(Choice::AutoDetect) });
    assert_eq!(work(s.update(Event::StartPressed)), vec![Effect::Start { jobs: vec![JobId(5)], language: None, translate_to: vec![] }]);
    assert!(s.view(Frame::Desktop).start_disabled);
}

// UPL-018/019: translation needs both sides; the same language clears the other side.
#[test]
fn translation_languages() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![draft(5, "a.mp3")], preferred: vec![], preferred_targets: vec![] });
    s.update(Event::TabChanged { mode: Mode::Translation, frame: Frame::Desktop });
    s.update(Event::SourceLanguageChosen { language: Some(lang("English")) });
    let effects = s.update(Event::TargetLanguageChosen { language: Some(lang("English")) });
    assert_eq!(s.source_language, None);
    assert_eq!(effects[0], Effect::SetOptions { jobs: vec![JobId(5)], language: Some(None), translate_to: None, speakers: None });
    assert!(s.update(Event::StartPressed).is_empty());
    s.update(Event::SourceLanguageChosen { language: Some(lang("Turkish")) });
    assert_eq!(
        work(s.update(Event::StartPressed)),
        vec![Effect::Start { jobs: vec![JobId(5)], language: Some(lang("Turkish")), translate_to: vec![lang("English")] }]
    );
    assert!(!s.view(Frame::Desktop).auto_detect_offered);
}

// UPL-008: 1 s ten times, then 2 s growing by 1.5 up to 30 s; timeout after 5 minutes.
#[test]
fn duration_polling() {
    assert_eq!(poll_interval_ms(0), 1000);
    assert_eq!(poll_interval_ms(10), 2000);
    assert_eq!(poll_interval_ms(11), 3000);
    assert_eq!(poll_interval_ms(30), 30_000);
    let mut s = UploadQueueState::default();
    let mut d = draft(5, "a.mp3");
    d.duration_minutes = None;
    assert_eq!(s.update(Event::Restored { drafts: vec![d.clone()], preferred: vec![], preferred_targets: vec![] }), vec![Effect::ReloadDrafts { batch: 0 }]);
    assert_eq!(s.update(Event::DraftsReloaded { drafts: vec![d.clone()], elapsed_ms: 0 }), vec![Effect::ScheduleReload { batch: 0, after_ms: 1000 }]);
    assert!(s.update(Event::DraftsReloaded { drafts: vec![d.clone()], elapsed_ms: POLL_TIMEOUT_MS }).is_empty());
    assert_eq!(s.alerts, vec![Alert::ProcessingTimeout]);
    d.duration_minutes = Some(3);
    s.update(Event::DraftsReloaded { drafts: vec![d], elapsed_ms: POLL_TIMEOUT_MS + 1 });
    assert_eq!(s.view(Frame::Desktop).rows[0].chip, Chip::Ready);
}

// UPL-025/027/028: YouTube uses ceil minutes, the detected language, resolve only, and removal on failure.
#[test]
fn youtube_queue() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![], preferred: vec![], preferred_targets: vec![] });
    let mut yt = draft(9, "Video");
    yt.source = crate::jobs::domain::JobSource::YouTube { video_id: Some("x".into()), youtube_url: None, transcript_source: None };
    s.update(Event::YouTubeAdded { job: yt, title: "Talk".into(), duration_seconds: 61.0, detected_language: Some("tr".into()) });
    assert_eq!(s.files[0].duration, Some(2));
    assert_eq!(s.language, Some(Choice::Language(lang("Turkish"))));
    assert_eq!(work(s.update(Event::StartPressed)), vec![Effect::ResolveYouTube { job: JobId(9) }]);
    let effects = s.update(Event::YouTubeResolved { job: JobId(9), completed: false, code: Some("rate_limited".into()) });
    assert_eq!(effects, vec![Effect::DeleteDraft { job: JobId(9) }]);
    assert_eq!(s.youtube_error, Some(app("Too many requests, please try again in a few minutes")));
    assert!(s.files.is_empty());
}

/// The effects that do work, without product analytics.
fn work(effects: Vec<Effect>) -> Vec<Effect> {
    effects.into_iter().filter(|e| !matches!(e, Effect::Telemetry { .. })).collect()
}

// MobileTranscriptionContainer.tsx:120-157: the phone picker keeps audio/video MIME or six
// extensions in any case, up to 500 MiB inclusive; the desktop picker keeps everything.
#[test]
fn the_phone_picker_drops_unsupported_and_oversized_files_silently() {
    let files = vec![
        file("a.MP3", "", 10),
        file("b.webm", "", 10),
        file("c.bin", "video/quicktime", PHONE_MAX_BYTES),
        file("d.txt", "text/plain", 10),
        file("e.avi", "", 10),
        file("f.wav", "audio/wav", PHONE_MAX_BYTES + 1),
    ];
    let mut phone = UploadQueueState::default();
    phone.update(Event::Picked { files: files.clone(), frame: Frame::Phone });
    let names: Vec<&str> = phone.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a.MP3", "b.webm", "c.bin"]);
    let mut desk = UploadQueueState::default();
    desk.update(desktop(files));
    assert_eq!(desk.files.len(), 6);
}

// MobileNewTranscript.tsx:77-88: "Calculating..." while any duration is unknown; Start needs every file ready.
#[test]
fn the_total_is_unknown_until_every_file_is_measured() {
    let mut s = UploadQueueState::default();
    assert_eq!(s.view(Frame::Desktop).total_minutes, Some(0));
    s.update(desktop(vec![picked("a.mp3"), picked("b.mp3")]));
    s.update(Event::Measured { file: 1, seconds: Some(120.0) });
    let vm = s.view(Frame::Desktop);
    assert_eq!((vm.total_minutes, vm.files_ready), (None, false));
    s.update(Event::Measured { file: 2, seconds: Some(1.0) });
    assert_eq!(s.view(Frame::Desktop).total_minutes, Some(3));
}

// UPL-002: the picker's accept list is the product's, chosen by the layout
// (desktop `TranscriptionContainer`, phone `FileUploadButton`).
#[test]
fn upl_002_the_picker_accept_list_follows_the_layout() {
    let s = UploadQueueState::default();
    assert_eq!(s.view(Frame::Desktop).picker_accept, "audio/*,video/*,.m4a");
    assert_eq!(s.view(Frame::Phone).picker_accept, "audio/mpeg,audio/mp4,audio/wav,audio/webm,.mp3,.m4a,.wav,.webm");
}

// JOB-003: every draft comes back as an uploaded file, even one whose upload had failed
// (a draft row records no upload outcome); the language is not restored; the speaker
// toggle is on only when every restored draft has speakers.
#[test]
fn job_003_restored_drafts_are_uploaded_without_language_and_aggregate_speakers() {
    let mut failed_upload = draft(5, "broken.mp3");
    failed_upload.language = Some(lang("Turkish"));
    failed_upload.speakers = true;
    let mut other = draft(6, "b.wav");
    other.speakers = true;
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![failed_upload.clone(), other.clone()], preferred: vec![], preferred_targets: vec![] });
    let vm = s.view(Frame::Desktop);
    assert!(vm.rows.iter().all(|r| r.chip == Chip::Ready), "restored as uploaded");
    assert_eq!((vm.language, vm.speakers), (None, true));

    other.speakers = false;
    let mut mixed = UploadQueueState::default();
    mixed.update(Event::Restored { drafts: vec![failed_upload, other], preferred: vec![], preferred_targets: vec![] });
    assert!(!mixed.view(Frame::Desktop).speakers, "on only when all drafts have speakers");
}

// UPL-015: removing a file hard deletes its draft and the entry; a later draft reload
// that still lists it never brings it back.
#[test]
fn upl_015_a_removed_draft_leaves_the_queue_for_good() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![draft(5, "a.mp3"), draft(6, "b.mp3")], preferred: vec![], preferred_targets: vec![] });
    assert_eq!(s.update(Event::Removed { file: 1 }), vec![Effect::DeleteDraft { job: JobId(5) }]);
    s.update(Event::DraftsReloaded { drafts: vec![draft(5, "a.mp3"), draft(6, "b.mp3")], elapsed_ms: 0 });
    let jobs: Vec<_> = s.view(Frame::Desktop).rows.iter().map(|r| r.id).collect();
    assert_eq!(jobs, vec![2]);
}

// UPL-020, LNG-024: the speaker toggle writes every queued draft, is offered and sent for
// every language, and new drafts are created with it.
#[test]
fn upl_020_lng_024_the_speaker_toggle_applies_to_every_draft_in_every_language() {
    for language in ["Turkish", "Indonesian", "English", "Arabic"] {
        let mut s = UploadQueueState::default();
        s.update(Event::Restored { drafts: vec![draft(5, "a.mp3"), draft(6, "b.mp3")], preferred: vec![], preferred_targets: vec![] });
        s.update(Event::LanguageChosen { choice: Some(Choice::Language(lang(language))) });
        assert_eq!(
            s.update(Event::SpeakersToggled { enabled: true }),
            vec![Effect::SetOptions { jobs: vec![JobId(5), JobId(6)], language: None, translate_to: None, speakers: Some(true) }],
            "{language}"
        );
        assert!(s.view(Frame::Desktop).speakers, "{language}: the toggle persists");
        let a = picked("c.mp3");
        s.update(desktop(vec![a]));
        let created = s.update(Event::Measured { file: 3, seconds: Some(60.0) });
        assert!(created.iter().any(|e| matches!(e, Effect::CreateDraft { speakers: true, .. })), "{language}: {created:?}");
    }
}

// UPL-027: every caption failure code maps to its source message.
#[test]
fn upl_027_youtube_failure_codes_show_their_messages() {
    let cases = [
        (Some("no_captions"), "This video has no captions available. As this feature is still in beta, you can download the video and upload it here to transcribe and analyze it as audio or video."),
        (Some("age_restricted"), "This video requires age verification and cannot be transcribed"),
        (Some("rate_limited"), "Too many requests, please try again in a few minutes"),
        // The displayed source text (`useTranscriptionViewModel.ts:943-947`); the ledger abbreviates it.
        (Some("network_error"), "Network error fetching captions — please try again"),
        (Some("unknown_error"), "Failed to transcribe this YouTube video. Please try again."),
        (Some("something_new"), "Failed to transcribe this YouTube video. Please try again."),
        (None, "Failed to transcribe this YouTube video. Please try again."),
    ];
    for (code, message) in cases {
        assert_eq!(youtube_error_message(code), app(message), "{code:?}");
    }
}

// UPL-028: a queue with a YouTube draft only resolves YouTube; its failure deletes that
// draft alone, and the files of the same queue then start normally.
#[test]
fn upl_028_a_failed_youtube_draft_is_removed_and_the_files_proceed() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![draft(5, "a.mp3")], preferred: vec![], preferred_targets: vec![] });
    let mut yt = draft(9, "Video");
    yt.source = crate::jobs::domain::JobSource::YouTube { video_id: Some("x".into()), youtube_url: None, transcript_source: None };
    s.update(Event::YouTubeAdded { job: yt, title: "Talk".into(), duration_seconds: 61.0, detected_language: Some("tr".into()) });
    assert_eq!(work(s.update(Event::StartPressed)), vec![Effect::ResolveYouTube { job: JobId(9) }]);
    assert_eq!(
        s.update(Event::YouTubeResolved { job: JobId(9), completed: false, code: Some("no_captions".into()) }),
        vec![Effect::DeleteDraft { job: JobId(9) }]
    );
    assert_eq!(s.view(Frame::Desktop).rows.len(), 1, "the file stays queued");
    assert_eq!(work(s.update(Event::StartPressed)), vec![Effect::Start { jobs: vec![JobId(5)], language: Some(lang("Turkish")), translate_to: vec![] }]);
}

// ANLZ-1-03: one started event per queued file on Start, one failed event per file on failure.
#[test]
fn start_reports_one_analytics_event_per_file() {
    let mut s = UploadQueueState::default();
    s.update(Event::Restored { drafts: vec![draft(5, "a.mp3"), draft(6, "b.wav")], preferred: vec![], preferred_targets: vec![] });
    s.update(Event::LanguageChosen { choice: Some(Choice::Language(lang("Turkish"))) });
    let names = |effects: &[Effect]| -> Vec<(String, Option<crate::presentation::telemetry::Prop>)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Telemetry { event } => Some((event.name.to_string(), event.properties.get("job_id").cloned())),
                _ => None,
            })
            .collect()
    };
    use crate::presentation::telemetry::Prop;
    let started = s.update(Event::StartPressed);
    assert_eq!(
        names(&started),
        vec![("transcription_started".to_string(), Some(Prop::Number(5.0))), ("transcription_started".to_string(), Some(Prop::Number(6.0)))]
    );
    let failed = s.update(Event::StartFailed { balance: true });
    assert_eq!(names(&failed).len(), 2);
    assert!(names(&failed).iter().all(|(n, _)| n == "transcription_failed"));
    assert_eq!(s.alerts, vec![Alert::Balance]);
}

#[test]
fn a_new_batch_preserves_server_drafts_and_ignores_old_poll_and_restore_answers() {
    let mut state = UploadQueueState::default();
    let mut old = draft(90, "old.mp3");
    old.duration_minutes = None;
    assert_eq!(state.update(Event::Restored { drafts: vec![old.clone()], preferred: vec![lang("English")], preferred_targets: vec![lang("Turkish")] }), vec![Effect::ReloadDrafts { batch: 0 }]);
    let old_file = state.files[0].id;
    assert!(state.update(Event::BeginBatch).is_empty());
    assert!(state.files.is_empty());
    assert_eq!(state.preferred, [lang("English")]);
    assert_eq!(state.preferred_targets, [lang("Turkish")]);
    assert!(state.update(Event::Restored { drafts: vec![old.clone()], preferred: vec![], preferred_targets: vec![] }).is_empty());
    assert!(state.files.is_empty());
    assert!(state.update(Event::LanguageChosen { choice: Some(Choice::Language(lang("Turkish"))) }).is_empty());
    assert!(state.update(Event::SpeakersToggled { enabled: true }).is_empty());
    state.update(desktop(vec![picked("new.mp3")]));
    let new_file = state.files[0].id;
    assert!(new_file > old_file);
    assert!(state.update(Event::Measured { file: old_file, seconds: Some(60.0) }).is_empty());
    state.update(Event::Measured { file: new_file, seconds: None });
    let mut new = draft(101, "new.mp3");
    new.duration_minutes = None;
    state.update(Event::DraftCreated { file: new_file, job: new.clone() });
    assert_eq!(state.update(Event::UploadFinished { file: new_file, ok: true }), vec![Effect::ReloadDrafts { batch: 1 }]);
    assert!(state.update(Event::PollReloaded { batch: 0, drafts: vec![old], elapsed_ms: POLL_TIMEOUT_MS }).is_empty());
    assert!(state.alerts.is_empty());
    assert!(state.polling);
    new.duration_minutes = Some(1);
    state.update(Event::PollReloaded { batch: 1, drafts: vec![new], elapsed_ms: 0 });
    assert!(state.view(Frame::Desktop).files_ready);
    let effects = state.update(Event::StartPressed);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Start { jobs, .. } if jobs == &[JobId(101)])));
    assert!(effects.iter().all(|effect| !matches!(effect, Effect::SetOptions { jobs, .. } if jobs.contains(&JobId(90)))));
}

#[test]
fn a_new_batch_cannot_discard_restoration_or_active_upload_work() {
    let mut state = UploadQueueState::default();
    state.update(Event::BeginBatch);
    assert!(state.initializing);
    state.update(Event::Restored { drafts: vec![], preferred: vec![], preferred_targets: vec![] });
    state.update(desktop(vec![picked("pending.mp3")]));
    let before = state.clone();
    assert!(state.update(Event::BeginBatch).is_empty());
    assert_eq!(state, before);
    state.files.clear();
    state.youtube_dialog.busy = true;
    let before = state.clone();
    assert!(state.update(Event::BeginBatch).is_empty());
    assert_eq!(state, before);
    state.youtube_dialog.busy = false;
    state.transcribing = true;
    let before = state.clone();
    assert!(state.update(Event::BeginBatch).is_empty());
    assert_eq!(state, before);
}
