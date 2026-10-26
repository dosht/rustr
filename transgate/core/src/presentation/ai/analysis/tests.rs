use serde_json::json;

use super::*;
use crate::ai::analysis::{DetectedType, GenericAnalysis, MeetingAnalysis};
use crate::ai::ports::{Draft, FinalContent};
use crate::ai::analysis::AnalysisResult;
use crate::ids::{JobId, MICROS_PER_MINUTE, Micros};

const JOB: JobId = JobId(7);
const NOW: Micros = Micros(1_800_000_000_000_000);

fn key(n: u128) -> RequestKey {
    uuid::Uuid::from_u128(n)
}

fn generation(n: u128) -> GenerationId {
    GenerationId(uuid::Uuid::from_u128(0x6e00 + n))
}

fn selected(lines: bool) -> Event {
    Event::Selected { job: JOB, job_name: Some("Weekly sync".into()), file_name: Some("sync.mp3".into()), has_lines: lines }
}

fn view(id: GenerationId, status: GenerationStatus, draft: Draft, final_content: Option<FinalContent>, created_at: Micros) -> GenerationView {
    GenerationView {
        id,
        job: JOB,
        kind: crate::ai::GenerationKind::Analysis,
        status,
        sequence: 3,
        source_revision: 1,
        consent_epoch: 1,
        draft,
        final_content,
        durable: true,
        remote_outcome_unknown: false,
        execution: None,
        tokens_used: None,
        interruption: None,
        created_at,
        finished_at: None,
    }
}

fn meeting_progress() -> AnalysisProgress {
    AnalysisProgress {
        detected: Some(DetectedType { kind: AnalysisType::Meeting, confidence: 92.0 }),
        summary: Some("Team agreed the plan.".into()),
        analysis: Some(Analysis::Meeting(MeetingAnalysis { next_steps: Some(vec!["Ship".into()]), ..Default::default() })),
        content_sections: 2,
        expected_content_sections: Some(4),
        warnings: vec![],
    }
}

/// A granted, selected panel whose cache read found nothing.
fn idle_panel() -> AnalysisPanelState {
    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    assert_eq!(s.update(selected(true)), vec![Effect::LoadAnalysis { job: JOB }]);
    s.update(Event::AnalysisLoaded { job: JOB, result: Ok(None) });
    s
}

fn wire(s: &mut AnalysisPanelState, id: GenerationId, w: AnalysisWire) -> Vec<Effect> {
    s.update(Event::WatchAnalysis { generation: id, wire: w })
}

#[test]
fn opening_the_tab_generates_once_when_consent_and_lines_exist() {
    let mut s = idle_panel();
    assert_eq!(s.update(Event::Shown { key: key(1) }), vec![Effect::StartAnalysis { job: JOB, key: key(1) }]);
    assert_eq!(s.view(NOW).state, PanelState::Loading);
    assert_eq!(s.view(NOW).progress.as_ref().map(|p| (p.stage.clone(), p.percentage)), Some((ai("Starting..."), 0)));
    s.update(Event::Hidden);
    assert!(s.update(Event::Shown { key: key(2) }).is_empty(), "hasTriggeredAnalysis: one automatic start per job");
}

#[test]
fn no_start_without_consent_lines_or_an_open_tab() {
    let mut s = AnalysisPanelState::default();
    assert!(s.update(selected(true)).is_empty(), "no consent: nothing is read or started");
    assert!(s.update(Event::Shown { key: key(1) }).is_empty());
    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(false));
    s.update(Event::AnalysisLoaded { job: JOB, result: Ok(None) });
    assert!(s.update(Event::Shown { key: key(1) }).is_empty(), "no transcript lines");
    assert!(s.view(NOW).no_transcript, "idle with no lines shows the no-content message");
    let mut s = idle_panel();
    assert!(s.update(Event::Hidden).is_empty(), "the cache read alone never starts work");
}

#[test]
fn start_header_attaches_the_canonical_analysis_watch_and_sections_render_progressively() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    assert_eq!(s.update(Event::StartAccepted { key: key(1), generation: g }), vec![Effect::Watch { generation: g, kind: WatchKind::Analysis, after: None }]);
    wire(&mut s, g, AnalysisWire::SectionCount { total: 4 });
    wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Meeting, confidence: 92.0 });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("Team agreed the plan.") });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "actionItems".into(), data: json!([{"task": "Draft memo", "owner": "Ada"}]) });
    let vm = s.view(NOW);
    assert_eq!(vm.state, PanelState::Loading);
    assert!(vm.has_data, "copy and export appear as soon as data exists, even while loading");
    assert_eq!(vm.detected.map(|d| d.kind), Some(AnalysisType::Meeting));
    assert_eq!(vm.summary.as_deref(), Some("Team agreed the plan."));
    let Some(Analysis::Meeting(m)) = &vm.analysis else { panic!("typed meeting sections") };
    assert_eq!(m.action_items.as_ref().unwrap()[0].owner.as_deref(), Some("Ada"));
    let progress = vm.progress.unwrap();
    assert_eq!(progress.stage, ai("Analyzing ({{received}}/{{total}} sections)").number("received", 2).number("total", 4));
    assert_eq!(progress.percentage, 55, "round(2/4*90)+10");
    wire(&mut s, g, AnalysisWire::Complete { tokens_used: 120, processing_time_ms: 900 });
    let vm = s.view(NOW);
    assert_eq!(vm.state, PanelState::Ready);
    assert!(vm.progress.is_none());
}

#[test]
fn shared_names_are_typed_by_the_detected_shape() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Interview, confidence: 80.0 });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "nextSteps".into(), data: json!([{"action": "Call back", "priority": "high"}]) });
    let Some(Analysis::Interview(i)) = &s.view(NOW).analysis else { panic!("interview") };
    assert_eq!(i.next_steps.as_ref().unwrap()[0].action, "Call back");
}

#[test]
fn a_reattached_resnapshot_replays_sections_idempotently() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    for _ in 0..2 {
        wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Generic, confidence: 50.0 });
        wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("One.") });
        wire(&mut s, g, AnalysisWire::SectionReady { section_type: "highlights".into(), data: json!(["A", "B"]) });
    }
    let vm = s.view(NOW);
    let Some(Analysis::Generic(g)) = &vm.analysis else { panic!("generic") };
    assert_eq!(g.highlights.as_deref(), Some(&["A".to_string(), "B".to_string()][..]));
    assert_eq!(g.summary.as_deref(), Some("One."), "generic mirrors the common summary");
    assert_eq!(vm.progress.unwrap().stage, ai("Analyzing ({{received}} sections)").number("received", 2));
}

#[test]
fn an_error_event_keeps_finished_sections_and_offers_retry_with_a_new_key() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Generic, confidence: 50.0 });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("Partial.") });
    wire(&mut s, g, AnalysisWire::Error { code: "ANALYSIS_FAILED".into(), message: "The model stopped early (length)".into() });
    let vm = s.view(NOW);
    assert_eq!(vm.state, PanelState::Interrupted);
    assert_eq!(vm.summary.as_deref(), Some("Partial."), "D1: valid partial sections stay visible");
    assert!(vm.can_retry && vm.has_data);
    assert_eq!(s.update(Event::RetryRequested { key: key(2) }), vec![Effect::StartAnalysis { job: JOB, key: key(2) }]);
    assert!(!s.view(NOW).has_data, "a retry replaces the old analysis payload");
}

#[test]
fn an_unknown_start_outcome_replays_the_same_key_never_a_new_one() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    assert_eq!(s.update(Event::StartUnknown { key: key(1) }), vec![Effect::StartAnalysis { job: JOB, key: key(1) }]);
    assert!(s.update(Event::StartUnknown { key: key(1) }).is_empty(), "one automatic replay");
    let vm = s.view(NOW);
    assert_eq!(vm.state, PanelState::Error);
    assert!(vm.can_retry);
    assert_eq!(
        s.update(Event::RetryRequested { key: key(9) }),
        vec![Effect::StartAnalysis { job: JOB, key: key(1) }],
        "the outcome is still unknown: retrying reconciles the same request"
    );
}

#[test]
fn a_rejected_start_shows_the_failure_and_a_busy_job_attaches_the_running_analysis() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let failure = ApiFailure::http(403, Some("consent_required"));
    let body = StartRejection { error: "AI consent required. Please enable AI features in your settings.".into(), code: "consent_required".into(), ..Default::default() };
    s.update(Event::StartRejected { key: key(1), failure, body: Some(body) });
    let vm = s.view(NOW);
    assert_eq!(vm.state, PanelState::Error);
    assert_eq!(vm.error_detail.as_deref(), Some("AI consent required. Please enable AI features in your settings."));
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let running = generation(5);
    let busy = StartRejection { code: "ai_busy".into(), generation_id: Some(running), ..Default::default() };
    assert_eq!(
        s.update(Event::StartRejected { key: key(1), failure: ApiFailure::http(409, Some("ai_busy")), body: Some(busy) }),
        vec![Effect::Watch { generation: running, kind: WatchKind::Analysis, after: None }],
        "another tab's analysis is attached, not duplicated"
    );
    assert_eq!(s.view(NOW).state, PanelState::Loading);
}

#[test]
fn stale_or_foreign_watch_events_change_nothing() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    assert!(s.update(Event::StartAccepted { key: key(7), generation: generation(2) }).is_empty(), "another key's header is ignored");
    wire(&mut s, generation(3), AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("Other") });
    assert!(!s.view(NOW).has_data);
}

#[test]
fn cached_completed_analysis_succeeds_and_legacy_empty_rows_are_a_miss() {
    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(true));
    let result = AnalysisResult {
        detected: DetectedType { kind: AnalysisType::Generic, confidence: 70.0 },
        summary: "Cached".into(),
        analysis: Analysis::Generic(GenericAnalysis { summary: Some("Cached".into()), highlights: Some(vec![]) }),
        warnings: vec![],
    };
    let cached = view(generation(1), GenerationStatus::Completed, Draft::Empty, Some(FinalContent::Analysis { result: Box::new(result) }), NOW);
    assert!(s.update(Event::AnalysisLoaded { job: JOB, result: Ok(Some(cached)) }).is_empty());
    assert_eq!(s.view(NOW).state, PanelState::Ready);
    assert!(s.update(Event::Shown { key: key(1) }).is_empty(), "a cached result is not regenerated");

    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(true));
    let empty = view(generation(1), GenerationStatus::Completed, Draft::Empty, None, NOW);
    s.update(Event::AnalysisLoaded { job: JOB, result: Ok(Some(empty)) });
    assert_eq!(s.update(Event::Shown { key: key(1) }), vec![Effect::StartAnalysis { job: JOB, key: key(1) }], "AI-007 cache miss");
}

#[test]
fn a_cache_read_failure_allows_generation_through_the_server() {
    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(true));
    s.update(Event::AnalysisLoaded { job: JOB, result: Err(ApiFailure::network()) });
    assert_eq!(s.update(Event::Shown { key: key(1) }), vec![Effect::StartAnalysis { job: JOB, key: key(1) }]);
}

#[test]
fn a_running_cached_analysis_is_watched_with_ai008_staleness_from_creation() {
    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(true));
    let g = generation(4);
    let created = Micros(NOW.0 - 2 * MICROS_PER_MINUTE);
    let accepted = view(g, GenerationStatus::Accepted, Draft::Empty, None, created);
    assert_eq!(s.update(Event::AnalysisLoaded { job: JOB, result: Ok(Some(accepted)) }), vec![Effect::Watch { generation: g, kind: WatchKind::Analysis, after: None }]);
    let p = s.view(NOW).progress.unwrap();
    assert_eq!((p.stage, p.percentage), (ai("Analysis in progress… taking longer than expected"), 30));
    assert_eq!(s.view(Micros(created.0 + 30 * 1_000_000)).progress.unwrap().percentage, 10, "under a minute: in progress");
    assert_eq!(s.view(Micros(created.0 + 5 * MICROS_PER_MINUTE)).state, PanelState::Interrupted, "five minutes from creation is stale");

    let mut s = AnalysisPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(selected(true));
    let partial = view(generation(5), GenerationStatus::Running, Draft::Analysis { progress: Box::new(meeting_progress()) }, None, NOW);
    s.update(Event::AnalysisLoaded { job: JOB, result: Ok(Some(partial)) });
    let vm = s.view(NOW);
    assert!(vm.has_data);
    assert_eq!(vm.progress.unwrap().stage, ai("Analysis in progress… (you may have had another tab open)"));
}

#[test]
fn a_lost_watch_reconciles_from_the_snapshot_once_before_giving_up() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    assert_eq!(s.update(Event::WatchEnded { generation: g, result: Err(WatchFailure::Disconnected) }), vec![Effect::LoadAnalysis { job: JOB }]);
    let running = view(g, GenerationStatus::Running, Draft::Empty, None, NOW);
    assert_eq!(s.update(Event::AnalysisLoaded { job: JOB, result: Ok(Some(running)) }), vec![Effect::Watch { generation: g, kind: WatchKind::Analysis, after: None }]);
    assert!(s.update(Event::WatchEnded { generation: g, result: Err(WatchFailure::Disconnected) }).is_empty(), "no reconnect loop without progress");
    assert_eq!(s.view(NOW).state, PanelState::Interrupted);
    assert_eq!(s.update(Event::RetryRequested { key: key(2) }), vec![Effect::StartAnalysis { job: JOB, key: key(2) }], "a still-running job answers ai_busy and is attached");
}

#[test]
fn revoking_consent_or_changing_job_clears_content_and_detaches() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("x") });
    assert_eq!(s.update(Event::ConsentChanged { granted: false }), vec![Effect::Detach { generation: g }]);
    assert!(!s.view(NOW).has_data);
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    s.update(Event::StartAccepted { key: key(1), generation: g });
    let next = Event::Selected { job: JobId(8), job_name: None, file_name: None, has_lines: true };
    assert_eq!(s.update(next), vec![Effect::Detach { generation: g }, Effect::LoadAnalysis { job: JobId(8) }]);
}

#[test]
fn copy_and_export_capture_the_displayed_partial_at_click_time() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Generic, confidence: 50.0 });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("Partial summary") });
    let effects = s.update(Event::ExportRequested { format: DocumentFormat::Pdf, at: NOW, utc_offset_minutes: 180, phone: false });
    let [Effect::Export { document, format }] = effects.as_slice() else { panic!("one export effect: {effects:?}") };
    assert_eq!(*format, DocumentFormat::Pdf);
    assert!(document.text().contains("Partial summary"), "the visible loading partial, not a saved result");
    assert_eq!(s.view(NOW).export_busy, Some(DocumentFormat::Pdf));
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "highlights".into(), data: json!(["Later"]) });
    assert!(!document.text().contains("Later"), "captured once at the click");
    s.update(Event::ExportFinished { format: DocumentFormat::Pdf, result: Err(ExportFailure::Fonts) });
    assert_eq!(s.view(NOW).export_busy, None);
    assert!(s.view(NOW).export_error.is_some());
    s.update(Event::ExportErrorDismissed);
    assert!(s.view(NOW).export_error.is_none());

    let effects = s.update(Event::CopyRequested { at: NOW, utc_offset_minutes: 0 });
    let [Effect::Copy { text, html }] = effects.as_slice() else { panic!("one copy effect") };
    assert!(text.contains("Partial summary") && html.contains("Partial summary"));
}

#[test]
fn the_export_title_follows_each_source_layout() {
    let mut s = idle_panel();
    s.update(Event::Shown { key: key(1) });
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    wire(&mut s, g, AnalysisWire::TypeDetected { kind: AnalysisType::Generic, confidence: 50.0 });
    wire(&mut s, g, AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("S") });
    assert_eq!(s.export_title(false), "Weekly sync", "desktop: job_name || fileName || Analysis");
    assert_eq!(s.export_title(true), "sync.mp3", "phone: fileName || Analysis");
    assert!(s.update(Event::ExportRequested { format: DocumentFormat::Txt, at: NOW, utc_offset_minutes: 0, phone: false }).len() == 1);
    assert!(s.update(Event::ExportRequested { format: DocumentFormat::Md, at: NOW, utc_offset_minutes: 0, phone: false }).is_empty(), "one export at a time");
}
