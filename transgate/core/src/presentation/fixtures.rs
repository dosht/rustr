//! Named screen states for previews, Storybook and snapshot tests (P5/P8 render
//! them; stories never hand-write a view model). Every state comes from real
//! transitions of its state machine. Data is synthetic.
//!
//! [`export`] gathers every named state by view model type name; the `ts`
//! generator below writes it with the TypeScript declarations to
//! `transgate/web/src/lib/core/{fixtures.json,types/}`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::account::{FeedbackRating, SessionProfile};
use crate::flags::{Capabilities, FlagConfig, Subject};
use crate::ids::{AccountId, JobId, LineId, Micros};
use crate::jobs::domain::{DeferredCharge, Job, JobSource, JobStatus, StorageProvider};
use crate::languages::Language;
use crate::presentation::app::{
    AppConfig, AppEvent, AppIntent, AppState, AppVm, AuthEvent, AuthFailure, AuthIntent, AuthMode, AuthVm, BalanceView,
    NewPasswordIntent, NewPasswordVm, SessionFacts, SignedInVm, SignedOutVm,
};
use crate::presentation::data_management::{self, DataManagementState, DataManagementVm};
use crate::presentation::failure::ApiFailure;
use crate::presentation::feedback::{self, FeedbackState, FeedbackVm};
use crate::presentation::job_list::{self, JobListState, JobListVm};
use crate::presentation::layout::{Frame, Viewport};
use crate::presentation::settings::{self, SettingsState, SettingsVm};
use crate::presentation::support::{self, SupportState, SupportVm};
use crate::presentation::transcript::{self, TranscriptState, TranscriptVm};
use crate::presentation::upload_queue::{self, PickedFile, UploadQueueState, UploadQueueVm};
use crate::privacy::erasure::ErasureView;
use crate::privacy::settings::UserSettings;
use crate::transcript::{Line, TranslationStatus};

pub fn account() -> AccountId {
    AccountId(uuid::Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001))
}

pub fn job(id: i64, status: JobStatus, name: &str) -> Job {
    Job {
        id: JobId(id),
        account: account(),
        revision: 1,
        status,
        name: Some(name.into()),
        source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
        language: Language::from_name("English"),
        translate_to: vec![],
        speakers: true,
        fix_grammer: None,
        duration_minutes: Some(12),
        size_bytes: Some(4_200_000),
        provider: None,
        model: None,
        callback_url: None,
        whisper_params: None,
        error: if status == JobStatus::Failed { Some(json!({ "message": "No transcription segments found" })) } else { None },
        retry_count: 0,
        recovery_attempts: 0,
        soniox_transcription_id: None,
        started_at: None,
        created_at: Micros(1_790_000_000_000_000 + id * 1_000_000),
        updated_at: Micros(1_790_000_000_000_000 + id * 1_000_000),
        current_attempt: None,
        attempts_made: 0,
        deferred_charge: DeferredCharge::None,
        next_dispatch_at: None,
    }
}

pub fn line(id: i64, job: i64, start: &str, end: &str, text: &str, speaker: Option<&str>) -> Line {
    Line {
        id: LineId(id),
        job: JobId(job),
        account: account(),
        start_time: start.into(),
        end_time: end.into(),
        line: text.into(),
        speaker: speaker.map(str::to_string),
        corrected_text: None,
        split_on_speaker_change: Some(true),
        language: None,
        translation_status: None,
        created_at: Micros(0),
    }
}

/// The instant every fixture is rendered at (2026-01-01T00:00:00Z).
pub const NOW: Micros = Micros(1_767_225_600_000_000);

fn restored(drafts: Vec<Job>) -> UploadQueueState {
    let mut s = UploadQueueState::default();
    s.update(upload_queue::Event::Restored { drafts, preferred: Language::from_name("Turkish").into_iter().collect(), preferred_targets: vec![] });
    s
}

fn picked(handle: u64, name: &str) -> PickedFile {
    PickedFile { handle, name: name.into(), size: 4_200_000, mime: "audio/mpeg".into() }
}

pub fn queue_empty() -> UploadQueueVm {
    restored(vec![]).view(Frame::Desktop)
}

pub fn queue_uploading() -> UploadQueueVm {
    let mut s = restored(vec![]);
    s.update(upload_queue::Event::Picked { files: vec![picked(1, "lecture.mp3")], frame: Frame::Desktop });
    s.update(upload_queue::Event::UploadProgress { file: 1, progress: 42 });
    s.view(Frame::Desktop)
}

/// Uploaded but not yet measured: the phone "Calculating..." total and "Preparing files...".
pub fn queue_preparing() -> UploadQueueVm {
    let mut s = restored(vec![]);
    s.update(upload_queue::Event::Picked { files: vec![picked(1, "lecture.mp3"), picked(2, "notes.m4a")], frame: Frame::Phone });
    s.update(upload_queue::Event::Measured { file: 1, seconds: Some(125.0) });
    s.update(upload_queue::Event::UploadFinished { file: 1, ok: true });
    s.update(upload_queue::Event::UploadFinished { file: 2, ok: true });
    s.view(Frame::Phone)
}

pub fn queue_upload_failed() -> UploadQueueVm {
    let mut s = restored(vec![]);
    s.update(upload_queue::Event::Picked { files: vec![picked(1, "lecture.mp3")], frame: Frame::Desktop });
    s.update(upload_queue::Event::UploadFinished { file: 1, ok: false });
    s.view(Frame::Desktop)
}

pub fn queue_ready() -> UploadQueueVm {
    let mut s = restored(vec![job(7, JobStatus::Draft, "interview.m4a")]);
    s.update(upload_queue::Event::LanguageChosen { choice: Some(upload_queue::Choice::AutoDetect) });
    s.view(Frame::Desktop)
}

pub fn queue_translation() -> UploadQueueVm {
    let mut s = restored(vec![job(7, JobStatus::Draft, "interview.m4a")]);
    s.update(upload_queue::Event::TabChanged { mode: upload_queue::Mode::Translation, frame: Frame::Desktop });
    s.update(upload_queue::Event::SourceLanguageChosen { language: Language::from_name("English") });
    s.update(upload_queue::Event::TargetLanguageChosen { language: Language::from_name("German") });
    s.view(Frame::Desktop)
}

pub fn queue_starting() -> UploadQueueVm {
    let mut s = restored(vec![job(7, JobStatus::Draft, "interview.m4a")]);
    s.update(upload_queue::Event::LanguageChosen { choice: Some(upload_queue::Choice::AutoDetect) });
    s.update(upload_queue::Event::StartPressed);
    s.view(Frame::Desktop)
}

pub fn queue_language_alert() -> UploadQueueVm {
    let mut s = restored(vec![job(7, JobStatus::Draft, "interview.m4a")]);
    s.update(upload_queue::Event::StartPressed);
    s.view(Frame::Desktop)
}

pub fn queue_balance_alert() -> UploadQueueVm {
    let mut s = restored(vec![job(7, JobStatus::Draft, "interview.m4a")]);
    s.update(upload_queue::Event::LanguageChosen { choice: Some(upload_queue::Choice::AutoDetect) });
    s.update(upload_queue::Event::StartPressed);
    s.update(upload_queue::Event::StartFailed { balance: true });
    s.view(Frame::Desktop)
}

pub fn queue_processing_timeout() -> UploadQueueVm {
    let mut draft = job(7, JobStatus::Draft, "interview.m4a");
    draft.duration_minutes = None;
    let mut s = restored(vec![draft.clone()]);
    s.update(upload_queue::Event::DraftsReloaded { drafts: vec![draft], elapsed_ms: upload_queue::POLL_TIMEOUT_MS });
    s.view(Frame::Desktop)
}

/// UPL-003: ten queued files; drops are ignored from here on.
pub fn queue_max_files() -> UploadQueueVm {
    let mut s = restored(vec![]);
    let files = (1..=10).map(|n| picked(n, &format!("part-{n:02}.mp3"))).collect();
    s.update(upload_queue::Event::Picked { files, frame: Frame::Desktop });
    s.view(Frame::Desktop)
}

fn youtube_draft() -> Job {
    let mut j = job(9, JobStatus::Draft, "Talk");
    j.source = JobSource::YouTube { video_id: Some("dQw4w9WgXcQ".into()), youtube_url: None, transcript_source: None };
    j
}

pub fn queue_youtube() -> UploadQueueVm {
    let mut s = restored(vec![]);
    s.update(upload_queue::Event::YouTubeAdded { job: youtube_draft(), title: "Conference talk".into(), duration_seconds: 754.0, detected_language: Some("en".into()) });
    s.view(Frame::Desktop)
}

pub fn queue_youtube_error() -> UploadQueueVm {
    let mut s = restored(vec![]);
    s.update(upload_queue::Event::YouTubeAdded { job: youtube_draft(), title: "Conference talk".into(), duration_seconds: 754.0, detected_language: Some("en".into()) });
    s.update(upload_queue::Event::StartPressed);
    s.update(upload_queue::Event::YouTubeResolved { job: JobId(9), completed: false, code: Some("no_captions".into()) });
    s.view(Frame::Desktop)
}

fn history(jobs: Vec<Job>, next_cursor: Option<JobId>) -> JobListState {
    let mut s = JobListState::default();
    s.update(job_list::Event::Refresh);
    s.update(job_list::Event::PageLoaded { jobs, next_cursor, replace: true });
    s
}

fn history_jobs() -> Vec<Job> {
    let mut speaker = job(13, JobStatus::SpeakerLabeling, "panel.wav");
    speaker.language = Language::from_name("Turkish");
    vec![
        speaker,
        job(12, JobStatus::Running, "seminar.mp4"),
        job(11, JobStatus::Completed, "podcast.mp3"),
        job(10, JobStatus::Failed, "silence.wav"),
    ]
}

pub fn history_sample() -> JobListVm {
    history(history_jobs(), None).view(Some(JobId(11)), 0)
}

pub fn history_empty() -> JobListVm {
    history(vec![], None).view(None, 0)
}

pub fn history_loading() -> JobListVm {
    let mut s = JobListState::default();
    s.update(job_list::Event::Refresh);
    s.view(None, 0)
}

pub fn history_loading_more() -> JobListVm {
    let mut s = history(history_jobs(), Some(JobId(10)));
    s.update(job_list::Event::LoadMore);
    s.view(None, 0)
}

pub fn history_error() -> JobListVm {
    let mut s = JobListState::default();
    s.update(job_list::Event::Refresh);
    s.update(job_list::Event::LoadFailed { detail: Some("Network error".into()) });
    s.view(None, 0)
}

pub fn history_confirm_delete() -> JobListVm {
    let mut s = history(history_jobs(), None);
    s.update(job_list::Event::DeleteRequested { job: JobId(11) });
    s.view(None, 0)
}

pub fn history_downloading() -> JobListVm {
    let mut s = history(history_jobs(), None);
    s.update(job_list::Event::DownloadRequested { job: JobId(11) });
    s.view(None, 0)
}

fn loaded(job: Job, lines: Vec<Line>, is_freemium: bool) -> TranscriptState {
    let mut s = TranscriptState::default();
    let id = job.id;
    s.update(transcript::Event::Loaded { job, lines, show_original: false, is_freemium });
    s.update(transcript::Event::MediaUrlLoaded { job: id, result: Ok(format!("https://media.example.test/{}.mp3", id.0)) });
    s
}

fn podcast_lines() -> Vec<Line> {
    vec![
        line(1, 11, "00:00:00", "00:00:04", "Welcome to the show.", Some("SPEAKER_00")),
        line(2, 11, "00:00:04", "00:00:09", "Thanks for having me.", Some("SPEAKER_01")),
        line(3, 11, "00:00:09", "00:00:12", "Let us begin.", None),
    ]
}

pub fn transcript_running() -> TranscriptVm {
    loaded(job(12, JobStatus::Running, "seminar.mp4"), vec![], false).view()
}

pub fn transcript_speaker_labeling() -> TranscriptVm {
    let mut lines = podcast_lines();
    for l in &mut lines {
        l.job = JobId(13);
        l.speaker = None;
    }
    loaded(job(13, JobStatus::SpeakerLabeling, "panel.wav"), lines, false).view()
}

pub fn transcript_completed() -> TranscriptVm {
    loaded(job(11, JobStatus::Completed, "podcast.mp3"), podcast_lines(), false).view()
}

pub fn transcript_rtl() -> TranscriptVm {
    let mut j = job(16, JobStatus::Completed, "مناقشة خطة البحث.mp3");
    j.language = Language::from_name("Arabic");
    let mut lines = vec![
        line(1, 16, "00:00:00", "00:00:12", "مرحباً بكم في مناقشة خطة البحث. سنبدأ بعرض أهداف المشروع والخطوات القادمة.", Some("الدكتورة ليلى عبد الرحمن")),
        line(2, 16, "00:00:12", "00:00:24", "شكراً على الدعوة. نراجع اليوم نتائج الإصدار 2026.10 مع فريق Transgate.", Some("الأستاذ يوسف محمود")),
        line(3, 16, "00:00:24", "00:00:36", "هل يمكننا توضيح الجدول الزمني والإجابة عن أسئلة المشاركين قبل نهاية الاجتماع؟", Some("الدكتورة ليلى عبد الرحمن")),
    ];
    for row in &mut lines {
        row.language = j.language;
        row.translation_status = Some(TranslationStatus::Original);
    }
    loaded(j, lines, false).view()
}

pub fn transcript_long_strings() -> TranscriptVm {
    let name = format!("{}.mp3", "International research programme planning and accessibility review ".repeat(4));
    let speaker = "Professor Alexandra Montgomery-Worthington, International Research Programme Coordinator ".repeat(3);
    let unbroken_speaker = "InternationalResearchProgrammeAccessibilityCoordinator".repeat(4);
    let paragraph = "We are reviewing the complete research programme with participants from several departments, including the accessibility requirements, publication schedule, and responsibilities for every stage. ".repeat(12);
    let unbroken_word = "TranscriptionOverflowBoundary".repeat(12);
    let lines = vec![
        line(1, 17, "00:00:00", "00:01:30", &paragraph, Some(&speaker)),
        line(2, 17, "00:01:30", "00:02:00", &format!("The uninterrupted reference identifier is {unbroken_word} and must remain readable without widening the page."), Some(&unbroken_speaker)),
        line(3, 17, "00:02:00", "00:03:30", &paragraph, Some(&speaker)),
    ];
    loaded(job(17, JobStatus::Completed, &name), lines, false).view()
}

pub fn transcript_editing() -> TranscriptVm {
    let mut s = loaded(job(11, JobStatus::Completed, "podcast.mp3"), podcast_lines(), false);
    s.update(transcript::Event::EditStarted { line: LineId(2) });
    s.view()
}

/// FRE-020: a freemium account sees the display cut and the upgrade banner.
pub fn transcript_freemium() -> TranscriptVm {
    let mut j = job(14, JobStatus::Completed, "lecture.mp3");
    j.duration_minutes = Some(30);
    let lines = (0..40)
        .map(|i| {
            let start = i * 45;
            let at = |s: i64| format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60);
            line(100 + i, 14, &at(start), &at(start + 45), &format!("Sentence number {}.", i + 1), Some("SPEAKER_00"))
        })
        .collect();
    loaded(j, lines, true).view()
}

pub fn transcript_translation() -> TranscriptVm {
    let mut j = job(13, JobStatus::Completed, "talk.mp3");
    j.translate_to = Language::from_name("German").into_iter().collect();
    let mut original = line(1, 13, "00:00:00", "00:00:03", "Hello everyone", None);
    original.language = Language::from_name("English");
    original.translation_status = Some(TranslationStatus::Original);
    let mut translated = line(2, 13, "00:00:00", "00:00:03", "Hallo zusammen", None);
    translated.language = Language::from_name("German");
    translated.translation_status = Some(TranslationStatus::Translation);
    loaded(j, vec![original, translated], false).view()
}

pub fn transcript_youtube() -> TranscriptVm {
    let mut j = youtube_draft();
    j.status = JobStatus::Completed;
    j.id = JobId(15);
    let mut s = TranscriptState::default();
    s.update(transcript::Event::Loaded { job: j, lines: vec![line(1, 15, "00:00:00", "00:00:05", "Captions from the video.", None)], show_original: false, is_freemium: false });
    s.view()
}

pub fn transcript_no_audio() -> TranscriptVm {
    let mut s = TranscriptState::default();
    s.update(transcript::Event::Loaded { job: job(11, JobStatus::Completed, "podcast.mp3"), lines: podcast_lines(), show_original: false, is_freemium: false });
    s.update(transcript::Event::MediaUrlLoaded { job: JobId(11), result: Err(ApiFailure::http(404, None)) });
    s.view()
}

pub fn session_profile() -> SessionProfile {
    let flags = FlagConfig::default();
    let user = account().0.to_string();
    let subject = Subject { email: Some("ada@example.test"), user_id: Some(&user), locale: Some("en") };
    SessionProfile {
        account: account(),
        email: "ada@example.test".into(),
        full_name: Some("Ada Lovelace".into()),
        display_name: "Ada Lovelace".into(),
        joined_on: Some(Micros(NOW.0 - 90 * crate::ids::MICROS_PER_DAY)),
        capabilities: flags.capabilities(&subject, true),
    }
}

fn settings_at(tab: settings::Tab) -> SettingsVm {
    let profile = session_profile();
    let mut s = SettingsState::default();
    s.update(settings::Event::Opened, &profile.capabilities);
    s.update(settings::Event::TabSelected { tab }, &profile.capabilities);
    s.view(&profile)
}

pub fn settings_profile() -> SettingsVm {
    settings_at(settings::Tab::Profile)
}

pub fn settings_manage_data() -> SettingsVm {
    settings_at(settings::Tab::ManageData)
}

/// Without the AI capability the tab does not exist.
pub fn settings_without_ai() -> SettingsVm {
    let mut profile = session_profile();
    profile.capabilities = Capabilities { ai: false, ..profile.capabilities };
    let mut s = SettingsState::default();
    s.update(settings::Event::Opened, &profile.capabilities);
    s.view(&profile)
}

fn manage_data() -> DataManagementState {
    let mut s = DataManagementState::default();
    s.update(data_management::Event::Loaded { settings: UserSettings::defaults(account()) });
    s
}

pub fn manage_data_loaded() -> DataManagementVm {
    manage_data().view()
}

pub fn manage_data_confirm_delete_account() -> DataManagementVm {
    let mut s = manage_data();
    s.update(data_management::Event::DeleteAccountPressed);
    s.view()
}

pub fn manage_data_erasure_requested() -> DataManagementVm {
    let mut s = manage_data();
    s.update(data_management::Event::DeleteAccountPressed);
    s.update(data_management::Event::Confirmed);
    s.update(data_management::Event::ErasureState { view: ErasureView::Requested { waiting_for: vec!["subscription".into()] } });
    s.view()
}

pub fn manage_data_erasure_failed() -> DataManagementVm {
    let mut s = manage_data();
    s.update(data_management::Event::DeleteAccountPressed);
    s.update(data_management::Event::Confirmed);
    s.update(data_management::Event::ErasureFailed { failure: ApiFailure::http(503, None) });
    s.view()
}

fn feedback_open() -> FeedbackState {
    let mut s = FeedbackState::default();
    s.update(feedback::Event::Opened { job: JobId(11) });
    s
}

pub fn feedback_empty() -> FeedbackVm {
    feedback_open().view()
}

pub fn feedback_select_first() -> FeedbackVm {
    let mut s = feedback_open();
    s.update(feedback::Event::Submitted);
    s.view()
}

pub fn feedback_submitting() -> FeedbackVm {
    let mut s = feedback_open();
    s.update(feedback::Event::Selected { rating: FeedbackRating::Satisfied });
    s.update(feedback::Event::CommentChanged { text: "Accurate and fast.".into() });
    s.update(feedback::Event::Submitted);
    s.view()
}

pub fn feedback_thanked() -> FeedbackVm {
    let mut s = feedback_open();
    s.update(feedback::Event::Selected { rating: FeedbackRating::Outstanding });
    s.update(feedback::Event::Submitted);
    s.update(feedback::Event::Answered { result: Ok(()) });
    s.view()
}

pub fn feedback_failed() -> FeedbackVm {
    let mut s = feedback_open();
    s.update(feedback::Event::Selected { rating: FeedbackRating::Disappointed });
    s.update(feedback::Event::Submitted);
    s.update(feedback::Event::Answered { result: Err(ApiFailure::network()) });
    s.view()
}

fn support_open(text: &str) -> SupportState {
    let mut s = SupportState::default();
    s.update(support::Event::Opened);
    s.update(support::Event::MessageChanged { text: text.into() });
    s
}

pub fn support_typing() -> SupportVm {
    support_open("The transcript stops after ten minutes.").view()
}

pub fn support_sending() -> SupportVm {
    let mut s = support_open("The transcript stops after ten minutes.");
    s.update(support::Event::Submitted { job: Some(JobId(11)) });
    s.view()
}

pub fn support_failed() -> SupportVm {
    let mut s = support_open("The transcript stops after ten minutes.");
    s.update(support::Event::Submitted { job: Some(JobId(11)) });
    s.update(support::Event::Answered { result: Err(ApiFailure::http(503, None)) });
    s.view()
}

fn signed_out_app(url: &str, captcha: bool) -> AppState {
    let mut s = AppState::new(AppConfig { site_url: "https://app.example.test".into(), captcha });
    s.update(AppEvent::User(AppIntent::Started { url: url.into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
    let epoch = s.epoch();
    s.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: None } }, NOW);
    s
}

fn auth_intents(s: &mut AppState, intents: Vec<AuthIntent>) {
    for intent in intents {
        s.update(AppEvent::User(AppIntent::Auth(intent)), NOW);
    }
}

fn signed_out(s: &AppState) -> SignedOutVm {
    match s.view(NOW) {
        AppVm::SignedOut(vm) => vm,
        other => panic!("signed out expected: {other:?}"),
    }
}

fn auth_in(mode: AuthMode) -> AuthVm {
    let mut s = signed_out_app("/signin", false);
    auth_intents(&mut s, vec![AuthIntent::ModeChosen { mode }, AuthIntent::EmailChanged { text: "ada@example.test".into() }]);
    signed_out(&s).auth
}

pub fn auth_fixtures() -> Vec<(&'static str, AuthVm)> {
    let email = || AuthIntent::EmailChanged { text: "ada@example.test".into() };
    let password = || AuthIntent::PasswordChanged { text: "correct horse".into() };

    let mut busy = signed_out_app("/signin", false);
    auth_intents(&mut busy, vec![email(), password(), AuthIntent::Submitted]);

    let mut notice = signed_out_app("/signin", false);
    auth_intents(&mut notice, vec![AuthIntent::ModeChosen { mode: AuthMode::MagicLink }, email(), AuthIntent::Submitted]);
    let epoch = notice.epoch();
    notice.update(AppEvent::Auth { epoch, event: AuthEvent::EmailSent }, NOW);

    let mut error = signed_out_app("/signin", false);
    auth_intents(&mut error, vec![email(), password(), AuthIntent::Submitted]);
    let epoch = error.epoch();
    error.update(AppEvent::Auth { epoch, event: AuthEvent::Failed { kind: AuthFailure::InvalidCredentials } }, NOW);

    let captcha = signed_out_app("/signin", true);
    vec![
        ("sign_in", auth_in(AuthMode::SignIn)),
        ("sign_up", auth_in(AuthMode::SignUp)),
        ("magic_link", auth_in(AuthMode::MagicLink)),
        ("forgotten_password", auth_in(AuthMode::ForgottenPassword)),
        ("busy", signed_out(&busy).auth),
        ("notice", signed_out(&notice).auth),
        ("error", signed_out(&error).auth),
        ("captcha_required", signed_out(&captcha).auth),
    ]
}

pub fn new_password_fixtures() -> Vec<(&'static str, NewPasswordVm)> {
    let typed = |s: &mut AppState| {
        s.update(AppEvent::User(AppIntent::NewPassword(NewPasswordIntent::PasswordChanged { text: "n3w-secret".into() })), NOW);
    };
    let submit = |s: &mut AppState| {
        s.update(AppEvent::User(AppIntent::NewPassword(NewPasswordIntent::Submitted)), NOW);
    };
    let empty = signed_out_app("/new-password", false);
    let mut busy = signed_out_app("/new-password", false);
    typed(&mut busy);
    submit(&mut busy);
    let mut error = signed_out_app("/new-password", false);
    typed(&mut error);
    submit(&mut error);
    let epoch = error.epoch();
    error.update(AppEvent::Auth { epoch, event: AuthEvent::Failed { kind: AuthFailure::WeakPassword } }, NOW);
    let mut success = signed_out_app("/new-password", false);
    typed(&mut success);
    submit(&mut success);
    let epoch = success.epoch();
    success.update(AppEvent::Auth { epoch, event: AuthEvent::PasswordUpdated }, NOW);
    vec![
        ("empty", signed_out(&empty).new_password),
        ("busy", signed_out(&busy).new_password),
        ("error", signed_out(&error).new_password),
        ("success", signed_out(&success).new_password),
    ]
}

fn signed_in_app(url: &str, viewport: Viewport) -> AppState {
    let mut state = AppState::new(AppConfig { site_url: "https://app.example.test".into(), captcha: false });
    state.update(AppEvent::User(AppIntent::Started { url: url.into(), viewport, utc_offset_minutes: 0 }), NOW);
    let epoch = state.epoch();
    state.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: Some(SessionFacts {
        user_id: account().0, email: "ada@example.test".into(), expires_at: Micros(NOW.0 + 3_600_000_000),
    }) } }, NOW);
    let epoch = state.epoch();
    state.update(AppEvent::SessionLoaded { epoch, result: Ok(session_profile()) }, NOW);
    state.update(AppEvent::Balance { epoch, result: Ok(BalanceView { balance: Some(75), is_freemium: false, special_client_alert: None }) }, NOW);
    state.update(AppEvent::Jobs { epoch, event: job_list::Event::PageLoaded { jobs: history_jobs(), next_cursor: None, replace: true } }, NOW);
    if url == "/home/11" {
        state.update(AppEvent::JobLoaded { epoch, job: JobId(11), result: Ok(crate::jobs::app::JobDetail {
            job: job(11, JobStatus::Completed, "podcast.mp3"), lines: podcast_lines(),
        }) }, NOW);
    }
    state
}

fn signed_in_view(state: &AppState) -> SignedInVm {
    match state.view(NOW) {
        AppVm::SignedIn(vm) => *vm,
        other => panic!("signed in expected: {other:?}"),
    }
}

/// WEB-045/046 states, each driven through the signed-in app on `/dashboard/api-keys`.
pub fn api_keys_fixtures() -> Vec<(&'static str, crate::presentation::api_keys::ApiKeysVm)> {
    use crate::api::KeyId;
    use crate::api::keys::{CreatedKey, ExpirationPolicy, KeyView};
    use crate::presentation::api_keys::{Copied, Event, Intent};
    use crate::presentation::app::AppIntent;
    use crate::presentation::layout::Viewport;
    fn key(n: u128, name: &str, scopes: &[&str], expires_at: Option<Micros>, active: bool) -> KeyView {
        KeyView {
            id: KeyId(uuid::Uuid::from_u128(n)),
            name: name.into(),
            created_at: Micros(NOW.0 - 10 * crate::ids::MICROS_PER_DAY),
            expires_at,
            active,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expiration_policy: if expires_at.is_some() { ExpirationPolicy::Days30 } else { ExpirationPolicy::Never },
            hint: Some("sk_Ab3x".into()),
        }
    }
    let keys = vec![
        key(1, "CI pipeline", &["transcriptions:read", "transcriptions:create"], None, true),
        key(2, "Reporting", &["transcriptions:read"], Some(Micros(NOW.0 + 3 * crate::ids::MICROS_PER_DAY)), true),
        key(3, "Old integration", &["transcriptions:create"], Some(Micros(NOW.0 - crate::ids::MICROS_PER_DAY)), false),
    ];
    let screen = |events: Vec<Event>, intents: Vec<Intent>| {
        let mut s = signed_in_app("/dashboard/api-keys", Viewport { touch: false, width: 1440 });
        let epoch = s.epoch();
        for event in events {
            s.update(AppEvent::ApiKeys { epoch, event }, NOW);
        }
        for intent in intents {
            s.update(AppEvent::User(AppIntent::ApiKeys(intent)), NOW);
        }
        let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
        vm.api_keys.expect("the key screen is open")
    };
    let loaded = || vec![Event::KeysLoaded { result: Ok(keys.clone()) }, Event::SecretLoaded { result: Ok("whsec_7f3a9c2e41b8d6".into()) }];
    let created = CreatedKey { key: key(4, "New key", &["transcriptions:read"], None, true), api_key: "sk_q1W2e3R4t5Y6u7I8o9P0a1S2d3F4g5H6j7K8l9Z0x1C=".into() };
    let with = |mut events: Vec<Event>, extra: Vec<Event>| {
        events.extend(extra);
        events
    };
    vec![
        ("loading", screen(vec![], vec![])),
        ("empty", screen(vec![Event::KeysLoaded { result: Ok(vec![]) }, Event::SecretLoaded { result: Ok("whsec_7f3a9c2e41b8d6".into()) }], vec![])),
        ("list", screen(loaded(), vec![])),
        ("creating_form", screen(loaded(), vec![Intent::FormToggled, Intent::NameChanged { text: "Nightly export".into() }, Intent::PolicyChosen { policy: ExpirationPolicy::Custom }])),
        ("created", {
            let mut s = signed_in_app("/dashboard/api-keys", Viewport { touch: false, width: 1440 });
            let epoch = s.epoch();
            for event in loaded() {
                s.update(AppEvent::ApiKeys { epoch, event }, NOW);
            }
            for intent in [Intent::FormToggled, Intent::NameChanged { text: "New key".into() }, Intent::CreateRequested] {
                s.update(AppEvent::User(AppIntent::ApiKeys(intent)), NOW);
            }
            s.update(AppEvent::ApiKeys { epoch, event: Event::Created { result: Ok(created.clone()) } }, NOW);
            s.update(AppEvent::User(AppIntent::ApiKeys(Intent::CopyRequested { what: Copied::Key })), NOW);
            s.update(AppEvent::ApiKeys { epoch, event: Event::CopyAnswered { what: Copied::Key, ok: true } }, NOW);
            let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
            vm.api_keys.expect("the key screen is open")
        }),
        ("confirm_revoke", screen(loaded(), vec![Intent::RevokeRequested { id: KeyId(uuid::Uuid::from_u128(1)) }])),
        ("load_failed", screen(with(vec![Event::KeysLoaded { result: Err(ApiFailure::http(503, None)) }], vec![Event::SecretLoaded { result: Err(ApiFailure::network()) }]), vec![])),
    ]
}

pub fn signed_in_fixtures() -> Vec<(&'static str, SignedInVm)> {
    use billing_core::presentation::{fixtures as billing, invoices::InvoicesEvent};
    let desktop = Viewport { touch: false, width: 1440 };
    let new = signed_in_app("/home?view=new", desktop);
    let phone = signed_in_app("/home/11", Viewport { touch: true, width: 390 });
    let mut settings = signed_in_app("/home", desktop);
    settings.update(AppEvent::User(AppIntent::Settings(settings::Event::Opened)), NOW);
    let mut invoices = signed_in_app("/billing", desktop);
    let epoch = invoices.epoch();
    invoices.update(AppEvent::Invoices { epoch, ticket: invoices.invoice_preview_ticket(), event: InvoicesEvent::Loaded(Ok(billing::invoice_context_sample())) }, NOW);
    let mut purchase = signed_in_app("/home", desktop);
    let epoch = purchase.epoch();
    purchase.update(AppEvent::User(AppIntent::PurchaseOpened), NOW);
    purchase.update(AppEvent::Catalog { epoch, result: Ok(billing::catalog()) }, NOW);
    let mut special = signed_in_app("/home", desktop);
    let epoch = special.epoch();
    special.update(AppEvent::Balance { epoch, result: Ok(BalanceView {
        balance: Some(75), is_freemium: false,
        special_client_alert: Some(billing_core::pricing::SpecialClientAlert {
            code: "LOYAL".into(), percent: 25, message: "A configured offer for returning clients.".into(),
        }),
    }) }, NOW);
    vec![
        ("desktop_new", signed_in_view(&new)),
        ("phone_job", signed_in_view(&phone)),
        ("settings_open", signed_in_view(&settings)),
        ("billing_owner", signed_in_view(&invoices)),
        ("purchase_open", signed_in_view(&purchase)),
        ("special_offer", signed_in_view(&special)),
    ]
}

pub fn app_fixtures() -> Vec<(&'static str, AppVm)> {
    use billing_core::presentation::confirm::{ConfirmPayload, ConfirmedPayment};
    use crate::presentation::app::AppEffect;
    let mut states: Vec<_> = signed_in_fixtures().into_iter().map(|(name, vm)| (name, AppVm::SignedIn(Box::new(vm)))).collect();
    let mut restoring = AppState::new(AppConfig { site_url: "https://app.example.test".into(), captcha: false });
    restoring.update(AppEvent::User(AppIntent::Started { url: "/home".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
    states.push(("restoring", restoring.view(NOW)));
    states.push(("signed_out", signed_out_app("/signin", false).view(NOW)));
    let mut erased = signed_in_app("/home", Viewport::default());
    let epoch = erased.epoch();
    erased.update(AppEvent::AccountErased { epoch }, NOW);
    states.push(("erased", erased.view(NOW)));
    for (name, status, success) in [
        ("confirmation_succeeded", billing_core::grants::PaymentStatus::Completed, true),
        ("confirmation_declined", billing_core::grants::PaymentStatus::Failed, false),
        ("confirmation_pending", billing_core::grants::PaymentStatus::Pending, false),
    ] {
        let mut state = AppState::new(AppConfig { site_url: "https://app.example.test".into(), captcha: false });
        let effects = state.update(AppEvent::User(AppIntent::Started { url: "/checkout/confirm".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
        let (epoch, sequence) = effects.into_iter().find_map(|e| match e {
            AppEffect::ReadConfirmation { epoch, sequence } => Some((epoch, sequence)),
            _ => None,
        }).expect("confirmation read");
        state.update(AppEvent::ConfirmationLoaded { epoch, sequence, payload: Some(ConfirmPayload::Confirmed { view: ConfirmedPayment {
            payment_id: 42, status, success, changed: false, analytics: vec![], redirect_after_seconds: 5,
        } }) }, NOW);
        states.push((name, state.view(NOW)));
    }
    states
}

type Named = Vec<(String, Value)>;

fn named<T: Serialize>(states: Vec<(&str, T)>) -> Named {
    states.into_iter().map(|(name, vm)| (name.to_string(), serde_json::to_value(vm).expect("a view model serializes"))).collect()
}

fn billing_message_fixtures() -> Named {
    use billing_core::presentation::{fixtures as billing, subscription::SubscriptionState};
    let empty_subscription = SubscriptionState::default().view();
    let mut out = Vec::new();
    for (name, purchase) in billing::purchase() {
        out.push((format!("purchase_{name}"), serde_json::to_value(super::billing::messages(Some(&purchase), &empty_subscription, None, false, NOW)).unwrap()));
    }
    for (name, subscription) in billing::subscription() {
        out.push((format!("subscription_{name}"), serde_json::to_value(super::billing::messages(None, &subscription, None, false, NOW)).unwrap()));
    }
    for (name, invoices) in billing::invoices() {
        out.push((format!("invoices_{name}"), serde_json::to_value(super::billing::messages(None, &empty_subscription, Some(&invoices), false, NOW)).unwrap()));
    }
    out
}

/// Every named state by view model type name, as `fixtures.json` holds it.
pub fn export() -> BTreeMap<&'static str, Named> {
    use crate::presentation::ai::fixtures as ai;
    use billing_core::presentation::fixtures as billing;
    use crate::presentation::billing as billing_copy;
    BTreeMap::from([
        (
            "UploadQueueVm",
            named(vec![
                ("empty", queue_empty()),
                ("uploading", queue_uploading()),
                ("preparing", queue_preparing()),
                ("upload_failed", queue_upload_failed()),
                ("ready", queue_ready()),
                ("translation", queue_translation()),
                ("starting", queue_starting()),
                ("language_alert", queue_language_alert()),
                ("balance_alert", queue_balance_alert()),
                ("processing_timeout", queue_processing_timeout()),
                ("max_files", queue_max_files()),
                ("youtube", queue_youtube()),
                ("youtube_error", queue_youtube_error()),
            ]),
        ),
        (
            "JobListVm",
            named(vec![
                ("sample", history_sample()),
                ("empty", history_empty()),
                ("loading", history_loading()),
                ("loading_more", history_loading_more()),
                ("error", history_error()),
                ("confirm_delete", history_confirm_delete()),
                ("downloading", history_downloading()),
            ]),
        ),
        (
            "TranscriptVm",
            named(vec![
                ("running", transcript_running()),
                ("speaker_labeling", transcript_speaker_labeling()),
                ("completed", transcript_completed()),
                ("rtl", transcript_rtl()),
                ("long_strings", transcript_long_strings()),
                ("editing", transcript_editing()),
                ("freemium", transcript_freemium()),
                ("translation", transcript_translation()),
                ("youtube", transcript_youtube()),
                ("no_audio", transcript_no_audio()),
            ]),
        ),
        ("SessionProfile", named(vec![("signed_in", session_profile())])),
        (
            "SettingsVm",
            named(vec![("profile", settings_profile()), ("manage_data", settings_manage_data()), ("without_ai", settings_without_ai())]),
        ),
        (
            "DataManagementVm",
            named(vec![
                ("loaded", manage_data_loaded()),
                ("confirm_delete_account", manage_data_confirm_delete_account()),
                ("erasure_requested", manage_data_erasure_requested()),
                ("erasure_failed", manage_data_erasure_failed()),
            ]),
        ),
        (
            "FeedbackVm",
            named(vec![
                ("empty", feedback_empty()),
                ("select_first", feedback_select_first()),
                ("submitting", feedback_submitting()),
                ("thanked", feedback_thanked()),
                ("failed", feedback_failed()),
            ]),
        ),
        ("SupportVm", named(vec![("typing", support_typing()), ("sending", support_sending()), ("failed", support_failed())])),
        ("ApiKeysVm", named(api_keys_fixtures())),
        ("AuthVm", named(auth_fixtures())),
        ("NewPasswordVm", named(new_password_fixtures())),
        ("SignedInVm", named(signed_in_fixtures())),
        ("AppVm", named(app_fixtures())),
        ("PurchaseVm", named(billing::purchase())),
        ("SubscriptionVm", named(billing::subscription())),
        ("InvoicesVm", named(billing::invoices())),
        ("ConfirmVm", named(billing::confirm())),
        ("ConfirmMessages", named(billing::confirm().into_iter().map(|(name, vm)| (name, billing_copy::confirmation(&vm))).collect())),
        ("BillingMessages", billing_message_fixtures()),
        ("ConsentPanelVm", named(ai::consent())),
        ("AnalysisPanelVm", named(ai::analysis())),
        ("ChatPanelVm", named(ai::chat())),
    ])
}

/// `fixtures.json`: `{"<VmTypeName>": {"<fixture name>": <view model>}}`.
pub fn export_json() -> Value {
    Value::Object(
        export()
            .into_iter()
            .map(|(ty, states)| (ty.to_string(), Value::Object(states.into_iter().collect())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::msg::app;

    // Fixtures stay consistent with the state machines that produce them.
    #[test]
    fn named_screens_render() {
        assert!(queue_empty().rows.is_empty());
        assert_eq!(queue_uploading().rows[0].chip, upload_queue::Chip::Uploading { progress: 42 });
        assert!(!queue_ready().start_disabled);
        assert_eq!((queue_preparing().total_minutes, queue_preparing().files_ready), (None, false));
        assert_eq!(queue_youtube_error().youtube_error, Some(upload_queue::youtube_error_message(Some("no_captions"))));
        assert_eq!(history_sample().rows.len(), 4);
        assert!(history_loading().initial_loading && history_loading_more().loading_more);
        assert_eq!(transcript_running().loading_message, Some(app("Transcribing audio...")));
        assert_eq!(transcript_completed().rows.len(), 3);
        assert!(transcript_freemium().upgrade_banner);
        assert!(transcript_no_audio().no_audio_alert.is_some());
        let t = transcript_translation();
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].text, "Hallo zusammen");
        assert!(feedback_thanked().thanked && support_failed().notice.is_some());
    }

    #[test]
    fn transcript_rtl_fixture_preserves_arabic_direction() {
        let fixtures = export_json();
        let vm: TranscriptVm = serde_json::from_value(
            fixtures["TranscriptVm"].get("rtl").expect("missing RTL transcript fixture").clone(),
        ).unwrap();
        assert!(vm.rtl, "RTL coverage must use the core direction projection");
        assert_eq!(vm.selected.as_ref().unwrap().language.name(), "Arabic");
        assert!(vm.rows.iter().all(|row| row.text.chars().any(|c| ('\u{0600}'..='\u{06ff}').contains(&c))));
        assert!(vm.show_speakers && vm.speaker_toggle);
        assert!(!transcript_completed().rtl);
    }

    #[test]
    fn transcript_long_strings_fixture_exercises_wrapping_boundaries() {
        let fixtures = export_json();
        let vm: TranscriptVm = serde_json::from_value(
            fixtures["TranscriptVm"].get("long_strings").expect("missing long-string transcript fixture").clone(),
        ).unwrap();
        assert!(vm.rows.iter().any(|row| row.text.chars().count() >= 1_000), "cover multi-line wrapping");
        assert!(vm.rows.iter().any(|row| row.text.split_whitespace().any(|word| word.chars().count() >= 200)),
            "cover an unbroken word wider than either viewport");
        assert!(vm.speaker_options.iter().any(|option| option.value.chars().count() >= 120),
            "cover long speaker labels and menu entries");
        assert!(vm.show_speakers && vm.speaker_toggle);
    }

    #[test]
    fn every_fixture_has_a_unique_name_per_view_model() {
        for (ty, states) in export() {
            let mut names: Vec<&str> = states.iter().map(|(n, _)| n.as_str()).collect();
            let total = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), total, "{ty} has duplicate fixture names");
            assert!(total > 0, "{ty} has no fixtures");
        }
        assert!(export_json()["TranscriptVm"]["completed"]["rows"].is_array());
    }
}

/// Writes the TypeScript declarations and `fixtures.json` for the web shell:
/// `cargo test -p transgate-core --features ts --lib presentation::fixtures::web::generate`.
/// Output goes only to `transgate/web/src/lib/core/` (never `TS_RS_EXPORT_DIR`).
#[cfg(all(test, feature = "ts"))]
mod web {
    use std::any::{type_name, TypeId};
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    use ts_rs::{Config, TypeVisitor, TS};

    use super::export_json;
    use crate::account::SessionProfile;
    use crate::presentation::ai::analysis::AnalysisPanelVm;
    use crate::presentation::ai::chat::ChatPanelVm;
    use crate::presentation::ai::consent::ConsentPanelVm;
    use crate::presentation::data_management::DataManagementVm;
    use crate::presentation::feedback::FeedbackVm;
    use crate::presentation::job_list::JobListVm;
    use crate::presentation::layout::{Layouts, Viewport};
    use crate::presentation::msg::Msg;
    use crate::presentation::route::Route;
    use crate::presentation::settings::SettingsVm;
    use crate::presentation::support::SupportVm;
    use crate::presentation::transcript::TranscriptVm;
    use crate::presentation::upload_queue::{PickedFile, UploadQueueVm};
    use crate::presentation::app::{AppIntent, AppVm, SignedInVm, AuthVm, NewPasswordVm};
    use crate::presentation::api_keys::ApiKeysVm;
    use billing_core::presentation::confirm::ConfirmVm;
    use billing_core::presentation::invoices::InvoicesVm;
    use billing_core::presentation::purchase::PurchaseVm;
    use billing_core::presentation::subscription::SubscriptionVm;

    // ts-rs silently keeps the first declaration for an existing path/name.
    // Validate the complete graph before export_all can write any output.
    struct ExportIntegrity<'a> {
        cfg: &'a Config,
        seen: HashSet<TypeId>,
        names: HashMap<String, &'static str>,
        paths: HashMap<PathBuf, &'static str>,
    }

    impl<'a> ExportIntegrity<'a> {
        fn new(cfg: &'a Config) -> Self {
            Self { cfg, seen: HashSet::new(), names: HashMap::new(), paths: HashMap::new() }
        }
    }

    impl TypeVisitor for ExportIntegrity<'_> {
        fn visit<T: TS + 'static + ?Sized>(&mut self) {
            let Some(path) = T::output_path() else { return };
            if !self.seen.insert(TypeId::of::<T>()) {
                return;
            }
            // Generic instantiations share one declaration, but their concrete
            // dependency graphs must each be traversed.
            let rust_type = type_name::<T::WithoutGenerics>();
            let name = T::ident(self.cfg);
            if let Some(previous) = self.names.get(&name) {
                assert_eq!(
                    *previous, rust_type,
                    "duplicate TypeScript name {name}: {previous} and {rust_type}"
                );
            } else {
                self.names.insert(name, rust_type);
            }
            if let Some(previous) = self.paths.get(&path) {
                assert_eq!(
                    *previous, rust_type,
                    "duplicate TypeScript output path {}: {previous} and {rust_type}",
                    path.display()
                );
            } else {
                self.paths.insert(path, rust_type);
            }
            T::visit_dependencies(self);
        }
    }

    fn fixture_module(fixtures: &serde_json::Value, json: &str) -> String {
        use std::fmt::Write;

        let roots = fixtures.as_object().expect("fixtures are keyed by view-model type");
        let mut module = String::from("// Generated by transgate-core; do not edit. Values are contextually checked against generated VM types.\n");
        for name in roots.keys() {
            writeln!(module, "import type {{ {name} }} from \"./types/{name}\";").unwrap();
        }
        module.push_str("\nexport interface Fixtures {\n");
        for name in roots.keys() {
            writeln!(module, "  {name}: {name};").unwrap();
        }
        module.push_str("}\n\nexport type FixtureFile = { [K in keyof Fixtures]: Record<string, Fixtures[K]> };\n\nexport const fixtures: FixtureFile = ");
        module.push_str(json);
        module.push_str(";\n");
        module
    }

    macro_rules! publish_roots {
        ($core:expr; $($ty:ty),+ $(,)?) => {{
            let core: &Path = $core;
            let cfg = Config::new().with_out_dir(core.join("types")).with_large_int("number");
            let mut integrity = ExportIntegrity::new(&cfg);
            $(integrity.visit::<$ty>();)+
            let fixtures = export_json();
            let json = serde_json::to_string_pretty(&fixtures).unwrap();
            let module = fixture_module(&fixtures, &json);
            $(<$ty>::export_all(&cfg).unwrap();)+
            std::fs::write(core.join("fixtures.json"), json + "\n").unwrap();
            std::fs::write(core.join("fixtures.generated.ts"), module).unwrap();
        }};
    }

    #[allow(dead_code)]
    mod collision_fixtures {
        use ts_rs::TS;

        #[derive(TS)]
        #[ts(rename = "Duplicated")]
        pub struct FirstName(pub String);
        #[derive(TS)]
        #[ts(rename = "Duplicated")]
        pub struct SecondName(pub bool);
        #[derive(TS)]
        pub struct NestedNames(pub FirstName, pub SecondName);

        #[derive(TS)]
        #[ts(export_to = "collision.ts")]
        pub struct FirstPath(pub String);
        #[derive(TS)]
        #[ts(export_to = "collision.ts")]
        pub struct SecondPath(pub bool);

        #[derive(TS)]
        pub struct SharedLeft(pub FirstName);
        #[derive(TS)]
        pub struct SharedRight(pub FirstName);
        #[derive(TS)]
        pub struct BeforeConflict(pub i32);
    }

    #[test]
    #[should_panic(expected = "duplicate TypeScript name Duplicated")]
    fn generate_rejects_colliding_nested_names() {
        let cfg = Config::new();
        ExportIntegrity::new(&cfg).visit::<collision_fixtures::NestedNames>();
    }

    #[test]
    #[should_panic(expected = "duplicate TypeScript output path collision.ts")]
    fn generate_rejects_colliding_root_paths() {
        let cfg = Config::new();
        let mut integrity = ExportIntegrity::new(&cfg);
        integrity.visit::<collision_fixtures::FirstPath>();
        integrity.visit::<collision_fixtures::SecondPath>();
    }

    #[test]
    fn export_integrity_preserves_outputs_on_rejection() {
        use std::collections::BTreeMap;

        use collision_fixtures::*;

        fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
            fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        collect(root, &path, files);
                    } else {
                        files.insert(path.strip_prefix(root).unwrap().to_owned(), std::fs::read(path).unwrap());
                    }
                }
            }
            let mut files = BTreeMap::new();
            collect(root, root, &mut files);
            files
        }

        fn conflicting_names(core: &Path) {
            publish_roots!(core; BeforeConflict, NestedNames);
        }

        fn conflicting_paths(core: &Path) {
            publish_roots!(core; BeforeConflict, FirstPath, SecondPath);
        }

        let core = std::env::temp_dir().join(format!("transgate-export-integrity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&core).unwrap();
        let result = std::panic::catch_unwind(|| {
            // The same dependency reached through two roots, and a repeated
            // root itself, must publish successfully rather than collide.
            publish_roots!(&core; SharedLeft, SharedRight, SharedLeft);
            let before = snapshot(&core);
            assert!(before.contains_key(Path::new("types/Duplicated.ts")));
            assert!(before.contains_key(Path::new("fixtures.json")));
            assert!(before.contains_key(Path::new("fixtures.generated.ts")));

            type Case = (&'static str, fn(&Path));
            let cases: [Case; 2] = [
                ("duplicate TypeScript name Duplicated", conflicting_names),
                ("duplicate TypeScript output path collision.ts", conflicting_paths),
            ];
            for (expected, publish) in cases {
                let failure = std::panic::catch_unwind(|| publish(&core))
                    .expect_err("a conflicting reachable graph must reject publication");
                let message = failure.downcast_ref::<String>().map(String::as_str)
                    .or_else(|| failure.downcast_ref::<&str>().copied())
                    .expect("integrity rejection must identify the collision");
                assert!(message.contains(expected), "unexpected rejection: {message}");
                // Compare the whole tree: no replaced bytes, removed files,
                // or premature BeforeConflict.ts may escape the preflight.
                assert_eq!(snapshot(&core), before, "{expected} changed existing publication");
            }
        });
        std::fs::remove_dir_all(&core).unwrap();
        if let Err(failure) = result {
            std::panic::resume_unwind(failure);
        }
    }

    #[test]
    fn generate() {
        let core = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/src/lib/core");
        publish_roots!(&core;
            UploadQueueVm, PickedFile, JobListVm, TranscriptVm, SessionProfile,
            SettingsVm, DataManagementVm, FeedbackVm, SupportVm, ConsentPanelVm,
            AnalysisPanelVm, ChatPanelVm, Layouts, Viewport, Route, Msg, AppIntent,
            AuthVm, NewPasswordVm, AppVm, SignedInVm, PurchaseVm, SubscriptionVm,
            InvoicesVm, ConfirmVm, ApiKeysVm,
        );
    }
}
