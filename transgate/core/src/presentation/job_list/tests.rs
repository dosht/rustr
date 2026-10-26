use super::*;
use crate::languages::Language;
use crate::presentation::fixtures;

fn listed() -> JobListState {
    let mut s = JobListState::default();
    s.update(Event::Refresh);
    s.update(Event::PageLoaded {
        jobs: vec![fixtures::job(12, JobStatus::Running, "seminar.mp4"), fixtures::job(11, JobStatus::Completed, "podcast.mp3")],
        next_cursor: Some(JobId(11)),
        replace: true,
    });
    s
}

#[test]
fn labels_and_durations_follow_the_source() {
    assert_eq!(status_label(JobStatus::NotStarted), app("NOT STARTED"));
    assert_eq!(status_label(JobStatus::SpeakerLabeling), app("SPEAKER LABELING"));
    assert_eq!(duration_text(Some(11)).as_deref(), Some("11:00"));
    assert_eq!(duration_text(None), None);
    assert_eq!(duration_text(Some(0)), None);
    assert_eq!(status_tone(JobStatus::Pending), Tone::Progress);
}

// JobListTable.tsx / MobileJobListView.tsx: a full spinner only for the first page, the sentinel for more.
#[test]
fn the_first_page_and_further_pages_load_differently() {
    let mut s = JobListState::default();
    assert_eq!(s.update(Event::Refresh), vec![Effect::LoadPage { before: None }]);
    let vm = s.view(None, 0);
    assert!(vm.initial_loading && !vm.loading_more && !vm.empty);
    s.update(Event::PageLoaded { jobs: vec![fixtures::job(5, JobStatus::Completed, "a.mp3")], next_cursor: Some(JobId(5)), replace: true });
    assert_eq!(s.update(Event::LoadMore), vec![Effect::LoadPage { before: Some(JobId(5)) }]);
    let vm = s.view(None, 0);
    assert!(!vm.initial_loading && vm.loading_more && !vm.can_load_more);
    assert!(s.update(Event::LoadMore).is_empty(), "one page request at a time");
}

#[test]
fn a_failed_page_shows_the_source_error_with_its_detail_until_a_page_loads() {
    let mut s = listed();
    s.update(Event::LoadMore);
    s.update(Event::LoadFailed { detail: Some("timeout".into()) });
    let vm = s.view(None, 0);
    assert_eq!((vm.error, vm.error_detail.as_deref()), (Some(LOAD_FAILED), Some("timeout")));
    assert_eq!(vm.rows.len(), 2, "loaded rows stay");
    s.update(Event::Refresh);
    s.update(Event::PageLoaded { jobs: vec![], next_cursor: None, replace: true });
    assert_eq!(s.view(None, 0).error, None);
}

#[test]
fn the_route_job_is_the_selected_row() {
    let vm = listed().view(Some(JobId(11)), 0);
    let selected: Vec<JobId> = vm.rows.iter().filter(|r| r.selected).map(|r| r.id).collect();
    assert_eq!(selected, vec![JobId(11)]);
}

// JobListContainer.tsx:95-113: the row download saves every stored line with timestamps on and speakers
// as the job has them. The server export builds it and records transcript_downloaded; one at a time.
#[test]
fn a_row_download_is_one_server_export_of_every_line() {
    let mut s = listed();
    let request = ExportRequest {
        kind: ExportKind::Wysiwyg { show_timestamps: None, show_speakers: None },
        surface: ExportSurface::JobList,
        labels: ExportLabels::default(),
    };
    assert_eq!(s.update(Event::DownloadRequested { job: JobId(11) }), vec![Effect::Export { job: JobId(11), request: request.clone() }]);
    assert!(s.view(None, 0).rows[1].downloading);
    assert!(s.update(Event::DownloadRequested { job: JobId(12) }).is_empty(), "one download at a time");
    s.update(Event::DownloadFinished { job: JobId(12) });
    assert!(s.view(None, 0).rows[1].downloading, "another job's answer changes nothing");
    s.update(Event::DownloadFinished { job: JobId(11) });
    assert!(!s.view(None, 0).rows[1].downloading);
    assert_eq!(s.update(Event::DownloadRequested { job: JobId(12) }), vec![Effect::Export { job: JobId(12), request }]);
}

// JobListTable.tsx / MobileJobCard.tsx projections.
#[test]
fn rows_carry_the_source_display_name_actions_and_dates() {
    let mut translated = fixtures::job(13, JobStatus::Completed, "talk.final.mp3");
    translated.translate_to = Language::from_name("German").into_iter().collect();
    translated.language = None;
    translated.speakers = false;
    let mut unnamed = fixtures::job(12, JobStatus::Running, "");
    unnamed.created_at = crate::ids::Micros(1_767_625_440_000_000);
    let mut s = JobListState::default();
    s.update(Event::PageLoaded { jobs: vec![translated, unnamed], next_cursor: None, replace: true });
    let vm = s.view(None, 180);
    let (t, u) = (&vm.rows[0], &vm.rows[1]);
    assert_eq!((t.display_name.as_deref(), t.is_translation, t.can_download, t.can_feedback), (Some("talk.final"), true, true, true));
    assert_eq!((t.language.as_deref(), t.speakers.clone()), (None, None));
    assert_eq!((u.display_name.as_deref(), u.can_download, u.speakers.clone()), (None, false, Some(MULTIPLE_SPEAKERS)));
    assert_eq!((u.date_text.as_str(), u.created_at_ms), ("Jan 5, 2026, 06:04 PM", 1_767_625_440_000));
    assert_eq!((vm.untitled, vm.auto_detect, vm.end_of_list), (UNTITLED, AUTO_DETECT, None));
}

#[test]
fn the_end_of_list_shows_only_after_more_than_one_page_without_more() {
    let jobs: Vec<Job> = (1..=21).rev().map(|i| fixtures::job(i, JobStatus::Completed, "a.mp3")).collect();
    let mut s = JobListState::default();
    s.update(Event::PageLoaded { jobs: jobs.clone(), next_cursor: Some(JobId(1)), replace: true });
    assert_eq!(s.view(None, 0).end_of_list, None);
    s.update(Event::PageLoaded { jobs, next_cursor: None, replace: true });
    assert_eq!(s.view(None, 0).end_of_list, Some(NO_MORE_JOBS));
    assert_eq!(without_extension("a.b.mp3"), "a.b");
    assert_eq!(without_extension(".env"), ".env");
}

// SideBar.tsx / MobileNavigationContainer.tsx: sidebar names lose the extension (empty when missing) and
// offer feedback for every status; the drawer keeps the raw name, the first twenty jobs and the source's
// minutes-as-seconds duration.
#[test]
fn the_sidebar_and_drawer_follow_their_own_source_rules() {
    let mut jobs: Vec<Job> = (1..=25).rev().map(|i| fixtures::job(i, JobStatus::Running, "talk.mp3")).collect();
    jobs[0].name = None;
    jobs[1].duration_minutes = Some(75);
    let mut s = JobListState::default();
    s.update(Event::PageLoaded { jobs, next_cursor: None, replace: true });
    let vm = s.view(Some(JobId(24)), 0);
    assert_eq!((vm.rows[0].sidebar_name.as_str(), vm.rows[1].sidebar_name.as_str()), ("", "talk"));
    assert!(vm.rows.iter().all(|r| r.sidebar_feedback && !r.can_feedback), "running jobs: menu feedback only");
    assert_eq!(vm.recent.len(), MAX_RECENT_JOBS);
    assert_eq!((vm.recent[0].file_name.clone(), vm.recent[1].file_name.as_deref()), (None, Some("talk.mp3")));
    assert_eq!((vm.recent[1].duration.as_deref(), vm.recent[1].selected), (Some("1m 15s"), true));
    assert_eq!(vm.recent_empty, NO_RECENT);
}
