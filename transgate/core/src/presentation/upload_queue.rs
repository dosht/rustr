//! The "New transcript" queue (UPL-001..008, UPL-013..021, UPL-025/027/028,
//! JOB-003/005, LNG-008/011/012), ported from `useTranscriptionViewModel.ts`.
//!
//! Files get a stable local id instead of being keyed by name (JOB-D07): two
//! files with the same name stay distinct. The shell keeps each `File` under
//! the opaque `handle` it chose; bytes never cross into core. Everything else
//! is as built: the desktop picker is not validated or capped (UPL-003/004),
//! while the phone picker filters through `validateFile` and 500 MiB
//! (`MobileTranscriptionContainer.tsx:120-157`).

use serde::{Deserialize, Serialize};

use crate::ids::JobId;
use crate::jobs::domain::Job;
use crate::languages::Language;
use crate::presentation::layout::Frame;
use crate::presentation::msg::{Msg, app, common};
use crate::presentation::failure::ApiFailure;
use crate::presentation::telemetry::TelemetryEvent;

pub const MAX_FILES: usize = 10;
/// UPL-008: 1 s for ten polls, then 2 s growing by 1.5 up to 30 s; give up after 5 minutes.
pub const POLL_TIMEOUT_MS: u64 = 5 * 60 * 1000;

pub fn poll_interval_ms(poll_count: u32) -> u64 {
    if poll_count < 10 {
        return 1000;
    }
    let backoff = (poll_count - 10) as i32;
    (2000.0 * 1.5f64.powi(backoff)).min(30_000.0) as u64
}

/// UPL-003 drop filters, both as built: the drop zone keeps audio/video MIME or
/// one of seven extensions in any case; the view model then keeps audio/video
/// MIME or a name ending in lower case `.m4a`.
pub fn accept_dropped(mime: &str, name: &str) -> bool {
    let lower = name.to_lowercase();
    let zone = mime.starts_with("audio/")
        || mime.starts_with("video/")
        || [".m4a", ".mp3", ".wav", ".mp4", ".avi", ".mov", ".ogg"].iter().any(|e| lower.ends_with(e));
    let model = mime.starts_with("audio/") || mime.starts_with("video/") || name.ends_with(".m4a");
    zone && model
}

/// UPL-002: picker accept attributes per layout.
pub const DESKTOP_PICKER_ACCEPT: &str = "audio/*,video/*,.m4a";
pub const PHONE_PICKER_ACCEPT: &str = "audio/mpeg,audio/mp4,audio/wav,audio/webm,.mp3,.m4a,.wav,.webm";

/// The phone picker's size limit, inclusive.
pub const PHONE_MAX_BYTES: i64 = 500 * 1024 * 1024;

/// The phone picker's `validateFile`: audio or video MIME, or one of six
/// extensions in any case; then at most 500 MiB.
pub fn phone_accepts(file: &PickedFile) -> bool {
    let lower = file.name.to_lowercase();
    let format = file.mime.starts_with("audio/")
        || file.mime.starts_with("video/")
        || [".m4a", ".mp3", ".wav", ".mp4", ".webm", ".ogg"].iter().any(|e| lower.ends_with(e));
    format && file.size <= PHONE_MAX_BYTES
}

/// UPL-006: seconds to whole minutes, rounded up.
pub fn minutes_from_seconds(seconds: f64) -> i32 {
    billing_core::ledger::minutes_from_seconds(seconds) as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Transcription,
    Translation,
}

/// A language choice; auto-detect is stored as NULL (UPL-017, LNG-009).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", content = "language", rename_all = "snake_case")]
pub enum Choice {
    AutoDetect,
    Language(Language),
}

impl Choice {
    pub fn stored(self) -> Option<Language> {
        match self {
            Choice::AutoDetect => None,
            Choice::Language(l) => Some(l),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    File,
    YouTube,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UploadStatus {
    Uploading { progress: u8 },
    Uploaded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: u64,
    pub name: String,
    pub size: i64,
    pub status: UploadStatus,
    pub source: Source,
    /// Whole minutes; `None` until measured or when measurement failed.
    pub duration: Option<i32>,
    pub job: Option<JobId>,
    /// The shell's handle of the picked `File`; `None` for restored drafts
    /// and YouTube rows, which have no local file.
    pub handle: Option<u64>,
}

/// A file chosen by the picker or dropped. `handle` is the shell's opaque key
/// of the `File`, echoed in the measure and upload effects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PickedFile {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub handle: u64,
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: i64,
    pub mime: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// JOB-003: drafts restored on load, with the language usage (UPL-021).
    Restored { drafts: Vec<Job>, preferred: Vec<Language>, preferred_targets: Vec<Language> },
    /// The picker. Desktop: no validation and no cap (UPL-003/004 as built).
    /// Phone: [`phone_accepts`] drops the rest silently.
    Picked { files: Vec<PickedFile>, frame: Frame },
    /// Drag and drop: ignored at ten, filtered twice.
    Dropped { files: Vec<PickedFile> },
    /// UPL-006: the shell's measurement result in seconds, `None` when both methods failed.
    Measured { file: u64, seconds: Option<f64> },
    DraftCreated { file: u64, job: Job },
    DraftFailed { file: u64 },
    UploadProgress { file: u64, progress: u8 },
    /// UPL-013/014: only HTTP 200 counts as uploaded.
    UploadFinished { file: u64, ok: bool },
    Removed { file: u64 },
    LanguageChosen { choice: Option<Choice> },
    SpeakersToggled { enabled: bool },
    /// The mode tab; `frame` selects the phone's target default (the app injects it).
    TabChanged { mode: Mode, frame: Frame },
    SourceLanguageChosen { language: Option<Language> },
    TargetLanguageChosen { language: Option<Language> },
    /// UPL-008 poll tick, with the drafts reread and the elapsed time.
    DraftsReloaded { drafts: Vec<Job>, elapsed_ms: u64 },
    StartPressed,
    StartSucceeded { first_job: JobId },
    /// A 402 or a balance message shows the balance alert.
    StartFailed { balance: bool },
    /// The "Add YouTube Video" dialog (`YouTubeDialog.tsx`).
    YouTubeDialogOpened,
    YouTubeDialogClosed,
    YouTubeUrlChanged { url: String },
    /// The dialog's URL was submitted: validated (UPL-022), then
    /// `POST /api/youtube/add` with the chosen language.
    YouTubeSubmitted,
    /// The add request failed (UPL-023); the dialog shows why and stays open.
    YouTubeAddFailed { failure: ApiFailure },
    YouTubeAdded { job: Job, title: String, duration_seconds: f64, detected_language: Option<String> },
    YouTubeResolved { job: JobId, completed: bool, code: Option<String> },
    YouTubeResolveCrashed,
    DismissAlert { alert: Alert },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Alert {
    Balance,
    Language,
    ProcessingTimeout,
    YouTube,
}

impl Alert {
    /// The alert's heading (`UploadAlerts.svelte`).
    pub fn title(self) -> Msg {
        common(match self {
            Alert::Balance => "Not enough balance.",
            Alert::Language => "Language Selection Required",
            Alert::ProcessingTimeout => "Processing Timeout",
            Alert::YouTube => "YouTube Transcription Failed",
        })
    }

    /// The alert's fixed paragraphs. The YouTube alert's reason is `UploadQueueVm::youtube_error`.
    pub fn paragraphs(self) -> Vec<Msg> {
        match self {
            Alert::Balance => vec![common("Your balance is not enough to run this job. Please top up your minutes and try again.")],
            Alert::Language => vec![common("Please select the audio language before starting transcription.")],
            Alert::ProcessingTimeout => vec![
                common("File preparation took longer than expected. This could be due to file processing issues or upload problems."),
                common("Please try uploading the file again. If the problem persists, contact support."),
            ],
            Alert::YouTube => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    MeasureDuration { file: u64, handle: u64 },
    /// Create the draft with the current language and speaker toggle (UPL-001).
    CreateDraft { file: u64, name: String, size: i64, language: Option<Language>, speakers: bool, duration: Option<i32> },
    /// JOB-005: option writes for the queued drafts.
    SetOptions { jobs: Vec<JobId>, language: Option<Option<Language>>, translate_to: Option<Vec<Language>>, speakers: Option<bool> },
    /// Request the upload URL, then PUT the file under `handle` with progress.
    Upload { file: u64, handle: u64, job: JobId, name: String },
    DeleteDraft { job: JobId },
    /// Start after rewriting the options once more (JOB-005).
    Start { jobs: Vec<JobId>, language: Option<Language>, translate_to: Vec<Language> },
    ResolveYouTube { job: JobId },
    AddYouTube { url: String, language: Option<Language> },
    ReloadDrafts,
    ScheduleReload { after_ms: u64 },
    Navigate { job: JobId },
    /// ANLZ-1-03 product analytics (`transcription_started` / `_failed`).
    Telemetry { event: TelemetryEvent },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UploadQueueState {
    pub files: Vec<FileEntry>,
    next_id: u64,
    pub mode: Mode,
    /// `None` means nothing chosen yet; Start is refused without a choice.
    pub language: Option<Choice>,
    user_set_language: bool,
    pub source_language: Option<Language>,
    pub target_language: Option<Language>,
    pub speakers: bool,
    pub transcribing: bool,
    pub initializing: bool,
    pub preferred: Vec<Language>,
    pub preferred_targets: Vec<Language>,
    pub alerts: Vec<Alert>,
    pub youtube_error: Option<Msg>,
    pub youtube_dialog: YouTubeDialog,
    poll_count: u32,
    polling: bool,
}

impl Default for UploadQueueState {
    fn default() -> Self {
        UploadQueueState {
            files: vec![],
            next_id: 1,
            mode: Mode::Transcription,
            language: None,
            user_set_language: false,
            source_language: None,
            target_language: None,
            speakers: false,
            transcribing: false,
            initializing: true,
            preferred: vec![],
            preferred_targets: vec![],
            alerts: vec![],
            youtube_error: None,
            youtube_dialog: YouTubeDialog::default(),
            poll_count: 0,
            polling: false,
        }
    }
}

/// UPL-027 user messages by resolver code. The source showed these literals
/// untranslated; they are keys of the app catalog.
pub fn youtube_error_message(code: Option<&str>) -> Msg {
    match code {
        Some("no_captions") => app(
            "This video has no captions available. As this feature is still in beta, you can download the video and upload it here to transcribe and analyze it as audio or video.",
        ),
        Some("age_restricted") => app("This video requires age verification and cannot be transcribed"),
        Some("rate_limited") => app("Too many requests, please try again in a few minutes"),
        Some("network_error") => app("Network error fetching captions — please try again"),
        _ => app("Failed to transcribe this YouTube video. Please try again."),
    }
}

pub const NO_YOUTUBE_IN_QUEUE: Msg = app("No YouTube video found in queue");
pub const YOUTUBE_UNEXPECTED: Msg = app("An unexpected error occurred. Please try again.");

/// The add dialog's state; the source kept it in `YouTubeDialog.tsx`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct YouTubeDialog {
    pub open: bool,
    pub url: String,
    pub busy: bool,
    pub error: Option<Msg>,
}

/// The dialog showed the add route's error text (UPL-023), else its own fallback.
pub fn youtube_add_error(failure: &ApiFailure) -> Msg {
    match failure.status {
        Some(400) => app("Invalid YouTube URL"),
        Some(403) => app("Account suspended"),
        Some(404) => app("Video not found or is private"),
        Some(429) => app("Too many requests. Please try again in a minute."),
        Some(502) => app("Failed to fetch video metadata"),
        Some(500) => app("Failed to create transcript job"),
        _ => common("Failed to add video. Please try again."),
    }
}

impl UploadQueueState {
    fn job_ids(&self) -> Vec<JobId> {
        self.files.iter().filter_map(|f| f.job).collect()
    }

    fn file_mut(&mut self, id: u64) -> Option<&mut FileEntry> {
        self.files.iter_mut().find(|f| f.id == id)
    }

    fn add(&mut self, files: Vec<PickedFile>) -> Vec<Effect> {
        let mut effects = Vec::new();
        for f in files {
            let id = self.next_id;
            self.next_id += 1;
            self.files.push(FileEntry {
                id,
                name: f.name,
                size: f.size,
                status: UploadStatus::Uploading { progress: 0 },
                source: Source::File,
                duration: None,
                job: None,
                handle: Some(f.handle),
            });
            effects.push(Effect::MeasureDuration { file: id, handle: f.handle });
        }
        effects
    }

    fn needs_duration(&self) -> bool {
        self.files.iter().any(|f| f.status == UploadStatus::Uploaded && f.duration.is_none())
    }

    fn start_polling(&mut self) -> Vec<Effect> {
        if self.polling || !self.needs_duration() {
            return vec![];
        }
        self.polling = true;
        self.poll_count = 0;
        vec![Effect::ReloadDrafts]
    }

    fn stored_language(&self) -> Option<Language> {
        self.language.and_then(Choice::stored)
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Restored { drafts, preferred, preferred_targets } => {
                self.initializing = false;
                for job in &drafts {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.files.push(FileEntry {
                        id,
                        name: job.name.clone().unwrap_or_else(|| "???".into()),
                        size: job.size_bytes.unwrap_or(0),
                        status: UploadStatus::Uploaded,
                        source: if job.source.is_youtube() { Source::YouTube } else { Source::File },
                        duration: job.duration_minutes,
                        job: Some(job.id),
                        handle: None,
                    });
                }
                self.preferred = preferred;
                self.preferred_targets = preferred_targets.into_iter().filter(|l| l.translation_capable()).collect();
                if !drafts.is_empty() {
                    self.speakers = drafts.iter().all(|j| j.speakers);
                }
                self.start_polling()
            }
            Event::Picked { files, frame } => match frame {
                Frame::Desktop => self.add(files),
                Frame::Phone => self.add(files.into_iter().filter(phone_accepts).collect()),
            },
            Event::Dropped { files } => {
                if self.files.len() >= MAX_FILES {
                    return vec![];
                }
                let valid: Vec<PickedFile> = files.into_iter().filter(|f| accept_dropped(&f.mime, &f.name)).collect();
                self.add(valid)
            }
            Event::Measured { file, seconds } => {
                let duration = seconds.map(minutes_from_seconds);
                let (language, speakers) = (self.stored_language(), self.speakers);
                let Some(entry) = self.file_mut(file) else { return vec![] };
                entry.duration = duration;
                vec![Effect::CreateDraft { file, name: entry.name.clone(), size: entry.size, language, speakers, duration }]
            }
            Event::DraftCreated { file, job } => {
                let job_id = job.id;
                let created_language = job.language;
                let created_speakers = job.speakers;
                let (mode, source, target, user_set, language, speakers) = (
                    self.mode,
                    self.source_language,
                    self.target_language,
                    self.user_set_language,
                    self.stored_language(),
                    self.speakers,
                );
                let Some(entry) = self.file_mut(file) else { return vec![Effect::DeleteDraft { job: job_id }] };
                entry.job = Some(job_id);
                let name = entry.name.clone();
                let Some(handle) = entry.handle else {
                    // Only a picked file is measured into a draft; without its handle there is nothing to upload.
                    entry.status = UploadStatus::Failed;
                    return vec![];
                };
                let mut effects = Vec::new();
                // Options changed while the draft was being created are written again.
                match mode {
                    Mode::Translation => {
                        if source != created_language {
                            effects.push(Effect::SetOptions { jobs: vec![job_id], language: Some(source), translate_to: None, speakers: None });
                        }
                        if let Some(t) = target {
                            effects.push(Effect::SetOptions { jobs: vec![job_id], language: None, translate_to: Some(vec![t]), speakers: None });
                        }
                    }
                    Mode::Transcription => {
                        if user_set && language != created_language {
                            effects.push(Effect::SetOptions { jobs: vec![job_id], language: Some(language), translate_to: None, speakers: None });
                        }
                    }
                }
                if speakers != created_speakers {
                    effects.push(Effect::SetOptions { jobs: vec![job_id], language: None, translate_to: None, speakers: Some(speakers) });
                }
                effects.push(Effect::Upload { file, handle, job: job_id, name });
                effects
            }
            Event::DraftFailed { file } => {
                if let Some(entry) = self.file_mut(file) {
                    entry.status = UploadStatus::Failed;
                }
                vec![]
            }
            Event::UploadProgress { file, progress } => {
                if let Some(entry) = self.file_mut(file) {
                    entry.status = UploadStatus::Uploading { progress: progress.min(100) };
                }
                vec![]
            }
            Event::UploadFinished { file, ok } => {
                if let Some(entry) = self.file_mut(file) {
                    entry.status = if ok { UploadStatus::Uploaded } else { UploadStatus::Failed };
                }
                self.start_polling()
            }
            Event::Removed { file } => {
                let job = self.files.iter().find(|f| f.id == file).and_then(|f| f.job);
                self.files.retain(|f| f.id != file);
                job.map(|job| vec![Effect::DeleteDraft { job }]).unwrap_or_default()
            }
            Event::LanguageChosen { choice } => {
                self.user_set_language = true;
                self.language = choice;
                let jobs = self.job_ids();
                if jobs.is_empty() {
                    return vec![];
                }
                vec![Effect::SetOptions { jobs, language: Some(choice.and_then(Choice::stored)), translate_to: None, speakers: None }]
            }
            Event::SpeakersToggled { enabled } => {
                self.speakers = enabled;
                let jobs = self.job_ids();
                if jobs.is_empty() {
                    return vec![];
                }
                vec![Effect::SetOptions { jobs, language: None, translate_to: None, speakers: Some(enabled) }]
            }
            Event::TabChanged { mode, frame } => {
                self.mode = mode;
                // `ConfigurationSection.tsx:65-73`: the phone defaults the target to Spanish
                // when switching to translation and clears it when switching back.
                if frame == Frame::Phone {
                    self.target_language = match mode {
                        Mode::Translation => self.target_language.or(Language::from_name("Spanish")),
                        Mode::Transcription => None,
                    };
                }
                let jobs = self.job_ids();
                if jobs.is_empty() {
                    return vec![];
                }
                // UPL-019: translation writes the target (or NULL); transcription clears it.
                let targets = match mode {
                    Mode::Translation => self.target_language.map(|t| vec![t]).unwrap_or_default(),
                    Mode::Transcription => vec![],
                };
                vec![Effect::SetOptions { jobs, language: None, translate_to: Some(targets), speakers: None }]
            }
            Event::SourceLanguageChosen { language } => {
                self.user_set_language = true;
                self.source_language = language;
                let jobs = self.job_ids();
                let mut effects = Vec::new();
                // UPL-018: the same language on both sides clears the other.
                if language.is_some() && self.target_language == language {
                    self.target_language = None;
                    if !jobs.is_empty() {
                        effects.push(Effect::SetOptions { jobs: jobs.clone(), language: None, translate_to: Some(vec![]), speakers: None });
                    }
                }
                if !jobs.is_empty() {
                    effects.push(Effect::SetOptions { jobs, language: Some(language), translate_to: None, speakers: None });
                }
                effects
            }
            Event::TargetLanguageChosen { language } => {
                self.user_set_language = true;
                self.target_language = language;
                let jobs = self.job_ids();
                let mut effects = Vec::new();
                if language.is_some() && self.source_language == language {
                    self.source_language = None;
                    if !jobs.is_empty() {
                        effects.push(Effect::SetOptions { jobs: jobs.clone(), language: Some(None), translate_to: None, speakers: None });
                    }
                }
                if !jobs.is_empty() {
                    effects.push(Effect::SetOptions { jobs, language: None, translate_to: Some(language.map(|l| vec![l]).unwrap_or_default()), speakers: None });
                }
                effects
            }
            Event::DraftsReloaded { drafts, elapsed_ms } => {
                for job in drafts {
                    if let Some(entry) = self.files.iter_mut().find(|f| f.job == Some(job.id)) && entry.duration.is_none() {
                        entry.duration = job.duration_minutes;
                    }
                }
                if !self.needs_duration() {
                    self.polling = false;
                    self.poll_count = 0;
                    return vec![];
                }
                if elapsed_ms >= POLL_TIMEOUT_MS {
                    self.polling = false;
                    if !self.alerts.contains(&Alert::ProcessingTimeout) {
                        self.alerts.push(Alert::ProcessingTimeout);
                    }
                    return vec![];
                }
                self.poll_count += 1;
                vec![Effect::ScheduleReload { after_ms: poll_interval_ms(self.poll_count) }]
            }
            Event::StartPressed => {
                if self.start_disabled() {
                    return vec![];
                }
                // UPL-028: a queue with any YouTube file only resolves YouTube.
                if self.files.iter().any(|f| f.source == Source::YouTube) {
                    let job = self.files.iter().find(|f| f.source == Source::YouTube).and_then(|f| f.job);
                    let Some(job) = job else {
                        self.youtube_error = Some(NO_YOUTUBE_IN_QUEUE);
                        return vec![];
                    };
                    self.transcribing = true;
                    self.youtube_error = None;
                    let mut effects = self.telemetry("transcription_started", None);
                    effects.push(Effect::ResolveYouTube { job });
                    return effects;
                }
                let (language, targets) = match self.mode {
                    Mode::Translation => match (self.source_language, self.target_language) {
                        (Some(s), Some(t)) => (Some(s), vec![t]),
                        _ => {
                            self.alerts.push(Alert::Language);
                            return vec![];
                        }
                    },
                    Mode::Transcription => match self.language {
                        Some(choice) => (choice.stored(), vec![]),
                        None => {
                            self.alerts.push(Alert::Language);
                            return vec![];
                        }
                    },
                };
                self.transcribing = true;
                let mut effects = self.telemetry("transcription_started", None);
                effects.push(Effect::Start { jobs: self.job_ids(), language, translate_to: targets });
                effects
            }
            Event::StartSucceeded { first_job } => vec![Effect::Navigate { job: first_job }],
            Event::StartFailed { balance } => {
                self.transcribing = false;
                if balance {
                    self.alerts.push(Alert::Balance);
                }
                self.telemetry("transcription_failed", Some(if balance { "insufficient_balance" } else { "start_failed" }))
            }
            Event::YouTubeDialogOpened => {
                self.youtube_dialog = YouTubeDialog { open: true, ..YouTubeDialog::default() };
                vec![]
            }
            Event::YouTubeDialogClosed if !self.youtube_dialog.busy => {
                self.youtube_dialog = YouTubeDialog::default();
                vec![]
            }
            Event::YouTubeDialogClosed => vec![],
            Event::YouTubeUrlChanged { url } => {
                self.youtube_dialog.url = url;
                self.youtube_dialog.error = None;
                vec![]
            }
            Event::YouTubeSubmitted if self.youtube_dialog.open && !self.youtube_dialog.busy => {
                let url = self.youtube_dialog.url.trim().to_string();
                if url.is_empty() {
                    self.youtube_dialog.error = Some(common("Please paste a YouTube URL"));
                    return vec![];
                }
                if crate::sources::youtube_video_id(&url).is_none() {
                    self.youtube_dialog.error = Some(common("Please enter a valid YouTube URL (e.g. https://youtube.com/watch?v=...)"));
                    return vec![];
                }
                self.youtube_dialog.busy = true;
                self.youtube_dialog.error = None;
                vec![Effect::AddYouTube { url, language: self.stored_language() }]
            }
            Event::YouTubeSubmitted => vec![],
            Event::YouTubeAddFailed { failure } => {
                self.youtube_dialog.busy = false;
                self.youtube_dialog.error = Some(youtube_add_error(&failure));
                vec![]
            }
            Event::YouTubeAdded { job, title, duration_seconds, detected_language } => {
                self.youtube_dialog = YouTubeDialog::default();
                let id = self.next_id;
                self.next_id += 1;
                self.files.push(FileEntry {
                    id,
                    name: if title.is_empty() { job.name.clone().unwrap_or_else(|| "YouTube Video".into()) } else { title },
                    size: 0,
                    status: UploadStatus::Uploaded,
                    source: Source::YouTube,
                    duration: (duration_seconds > 0.0).then(|| crate::sources::youtube_display_minutes(duration_seconds) as i32),
                    job: Some(job.id),
                    handle: None,
                });
                if self.language.is_none() && let Some(l) = detected_language.as_deref().and_then(|d| Language::from_code(d).or_else(|| Language::from_name(d))) {
                    self.language = Some(Choice::Language(l));
                }
                vec![]
            }
            Event::YouTubeResolved { job, completed, code } => {
                self.transcribing = false;
                if completed {
                    return vec![Effect::Navigate { job }];
                }
                self.youtube_error = Some(youtube_error_message(code.as_deref()));
                self.files.retain(|f| f.job != Some(job));
                vec![Effect::DeleteDraft { job }]
            }
            Event::YouTubeResolveCrashed => {
                self.transcribing = false;
                self.youtube_error = Some(YOUTUBE_UNEXPECTED);
                let job = self.files.iter().find(|f| f.source == Source::YouTube).and_then(|f| f.job);
                self.files.retain(|f| f.source != Source::YouTube);
                job.map(|job| vec![Effect::DeleteDraft { job }]).unwrap_or_default()
            }
            Event::DismissAlert { alert } => {
                self.alerts.retain(|a| *a != alert);
                if alert == Alert::YouTube {
                    self.youtube_error = None;
                }
                vec![]
            }
        }
    }

    /// UPL-007: every file uploaded with a duration (the phone "Preparing files..." state when false).
    pub fn files_ready(&self) -> bool {
        !self.files.is_empty() && self.files.iter().all(|f| f.status == UploadStatus::Uploaded && f.duration.is_some())
    }

    /// UPL-007: disabled until every file is ready, and while starting.
    pub fn start_disabled(&self) -> bool {
        !self.files_ready() || self.transcribing
    }

    /// ANLZ-1-03: one event per queued file, as the containers sent them
    /// (provider always reported as `deepgram`). `error_type` is the start
    /// failure's kind; the source sent the thrown error's class name.
    fn telemetry(&self, name: &'static str, error_type: Option<&'static str>) -> Vec<Effect> {
        let language = self.stored_language().map_or("unknown", |l| l.name());
        self.files
            .iter()
            .map(|f| {
                let mut event = TelemetryEvent::new(name).text("provider", "deepgram").text("language", language);
                if let Some(job) = f.job {
                    event = event.number("job_id", job.0 as f64);
                }
                event = match error_type {
                    Some(kind) => event.text("error_type", kind),
                    None => {
                        let event = event.flag("has_speaker_recognition", self.speakers);
                        let event = match f.name.rsplit_once('.') {
                            Some((_, ext)) => event.text("file_format", ext),
                            None => event,
                        };
                        if f.size > 0 { event.number("file_size_mb", f.size as f64 / (1024.0 * 1024.0)) } else { event }
                    }
                };
                Effect::Telemetry { event }
            })
            .collect()
    }

    /// `frame` picks the layout-dependent facts (the picker accept list).
    pub fn view(&self, frame: Frame) -> UploadQueueVm {
        let translation = self.mode == Mode::Translation;
        let in_mode = |l: &Language| !translation || l.translation_capable();
        let preferred: Vec<Language> = self.preferred.iter().copied().filter(in_mode).collect();
        let available: Vec<Language> = Language::all().filter(|l| !self.preferred.contains(l)).filter(in_mode).collect();
        let source_list: Vec<Language> = Language::all().filter(in_mode).collect();
        let target_list: Vec<Language> = Language::all().filter(|l| Some(*l) != self.source_language).filter(in_mode).collect();
        let preferred_targets: Vec<Language> =
            self.preferred_targets.iter().copied().filter(in_mode).filter(|l| Some(*l) != self.source_language).collect();
        UploadQueueVm {
            rows: self.files.iter().map(row).collect(),
            mode: self.mode,
            language: self.language,
            source_language: self.source_language,
            target_language: self.target_language,
            speakers: self.speakers,
            // LNG-008/011: auto-detect pinned first, transcription mode only.
            auto_detect_offered: !translation,
            auto_detect_label: common("Auto-detect"),
            preferred: options(&preferred),
            available: options(&available),
            source_languages: options(&source_list),
            target_languages: options(&target_list),
            preferred_targets: options(&preferred_targets),
            phone_preferred: options(&PHONE_PREFERRED.iter().filter_map(|n| Language::from_name(n)).filter(in_mode).collect::<Vec<_>>()),
            start_disabled: self.start_disabled(),
            files_ready: self.files_ready(),
            transcribing: self.transcribing,
            max_files_reached: self.files.len() >= MAX_FILES,
            picker_accept: match frame {
                Frame::Desktop => DESKTOP_PICKER_ACCEPT,
                Frame::Phone => PHONE_PICKER_ACCEPT,
            }
            .to_string(),
            show_language_selector: !self.files.is_empty(),
            // Phone "Calculating..." while any file is unmeasured.
            total_minutes: self.files.iter().map(|f| f.duration.map(i64::from)).sum(),
            alerts: self.alerts.clone(),
            youtube_error: self.youtube_error.clone(),
            youtube_dialog: self.youtube_dialog.clone(),
            initializing: self.initializing,
        }
    }
}

/// The chip under each file (jobs-transcription 2.1 `draft` row).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "chip", rename_all = "snake_case")]
pub enum Chip {
    Uploading {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        progress: u8,
    },
    Preparing,
    Ready,
    UploadFailed,
}

impl Chip {
    pub fn label(&self) -> Msg {
        match self {
            Chip::Uploading { .. } => common("Uploading"),
            Chip::Preparing => common("Preparing"),
            Chip::Ready => common("Ready"),
            Chip::UploadFailed => common("Upload Failed"),
        }
    }
}

/// A picker entry: the stored English name, as the source pickers show it, and its flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LanguageOption {
    pub language: Language,
    pub name: String,
    pub flag: String,
}

impl LanguageOption {
    pub fn of(language: Language) -> LanguageOption {
        let flag = language.flag();
        LanguageOption { language, name: language.name().to_string(), flag: if flag.is_empty() { "🌐".into() } else { flag.to_string() } }
    }
}

fn options(languages: &[Language]) -> Vec<LanguageOption> {
    languages.iter().copied().map(LanguageOption::of).collect()
}

/// `languageHelpers.ts:26-33`: the phone's fixed preferred languages.
pub const PHONE_PREFERRED: [&str; 4] = ["English", "Turkish", "German", "Indonesian"];

/// Desktop `formatBytes`: binary units, two decimals.
pub fn desktop_size(bytes: i64) -> String {
    let units = ["Bytes", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", units[unit])
}

/// Phone `formatSize`: B, KB with one decimal, else MB with one decimal.
pub fn phone_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileRow {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: i64,
    pub desktop_size: String,
    pub phone_size: String,
    pub chip: Chip,
    pub label: Msg,
    /// The phone item's status text (`Uploading...`, `Preparing...`, `Ready ✓`, `Failed`).
    pub phone_label: Msg,
    /// 0 failed, the percentage while uploading, 100 uploaded.
    pub progress: u8,
    pub duration: Option<i32>,
    /// `{n} min` when measured.
    pub duration_text: Option<Msg>,
    pub youtube: bool,
}

fn row(f: &FileEntry) -> FileRow {
    let chip = match f.status {
        UploadStatus::Uploading { progress } => Chip::Uploading { progress },
        UploadStatus::Failed => Chip::UploadFailed,
        UploadStatus::Uploaded if f.duration.is_none() => Chip::Preparing,
        UploadStatus::Uploaded => Chip::Ready,
    };
    let phone_label = common(match chip {
        Chip::Uploading { .. } => "Uploading...",
        Chip::Preparing => "Preparing...",
        Chip::Ready => "Ready ✓",
        Chip::UploadFailed => "Failed",
    });
    let progress = match f.status {
        UploadStatus::Uploading { progress } => progress,
        UploadStatus::Failed => 0,
        UploadStatus::Uploaded => 100,
    };
    FileRow {
        id: f.id,
        name: f.name.clone(),
        size: f.size,
        desktop_size: desktop_size(f.size),
        phone_size: phone_size(f.size),
        label: chip.label(),
        phone_label,
        chip,
        progress,
        duration: f.duration,
        duration_text: f.duration.map(|m| app("p5.duration.minutes").number("minutes", i64::from(m))),
        youtube: f.source == Source::YouTube,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UploadQueueVm {
    pub rows: Vec<FileRow>,
    pub mode: Mode,
    pub language: Option<Choice>,
    pub source_language: Option<Language>,
    pub target_language: Option<Language>,
    pub speakers: bool,
    pub auto_detect_offered: bool,
    /// The translated auto-detect entry (the only translated picker name).
    pub auto_detect_label: Msg,
    pub preferred: Vec<LanguageOption>,
    pub available: Vec<LanguageOption>,
    pub source_languages: Vec<LanguageOption>,
    pub target_languages: Vec<LanguageOption>,
    pub preferred_targets: Vec<LanguageOption>,
    /// The phone sheet's fixed preferred languages.
    pub phone_preferred: Vec<LanguageOption>,
    pub start_disabled: bool,
    /// Every file uploaded and measured.
    pub files_ready: bool,
    pub transcribing: bool,
    pub max_files_reached: bool,
    /// UPL-002: the file input's `accept` for the current layout.
    pub picker_accept: String,
    pub show_language_selector: bool,
    /// `None` while any file's duration is unknown.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub total_minutes: Option<i64>,
    pub alerts: Vec<Alert>,
    pub youtube_error: Option<Msg>,
    pub youtube_dialog: YouTubeDialog,
    pub initializing: bool,
}

#[cfg(test)]
mod tests;
