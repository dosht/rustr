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
    assert_eq!(s.update(Event::Restored { drafts: vec![d.clone()], preferred: vec![], preferred_targets: vec![] }), vec![Effect::ReloadDrafts]);
    assert_eq!(s.update(Event::DraftsReloaded { drafts: vec![d.clone()], elapsed_ms: 0 }), vec![Effect::ScheduleReload { after_ms: 1000 }]);
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
