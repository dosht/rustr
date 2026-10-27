//! History list and sidebar (JOB-036/039, PRV-012/032/034, jobs-transcription 2.1).
//! Cursor paging by id descending, 20 per page; the status allow-list and the
//! one-month window are applied by the use case, not the shell.

use serde::{Deserialize, Serialize};

use crate::export::ExportLabels;
use crate::ids::JobId;
use crate::jobs::app::{ExportKind, ExportRequest, ExportSurface, PAGE_SIZE};
use crate::jobs::domain::{Job, JobStatus};
use crate::presentation::dates;
use crate::presentation::msg::{Msg, app, common};

/// `getStatusLabel`: the status with its first underscore as a space, upper
/// case. The source rendered these untranslated; they are app catalog keys.
pub fn status_label(status: JobStatus) -> Msg {
    app(match status {
        JobStatus::Draft => "DRAFT",
        JobStatus::NotStarted => "NOT STARTED",
        JobStatus::Pending => "PENDING",
        JobStatus::Running => "RUNNING",
        JobStatus::Completed => "COMPLETED",
        JobStatus::Failed => "FAILED",
        JobStatus::Canceled => "CANCELED",
        JobStatus::Deleted => "DELETED",
        JobStatus::SpeakerLabeling => "SPEAKER LABELING",
        JobStatus::Uploaded => "UPLOADED",
        JobStatus::Processing => "PROCESSING",
        JobStatus::Ready => "READY",
    })
}

/// Semantic tone for a badge; shells map tones to tokens, never colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Neutral,
    Progress,
    Accent,
    Success,
    Danger,
}

pub fn status_tone(status: JobStatus) -> Tone {
    match status {
        JobStatus::Completed => Tone::Success,
        JobStatus::Failed => Tone::Danger,
        // The source shows speaker labeling in purple.
        JobStatus::SpeakerLabeling => Tone::Accent,
        s if s.is_in_progress() => Tone::Progress,
        _ => Tone::Neutral,
    }
}

/// `formatDuration`: stored minutes as `M:SS`; `None` (shown as
/// [`NOT_AVAILABLE`]) when missing or zero.
pub fn duration_text(minutes: Option<i32>) -> Option<String> {
    match minutes {
        None | Some(0) => None,
        Some(m) => {
            let total = i64::from(m) * 60;
            Some(format!("{}:{:02}", total / 60, total % 60))
        }
    }
}

pub const NOT_AVAILABLE: Msg = app("N/A");
pub const LOAD_FAILED: Msg = app("Error loading jobs");
pub const UNTITLED: Msg = app("Untitled");
pub const AUTO_DETECT: Msg = common("Auto-detect");
pub const MULTIPLE_SPEAKERS: Msg = app("2+ speakers");
pub const NO_MORE_JOBS: Msg = app("No more jobs to load");
/// "No more jobs to load" shows only past the first page (`JobListTable.tsx:465-469`).
pub const END_OF_LIST_AFTER: usize = 20;
/// The phone drawer lists the first twenty jobs (`useSidebarViewModel` `MAX_RECENT_JOBS`).
pub const MAX_RECENT_JOBS: usize = 20;
pub const NO_RECENT: Msg = common("mobile.navigation.noRecentTranscripts");

/// The drawer's duration text: the source formats the stored *minutes* as if
/// they were seconds (`Math.floor(d/60)}m ${d%60}s`), so 75 reads `1m 15s`.
pub fn drawer_duration(minutes: i32) -> String {
    format!("{}m {}s", minutes.div_euclid(60), minutes.rem_euclid(60))
}

/// One phone drawer entry (`MobileNavigationContainer.tsx:273-279`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RecentRow {
    pub id: JobId,
    /// The raw `job_name`, extension included; `None` shows `JobListVm::untitled`.
    pub file_name: Option<String>,
    pub duration: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at_ms: i64,
    pub selected: bool,
}

/// `removeFileExtension`: everything before the last dot, when there is one.
pub fn without_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobRow {
    pub id: JobId,
    /// `removeFileExtension(job_name)`; `None` shows `JobListVm::untitled`.
    pub display_name: Option<String>,
    pub status: JobStatus,
    pub status_label: Msg,
    pub tone: Tone,
    /// A spinner next to the badge while processing.
    pub spinner: bool,
    /// The Languages icon instead of play/mic while not processing.
    pub is_translation: bool,
    /// Download and feedback are offered only for completed jobs.
    pub can_download: bool,
    pub can_feedback: bool,
    /// `M:SS`; `None` shows `JobListVm::not_available`.
    pub duration: Option<String>,
    /// The phone card's "2+ speakers" (only when the job has speakers).
    pub speakers: Option<Msg>,
    /// The stored language's English name as the source showed it (pickers and
    /// the subtitle do not translate it); `None` shows `JobListVm::auto_detect`.
    pub language: Option<String>,
    pub flag: Option<String>,
    /// `formatJobDate`: the fixed en-US text at the browser's local offset.
    pub date_text: String,
    /// The phone table's `toLocaleDateString()`: the platform formats this
    /// instant (milliseconds since the epoch) with its own locale and zone.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at_ms: i64,
    /// The open job's row is highlighted (from the route).
    pub selected: bool,
    /// A row download is being prepared.
    pub downloading: bool,
    /// The desktop sidebar name: `removeFileExtension(job_name || "")`, empty when missing.
    pub sidebar_name: String,
    /// The sidebar menu offers Feedback for every status (`SideBar.tsx:277-292`).
    pub sidebar_feedback: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    PageLoaded { jobs: Vec<Job>, next_cursor: Option<JobId>, replace: bool },
    /// The page request failed; `detail` is shown under "Error loading jobs".
    LoadFailed { detail: Option<String> },
    /// D11 poll tick: ask for every loaded row again (`Effect::PollPage`).
    PollRequested,
    /// The poll's rows, newest first, down to at least the requested `through`.
    Polled { jobs: Vec<Job> },
    /// The poll failed: rows stay, no error is shown, the next tick retries.
    PollFailed,
    LoadMore,
    DeleteRequested { job: JobId },
    DeleteConfirmed,
    DeleteCanceled,
    Refresh,
    /// The desktop row download (`JobListContainer.tsx:95-113`).
    DownloadRequested { job: JobId },
    /// The export answer arrived (saved, or failed: the source only logged it).
    DownloadFinished { job: JobId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    LoadPage { before: Option<JobId> },
    /// D11: the owned list from the newest job down to the page holding
    /// `through` (the oldest loaded row); `None` asks for the first page only.
    PollPage { through: Option<JobId> },
    DeleteJob { job: JobId },
    /// `POST /api/jobs/{id}/export`: the server builds the ZIP and records
    /// `transcript_downloaded` (EXP-016); the shell saves the bytes.
    Export { job: JobId, request: ExportRequest },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Loading {
    #[default]
    Idle,
    /// The first page, or a refresh: a full spinner.
    Initial,
    /// A further page: the sentinel spinner.
    More,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobListState {
    pub jobs: Vec<Job>,
    pub next_cursor: Option<JobId>,
    loading: Loading,
    /// The last page request failed.
    pub failed: bool,
    pub error_detail: Option<String>,
    pub confirm_delete: Option<JobId>,
    downloading: Option<JobId>,
    pub labels: ExportLabels,
    /// The `through` of the poll in flight (D11).
    polling: Option<Option<JobId>>,
    /// Rows deleted here; a poll answered before the delete committed never revives them.
    deleted: Vec<JobId>,
}

impl JobListState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::PageLoaded { jobs, next_cursor, replace } => {
                self.loading = Loading::Idle;
                self.failed = false;
                self.error_detail = None;
                if replace {
                    self.jobs = jobs;
                } else {
                    for j in jobs {
                        if !self.jobs.iter().any(|x| x.id == j.id) {
                            self.jobs.push(j);
                        }
                    }
                }
                self.next_cursor = next_cursor;
                vec![]
            }
            Event::LoadFailed { detail } => {
                self.loading = Loading::Idle;
                self.failed = true;
                self.error_detail = detail;
                vec![]
            }
            Event::PollRequested => {
                if self.loading != Loading::Idle {
                    return vec![];
                }
                let through = self.jobs.iter().map(|j| j.id).min();
                self.polling = Some(through);
                vec![Effect::PollPage { through }]
            }
            Event::Polled { jobs } => {
                let through = self.polling.take().unwrap_or_else(|| self.jobs.iter().map(|j| j.id).min());
                // The polled window: down to `through`, or a full first page's last row.
                let floor = match through {
                    Some(t) => t.0,
                    None if jobs.len() >= PAGE_SIZE as usize => jobs.iter().map(|j| j.id.0).min().unwrap_or(i64::MIN),
                    None => i64::MIN,
                };
                let deleted = &self.deleted;
                self.jobs.retain(|j| j.id.0 < floor);
                self.jobs.extend(jobs.into_iter().filter(|j| j.id.0 >= floor && j.status.is_listed() && !deleted.contains(&j.id)));
                self.jobs.sort_by_key(|j| std::cmp::Reverse(j.id));
                vec![]
            }
            Event::PollFailed => {
                self.polling = None;
                vec![]
            }
            Event::LoadMore => match (self.loading, self.next_cursor) {
                (Loading::Idle, Some(cursor)) => {
                    self.loading = Loading::More;
                    vec![Effect::LoadPage { before: Some(cursor) }]
                }
                _ => vec![],
            },
            Event::Refresh => {
                self.loading = Loading::Initial;
                vec![Effect::LoadPage { before: None }]
            }
            Event::DeleteRequested { job } => {
                self.confirm_delete = Some(job);
                vec![]
            }
            Event::DeleteCanceled => {
                self.confirm_delete = None;
                vec![]
            }
            Event::DeleteConfirmed => match self.confirm_delete.take() {
                Some(job) => {
                    // JOB-036: a soft delete; the row leaves the list.
                    self.jobs.retain(|j| j.id != job);
                    self.deleted.push(job);
                    vec![Effect::DeleteJob { job }]
                }
                None => vec![],
            },
            // `transcriptFileSaver`: every stored line, timestamps on, speakers as the job has them.
            Event::DownloadRequested { job } if self.downloading.is_none() && self.jobs.iter().any(|j| j.id == job) => {
                self.downloading = Some(job);
                let request = ExportRequest {
                    kind: ExportKind::Wysiwyg { show_timestamps: None, show_speakers: None },
                    surface: ExportSurface::JobList,
                    labels: self.labels.clone(),
                };
                vec![Effect::Export { job, request }]
            }
            Event::DownloadRequested { .. } => vec![],
            Event::DownloadFinished { job } => {
                if self.downloading == Some(job) {
                    self.downloading = None;
                }
                vec![]
            }
        }
    }

    /// D11 cadence: a listed job is still in progress.
    pub fn has_active(&self) -> bool {
        self.jobs.iter().any(|j| j.status.is_in_progress())
    }

    /// `selected` is the route's job, highlighted in the list; dates are shown
    /// at the browser's `utc_offset_minutes`.
    pub fn view(&self, selected: Option<JobId>, utc_offset_minutes: i16) -> JobListVm {
        let has_more = self.next_cursor.is_some();
        let idle = self.loading == Loading::Idle;
        JobListVm {
            rows: self
                .jobs
                .iter()
                .map(|j| JobRow {
                    id: j.id,
                    display_name: j.name.as_deref().filter(|n| !n.is_empty()).map(|n| without_extension(n).to_string()),
                    status: j.status,
                    status_label: status_label(j.status),
                    tone: status_tone(j.status),
                    spinner: j.status.is_in_progress(),
                    is_translation: j.is_translation(),
                    can_download: j.status == JobStatus::Completed,
                    can_feedback: j.status == JobStatus::Completed,
                    duration: duration_text(j.duration_minutes),
                    speakers: j.speakers.then_some(MULTIPLE_SPEAKERS),
                    language: j.language.map(|l| l.name().to_string()),
                    flag: j.language.map(|l| l.flag().to_string()),
                    date_text: dates::job_date(j.created_at, utc_offset_minutes),
                    created_at_ms: j.created_at.0.div_euclid(1000),
                    selected: selected == Some(j.id),
                    downloading: self.downloading == Some(j.id),
                    sidebar_name: without_extension(j.name.as_deref().unwrap_or_default()).to_string(),
                    sidebar_feedback: true,
                })
                .collect(),
            recent: self
                .jobs
                .iter()
                .take(MAX_RECENT_JOBS)
                .map(|j| RecentRow {
                    id: j.id,
                    file_name: j.name.clone().filter(|n| !n.is_empty()),
                    duration: j.duration_minutes.map(drawer_duration),
                    created_at_ms: j.created_at.0.div_euclid(1000),
                    selected: selected == Some(j.id),
                })
                .collect(),
            recent_empty: NO_RECENT,
            can_load_more: has_more && idle,
            initial_loading: self.loading == Loading::Initial,
            loading_more: self.loading == Loading::More,
            end_of_list: (!has_more && idle && self.jobs.len() > END_OF_LIST_AFTER).then_some(NO_MORE_JOBS),
            error: self.failed.then_some(LOAD_FAILED),
            error_detail: self.error_detail.clone(),
            confirm_delete: self.confirm_delete,
            empty: self.jobs.is_empty() && idle,
            not_available: NOT_AVAILABLE,
            untitled: UNTITLED,
            auto_detect: AUTO_DETECT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobListVm {
    pub rows: Vec<JobRow>,
    pub can_load_more: bool,
    /// The full spinner: the first page or a refresh.
    pub initial_loading: bool,
    /// The sentinel spinner of a further page.
    pub loading_more: bool,
    /// "No more jobs to load" after more than one page.
    pub end_of_list: Option<Msg>,
    pub error: Option<Msg>,
    pub error_detail: Option<String>,
    pub confirm_delete: Option<JobId>,
    pub empty: bool,
    /// The text of a missing duration.
    pub not_available: Msg,
    /// The text of a missing job name.
    pub untitled: Msg,
    /// The subtitle of a job without a stored language.
    pub auto_detect: Msg,
    /// The phone drawer's recent list: the first twenty jobs, no paging or delete.
    pub recent: Vec<RecentRow>,
    pub recent_empty: Msg,
}

#[cfg(test)]
mod tests;
