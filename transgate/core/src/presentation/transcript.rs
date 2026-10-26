//! The transcript page (EDT-001..014, EXP-001/012..016, TRX-042, LNG-007,
//! JOB-F09..F14), ported from `useTranscriptLinesVM.ts`, the header models and
//! `LoadingState.tsx`.

use serde::{Deserialize, Serialize};

use crate::export::{self, ExportLabels};
use crate::ids::{JobId, LineId};
use crate::jobs::app::{ExportKind, ExportRequest, ExportSurface};
use crate::jobs::domain::{Job, JobSource, JobStatus};
use crate::languages::{Language, is_rtl};
use crate::presentation::failure::ApiFailure;
use crate::presentation::msg::{Msg, app, common};
use crate::transcript::{
    LanguageTab, Line, LineEdit, SpeakerLabel, TranslationStatus, display_speakers, distinct_speakers, language_tabs,
    merge_lines, speaker_label, time::hms_seconds,
};

/// Poll the job every two seconds while it runs (the source's realtime fallback).
pub const RUNNING_POLL_MS: u64 = 2000;
/// Lines load two seconds after completion.
pub const COMPLETION_DELAY_MS: u64 = 2000;
/// UPL-016: a signed audio URL lives 900 s; the source reused one for 14
/// minutes and then asked for a fresh one (`WavesurferAudioService.ts`).
pub const MEDIA_URL_REUSE_MS: u64 = 14 * 60 * 1000;
/// The "Audio is not available." alert hides itself after five seconds.
pub const NO_AUDIO_ALERT_MS: u64 = 5000;
pub const NO_AUDIO: Msg = app("Audio is not available.");

/// Loading copy by status (`LoadingState.tsx`).
pub fn loading_message(status: JobStatus, translation: bool) -> Option<Msg> {
    let key = match (status, translation) {
        (JobStatus::NotStarted, false) => "Waiting to start transcription...",
        (JobStatus::NotStarted, true) => "Waiting to start translation...",
        (JobStatus::Pending, false) => "Preparing transcription...",
        (JobStatus::Pending, true) => "Preparing translation...",
        (JobStatus::Running, false) => "Transcribing audio...",
        (JobStatus::Running, true) => "Translating audio...",
        _ => return None,
    };
    Some(app(key))
}

/// What the player plays: the job's uploaded audio through a signed URL, or
/// the YouTube video in the iframe player.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Media {
    #[default]
    None,
    Audio { url: Option<String>, loading: bool, unavailable: bool },
    YouTube { video_id: String },
}

fn is_streaming(status: JobStatus) -> bool {
    matches!(status, JobStatus::Running | JobStatus::SpeakerLabeling)
}

/// EDT-010: YouTube caption transcripts offer no speaker handling.
fn supports_speakers(job: &Job) -> bool {
    !matches!(&job.source, JobSource::YouTube { transcript_source: Some(s), .. } if s == "transcriptapi")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The job (and, once loaded, its lines). `show_original` is the
    /// `SHOW_ORIGINAL_TRANSCRIPT` flag for this user; `is_freemium` app definition 1.
    Loaded { job: Job, lines: Vec<Line>, show_original: bool, is_freemium: bool },
    JobPolled { job: Job },
    LinesArrived { lines: Vec<Line> },
    /// The lines request failed ("Error loading transcripts:" + detail).
    LinesFailed { detail: String },
    TabSelected { tab: LanguageTab },
    EditStarted { line: LineId },
    EditCanceled,
    EditSubmitted { text: String },
    SpeakerChosen { line: LineId, speaker: String },
    SpeakerRenamed { from: String, to: String },
    ShowTimeToggled { on: bool },
    ShowSpeakersToggled { on: bool },
    TimeClicked { line: LineId },
    DownloadPressed { all_formats: bool },
    CopyPressed,
    /// The player reported whether it was playing when editing began.
    PlayerState { playing: bool },
    /// UPL-016: the signed URL answer for the job it was asked for.
    MediaUrlLoaded { job: JobId, result: Result<String, ApiFailure> },
    /// The reuse window of the current URL ended.
    MediaUrlDue { job: JobId },
    NoAudioAlertExpired,
    NoAudioAlertDismissed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    SaveEdit { edit: LineEdit },
    PollJob { after_ms: u64 },
    LoadLines { after_ms: u64 },
    /// EDT-001: pause while editing, resume afterwards if it was playing.
    PausePlayer,
    ResumePlayer,
    /// EDT-014
    Seek { seconds: f64 },
    /// `POST /api/jobs/{id}/export`: the server builds the ZIP and records
    /// `transcript_downloaded` (EXP-016); the shell saves the bytes.
    Export { job: JobId, request: ExportRequest },
    CopyText { text: String },
    /// `POST /api/streaming-url {jobId, fileName}`.
    LoadMediaUrl { job: JobId, file_name: String },
    /// Answer with `MediaUrlDue` after the delay.
    ScheduleMediaUrl { job: JobId, after_ms: u64 },
    /// Answer with `NoAudioAlertExpired` after the delay.
    ScheduleNoAudioAlert { after_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptState {
    pub job: Option<Job>,
    pub lines: Vec<Line>,
    pub selected: Option<LanguageTab>,
    pub editing: Option<LineId>,
    pub show_original: bool,
    pub is_freemium: bool,
    pub show_time: bool,
    pub show_speakers: bool,
    was_playing: bool,
    pub labels: ExportLabels,
    pub media: Media,
    no_audio_alert: bool,
    load_error: Option<String>,
}

impl Default for TranscriptState {
    fn default() -> Self {
        TranscriptState {
            job: None,
            lines: vec![],
            selected: None,
            editing: None,
            show_original: false,
            is_freemium: false,
            show_time: true,
            show_speakers: false,
            was_playing: false,
            labels: ExportLabels::default(),
            media: Media::None,
            no_audio_alert: false,
            load_error: None,
        }
    }
}

impl TranscriptState {
    fn translation(&self) -> bool {
        self.job.as_ref().is_some_and(Job::is_translation)
    }

    fn select_default(&mut self) {
        let tabs = language_tabs(&self.lines);
        // Prefer the translation tab, else the original (EDT-012).
        self.selected = tabs
            .iter()
            .find(|t| t.translation_status == TranslationStatus::Translation)
            .or_else(|| tabs.iter().find(|t| t.translation_status == TranslationStatus::Original))
            .or(tabs.first())
            .cloned();
    }

    fn follow_up(&self) -> Vec<Effect> {
        match self.job.as_ref().map(|j| j.status) {
            Some(s) if is_streaming(s) || s.is_in_progress() => vec![Effect::PollJob { after_ms: RUNNING_POLL_MS }],
            _ => vec![],
        }
    }

    /// The player source for a newly shown job: the YouTube video, or a
    /// signed URL request for the uploaded file (`job_name`; none without it).
    fn load_media(&mut self, job: &Job) -> Vec<Effect> {
        self.no_audio_alert = false;
        match (&job.source, &job.name) {
            (JobSource::YouTube { video_id: Some(id), .. }, _) => {
                self.media = Media::YouTube { video_id: id.clone() };
                vec![]
            }
            (JobSource::YouTube { .. }, _) | (_, None) => {
                self.media = Media::None;
                vec![]
            }
            (_, Some(name)) => {
                self.media = Media::Audio { url: None, loading: true, unavailable: false };
                vec![Effect::LoadMediaUrl { job: job.id, file_name: name.clone() }]
            }
        }
    }

    fn current_job(&self) -> Option<JobId> {
        self.job.as_ref().map(|j| j.id)
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Loaded { job, lines, show_original, is_freemium } => {
                let same_job = self.current_job() == Some(job.id);
                let mut effects = if same_job { vec![] } else { self.load_media(&job) };
                self.load_error = None;
                self.show_speakers = job.speakers;
                self.show_time = !job.is_translation();
                self.job = Some(job);
                self.show_original = show_original;
                self.is_freemium = is_freemium;
                self.lines = vec![];
                merge_lines(&mut self.lines, lines);
                self.select_default();
                effects.extend(self.follow_up());
                effects
            }
            Event::JobPolled { job } => {
                let was = self.job.as_ref().map(|j| j.status);
                let now_completed = job.status == JobStatus::Completed;
                self.job = Some(job);
                if now_completed && was != Some(JobStatus::Completed) {
                    return vec![Effect::LoadLines { after_ms: COMPLETION_DELAY_MS }];
                }
                self.follow_up()
            }
            Event::LinesArrived { lines } => {
                self.load_error = None;
                merge_lines(&mut self.lines, lines);
                if self.selected.is_none() {
                    self.select_default();
                }
                vec![]
            }
            Event::TabSelected { tab } => {
                self.selected = Some(tab);
                vec![]
            }
            Event::EditStarted { line } => {
                self.editing = Some(line);
                vec![Effect::PausePlayer]
            }
            Event::PlayerState { playing } => {
                self.was_playing = playing;
                vec![]
            }
            Event::EditCanceled => {
                self.editing = None;
                if std::mem::take(&mut self.was_playing) { vec![Effect::ResumePlayer] } else { vec![] }
            }
            Event::EditSubmitted { text } => {
                let Some(line) = self.editing.take() else { return vec![] };
                // EDT-002: write only `line`, update the screen, empty is allowed.
                if let Some(l) = self.lines.iter_mut().find(|l| l.id == line) {
                    l.line = text.clone();
                }
                let mut effects = vec![Effect::SaveEdit { edit: LineEdit::Text { line, text } }];
                if std::mem::take(&mut self.was_playing) {
                    effects.push(Effect::ResumePlayer);
                }
                effects
            }
            Event::SpeakerChosen { line, speaker } => {
                if let Some(l) = self.lines.iter_mut().find(|l| l.id == line) {
                    l.speaker = Some(speaker.clone());
                }
                vec![Effect::SaveEdit { edit: LineEdit::Speaker { line, speaker } }]
            }
            Event::SpeakerRenamed { from, to } => {
                let Some(job) = self.job.as_ref().map(|j| j.id) else { return vec![] };
                for l in self.lines.iter_mut().filter(|l| l.speaker.as_deref() == Some(from.as_str())) {
                    l.speaker = Some(to.clone());
                }
                vec![Effect::SaveEdit { edit: LineEdit::RenameSpeaker { job, from, to } }]
            }
            Event::ShowTimeToggled { on } => {
                self.show_time = on && !self.translation();
                vec![]
            }
            Event::ShowSpeakersToggled { on } => {
                self.show_speakers = on && self.job.as_ref().is_some_and(|j| j.speakers);
                vec![]
            }
            Event::TimeClicked { line } => self
                .lines
                .iter()
                .find(|l| l.id == line)
                .map(|l| vec![Effect::Seek { seconds: hms_seconds(&l.start_time) }])
                .unwrap_or_default(),
            Event::DownloadPressed { all_formats } => {
                let Some(job) = &self.job else { return vec![] };
                // EXP-001: only completed jobs; FRE-022 download is never cut. EXP-013's
                // translation filter is the server's `TranscriptPage` surface.
                if job.status != JobStatus::Completed {
                    return vec![];
                }
                let kind = if all_formats {
                    ExportKind::AllFormats
                } else {
                    ExportKind::Wysiwyg { show_timestamps: Some(self.show_time), show_speakers: Some(self.show_speakers) }
                };
                vec![Effect::Export { job: job.id, request: ExportRequest { kind, surface: ExportSurface::TranscriptPage, labels: self.labels.clone() } }]
            }
            Event::LinesFailed { detail } => {
                self.load_error = Some(detail);
                vec![]
            }
            Event::CopyPressed => vec![Effect::CopyText { text: export::clipboard_text(&self.visible_lines(), &self.labels) }],
            Event::MediaUrlLoaded { job, result } => {
                if self.current_job() != Some(job) || !matches!(self.media, Media::Audio { .. }) {
                    return vec![];
                }
                match result {
                    Ok(url) => {
                        self.media = Media::Audio { url: Some(url), loading: false, unavailable: false };
                        vec![Effect::ScheduleMediaUrl { job, after_ms: MEDIA_URL_REUSE_MS }]
                    }
                    Err(_) => {
                        self.media = Media::Audio { url: None, loading: false, unavailable: true };
                        self.no_audio_alert = true;
                        vec![Effect::ScheduleNoAudioAlert { after_ms: NO_AUDIO_ALERT_MS }]
                    }
                }
            }
            Event::MediaUrlDue { job } => match (&self.job, &self.media) {
                (Some(current), Media::Audio { url: Some(_), .. }) if current.id == job => match &current.name {
                    Some(name) => vec![Effect::LoadMediaUrl { job, file_name: name.clone() }],
                    None => vec![],
                },
                _ => vec![],
            },
            Event::NoAudioAlertExpired | Event::NoAudioAlertDismissed => {
                self.no_audio_alert = false;
                vec![]
            }
        }
    }

    /// EDT-012 filtering: originals hidden for translation jobs unless the flag
    /// applies; streaming shows everything left; otherwise the selected tab.
    fn visible_lines(&self) -> Vec<Line> {
        let Some(job) = &self.job else { return vec![] };
        let translation = job.is_translation();
        let mut lines: Vec<Line> = if !self.show_original && translation {
            self.lines.iter().filter(|l| l.translation_status == Some(TranslationStatus::Translation)).cloned().collect()
        } else {
            self.lines.clone()
        };
        if is_streaming(job.status) || !translation {
            return lines;
        }
        if let Some(tab) = &self.selected {
            lines.retain(|l| l.language == Some(tab.language) && l.translation_status == Some(tab.translation_status));
        }
        lines
    }

    pub fn view(&self) -> TranscriptVm {
        let Some(job) = &self.job else { return TranscriptVm::default() };
        let translation = job.is_translation();
        let lines = self.visible_lines();
        let speakers_shown = self.show_speakers && supports_speakers(job);
        let hide_labels = matches!(job.status, JobStatus::NotStarted | JobStatus::Pending | JobStatus::Running);
        let fallback = display_speakers(&lines);
        let line_starts: Vec<f64> = lines.iter().map(|l| hms_seconds(&l.start_time)).collect();
        let minutes = i64::from(job.duration_minutes.unwrap_or(0));
        let cut = billing_core::freemium::display_cut(self.is_freemium, minutes, &line_starts);
        let rows = lines
            .iter()
            .zip(fallback)
            .take(cut.visible)
            .map(|(l, speaker)| LineRow {
                id: l.id,
                start_time: l.start_time.clone(),
                end_time: l.end_time.clone(),
                time_text: time_text(&l.start_time, &l.end_time),
                start_seconds: hms_seconds(&l.start_time),
                end_seconds: hms_seconds(&l.end_time),
                text: l.line.clone(),
                speaker: if !speakers_shown || hide_labels { None } else { Some(speaker_label(speaker.as_deref())) },
                speaker_value: l.speaker.clone(),
                speaker_pending: job.status == JobStatus::SpeakerLabeling && l.speaker.is_none(),
                editing: self.editing == Some(l.id),
            })
            .collect::<Vec<_>>();
        let mut tabs = language_tabs(&self.lines);
        if !self.show_original && translation {
            tabs.retain(|t| t.translation_status == TranslationStatus::Translation);
        }
        let rtl = if is_streaming(job.status) {
            is_rtl(job.language.map(Language::name))
        } else if let (Some(tab), true) = (&self.selected, translation) {
            tab.language.is_rtl()
        } else {
            is_rtl(job.language.map(Language::name))
        };
        let translation_flag = job.translate_to.first().map(|l| l.flag().to_string());
        // `Transcript.tsx:670-680`: the lines tab reads Translation with a flag for a
        // translated view, else Transcript.
        let selected_translation = !self.show_original && self.selected.as_ref().is_some_and(|t| t.translation_status == TranslationStatus::Translation);
        let lines_tab_flag = if selected_translation {
            self.selected.as_ref().map(|t| t.language.flag().to_string())
        } else if translation {
            translation_flag.clone()
        } else {
            None
        };
        TranscriptVm {
            job: Some(job.id),
            title: job.name.clone().unwrap_or_default(),
            duration_minutes: job.duration_minutes,
            minutes_unit: common("min"),
            status: Some(job.status),
            status_badge: Some(StatusBadge { label: common(job.status.as_str()), status: job.status, spinning: badge_spins(job.status) }),
            is_translation: translation,
            translation_flag,
            loading_message: loading_message(job.status, translation),
            empty: (self.lines.is_empty() && !hide_labels).then_some(NO_LINES),
            load_error: self.load_error.as_ref().map(|_| LOAD_ERROR),
            load_error_detail: self.load_error.clone(),
            can_copy: !self.lines.is_empty(),
            rows,
            fades: cut.fades.clone(),
            upgrade_banner: cut.show_upgrade_banner,
            freemium: cut.show_upgrade_banner.then(|| freemium_banner(minutes)),
            show_tabs: translation,
            tabs: tabs.into_iter().map(|tab| TabOption { name: tab.language.name().to_string(), flag: tab.language.flag().to_string(), tab }).collect(),
            selected: self.selected.clone(),
            lines_tab_label: if lines_tab_flag.is_some() { app("Translation") } else { app("Transcript") },
            lines_tab_flag,
            rtl,
            speakers: distinct_speakers(&self.lines),
            speaker_options: distinct_speakers(&self.lines).into_iter().map(|value| SpeakerOption { label: speaker_label(Some(value.as_str())), value }).collect(),
            time_toggle: !translation,
            speaker_toggle: job.speakers && supports_speakers(job),
            phone_speaker_toggle: job.speakers,
            show_time: self.show_time,
            show_speakers: self.show_speakers,
            download_enabled: job.status == JobStatus::Completed,
            media: self.media.clone(),
            no_audio_alert: self.no_audio_alert.then_some(NO_AUDIO),
        }
    }
}

pub const NO_LINES: Msg = common("No transcript lines available");
pub const LOAD_ERROR: Msg = common("Error loading transcripts:");

/// `TranscriptLineView.tsx:140`: each time up to its first comma.
pub fn time_text(start: &str, end: &str) -> String {
    let cut = |t: &str| t.split(',').next().unwrap_or_default().to_string();
    format!("{} - {}", cut(start), cut(end))
}

/// `statusConfig.tsx`: the statuses whose badge icon spins.
pub fn badge_spins(status: JobStatus) -> bool {
    matches!(status, JobStatus::Processing | JobStatus::Running | JobStatus::SpeakerLabeling | JobStatus::Pending)
}

/// `FreemiumLimitsBanner`: the job's minutes over the free limit, the bar capped at 100 %.
pub fn freemium_banner(current_minutes: i64) -> FreemiumBanner {
    let max = billing_core::freemium::FREE_LIMIT_MINUTES;
    let percent = (current_minutes.max(0) * 100 / max).min(100) as u8;
    FreemiumBanner { current_minutes, max_minutes: max, percent }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StatusBadge {
    /// `t(job_status)`.
    pub label: Msg,
    /// The shell maps each status to its badge hue (`statusConfig.tsx`).
    pub status: JobStatus,
    pub spinning: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FreemiumBanner {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub current_minutes: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_minutes: i64,
    pub percent: u8,
}

/// A language tab with the source's flag and raw English name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TabOption {
    pub tab: LanguageTab,
    pub name: String,
    pub flag: String,
}

/// One entry of the speaker dropdown: the raw stored value and its label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpeakerOption {
    pub value: String,
    pub label: SpeakerLabel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LineRow {
    pub id: LineId,
    pub start_time: String,
    pub end_time: String,
    /// `start - end`, each up to its first comma.
    pub time_text: String,
    /// The same times in seconds, for the player's active-line pick and seek.
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    /// `None` hides the label (EDT-008/010); otherwise EDT-004's label.
    pub speaker: Option<SpeakerLabel>,
    /// The stored value: the dropdown's check and the rename source.
    pub speaker_value: Option<String>,
    /// EDT-008: a pulsing placeholder while speakers are being labeled.
    pub speaker_pending: bool,
    pub editing: bool,
}

impl Default for TranscriptVm {
    /// No job shown.
    fn default() -> TranscriptVm {
        TranscriptVm {
            job: None,
            title: String::new(),
            duration_minutes: None,
            minutes_unit: common("min"),
            status: None,
            status_badge: None,
            is_translation: false,
            translation_flag: None,
            loading_message: None,
            empty: None,
            load_error: None,
            load_error_detail: None,
            can_copy: false,
            rows: vec![],
            fades: vec![],
            upgrade_banner: false,
            freemium: None,
            show_tabs: false,
            tabs: vec![],
            selected: None,
            lines_tab_label: app("Transcript"),
            lines_tab_flag: None,
            rtl: false,
            speakers: vec![],
            speaker_options: vec![],
            time_toggle: false,
            speaker_toggle: false,
            phone_speaker_toggle: false,
            show_time: false,
            show_speakers: false,
            download_enabled: false,
            media: Media::None,
            no_audio_alert: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TranscriptVm {
    pub job: Option<JobId>,
    /// The raw `job_name` (`TranscriptTitle.tsx`).
    pub title: String,
    pub duration_minutes: Option<i32>,
    /// The word after the duration (`{duration} min`).
    pub minutes_unit: Msg,
    pub status: Option<JobStatus>,
    pub status_badge: Option<StatusBadge>,
    pub is_translation: bool,
    /// The first target language's flag.
    pub translation_flag: Option<String>,
    pub loading_message: Option<Msg>,
    /// "No transcript lines available" once the job is past running.
    pub empty: Option<Msg>,
    pub load_error: Option<Msg>,
    pub load_error_detail: Option<String>,
    /// The copy-all action exists when lines exist.
    pub can_copy: bool,
    pub rows: Vec<LineRow>,
    /// FRE-020 display cut: `(row index, opacity percent)` for the faded rows.
    #[cfg_attr(feature = "ts", ts(type = "Array<[number, number]>"))]
    pub fades: Vec<(usize, u8)>,
    pub upgrade_banner: bool,
    pub freemium: Option<FreemiumBanner>,
    /// Language tabs exist only for translation jobs.
    pub show_tabs: bool,
    pub tabs: Vec<TabOption>,
    pub selected: Option<LanguageTab>,
    pub lines_tab_label: Msg,
    pub lines_tab_flag: Option<String>,
    pub rtl: bool,
    pub speakers: Vec<String>,
    pub speaker_options: Vec<SpeakerOption>,
    pub time_toggle: bool,
    pub speaker_toggle: bool,
    /// The phone action sheet's toggle (`job.speakers` only).
    pub phone_speaker_toggle: bool,
    pub show_time: bool,
    pub show_speakers: bool,
    pub download_enabled: bool,
    pub media: Media,
    /// "Audio is not available." for five seconds after a failed URL request.
    pub no_audio_alert: Option<Msg>,
}

#[cfg(test)]
mod tests;
