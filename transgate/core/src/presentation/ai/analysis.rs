//! The AI Analysis tab (WEB-027/028, AI-F03..F06, AI-001..009): one per
//! selected job. It reads the latest analysis once per job, generates once when
//! the tab opens with consent and transcript lines, renders the six typed shapes
//! as their sections stream, and shows interrupted partials with retry.
//!
//! Display state is the typed `AnalysisProgress` the content owner defined.
//! Watch events are applied idempotently by section name, because every
//! attachment begins from a full resnapshot. Copy and export capture exactly
//! what is displayed at the click (P5-DOCS-FINAL-A), including loading and
//! interrupted partials, never a reloaded saved result.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::analysis::{
    Analysis, AnalysisProgress, AnalysisType, DetectedType, GenericAnalysis, InterviewAnalysis, LectureAnalysis,
    MeetingAnalysis, SalesAnalysis, SupportAnalysis, sections,
};
use crate::ai::presentation::{Staleness, staleness};
use crate::ai::{Draft, FinalContent, GenerationId, GenerationStatus, GenerationView};
use crate::documents::{AnalysisDocument, AnalysisMetadata, DocumentFormat};
use crate::ids::{JobId, Micros};
use crate::presentation::ai::section_copy::{SectionId, by_priority, section_copy};
use crate::presentation::ai::{AnalysisWire, ApiFailure, RequestKey, StartRejection, WatchFailure, WatchKind};
use crate::presentation::msg::{Msg, ai, app};

/// The source's export title fallback (`useExportAnalysis` default).
pub const DEFAULT_TITLE: &str = "Analysis";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A job was selected. `job_name` and `file_name` select the export title.
    Selected { job: JobId, job_name: Option<String>, file_name: Option<String>, has_lines: bool },
    /// The selected job's transcript lines arrived or disappeared.
    LinesChanged { has_lines: bool },
    ConsentChanged { granted: bool },
    /// The analysis tab (desktop) or analysis view (phone) became active.
    /// The key is used only if this opening starts the automatic generation.
    Shown { key: RequestKey },
    Hidden,
    AnalysisLoaded { job: JobId, result: Result<Option<GenerationView>, ApiFailure> },
    StartAccepted { key: RequestKey, generation: GenerationId },
    StartRejected { key: RequestKey, failure: ApiFailure, body: Option<StartRejection> },
    /// No `x-generation-id` was observed: the outcome is unknown.
    StartUnknown { key: RequestKey },
    WatchAnalysis { generation: GenerationId, wire: AnalysisWire },
    WatchEnded { generation: GenerationId, result: Result<(), WatchFailure> },
    RetryRequested { key: RequestKey },
    /// The click instant and its local offset east of UTC.
    CopyRequested { at: Micros, utc_offset_minutes: i16 },
    /// A specialized section's own copy button (`SectionHeader`).
    CopySectionRequested { section: SectionId },
    /// `phone` selects the phone layout's source title rule.
    ExportRequested { format: DocumentFormat, at: Micros, utc_offset_minutes: i16, phone: bool },
    ExportFinished { format: DocumentFormat, result: Result<(), ExportFailure> },
    ExportErrorDismissed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// `GET /api/ai/jobs/{job}/analysis`.
    LoadAnalysis { job: JobId },
    /// `POST /api/v1/ai/analyze` with `requestId = key`; header, drop, then `StartAccepted`.
    StartAnalysis { job: JobId, key: RequestKey },
    Watch { generation: GenerationId, kind: WatchKind, after: Option<u64> },
    Detach { generation: GenerationId },
    /// Rich clipboard: HTML plus its plain-text alternative.
    Copy { text: String, html: String },
    /// Render through the document adapter, then save the bytes.
    Export { document: Box<AnalysisDocument>, format: DocumentFormat },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFailure {
    /// The bundled fonts could not be loaded.
    Fonts,
    /// The document adapter refused the captured model.
    Document,
    /// The bytes could not be saved or shared.
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PanelState {
    Idle,
    CheckingCache,
    Loading,
    Ready,
    Interrupted,
    Cancelled,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProgressVm {
    pub stage: Msg,
    pub percentage: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisPanelVm {
    pub state: PanelState,
    /// Idle with no transcript lines: "No transcript content available".
    pub no_transcript: bool,
    pub detected: Option<DetectedType>,
    pub summary: Option<String>,
    pub analysis: Option<Analysis>,
    /// Copy and export are offered whenever content is displayed.
    pub has_data: bool,
    pub progress: Option<ProgressVm>,
    pub can_retry: bool,
    /// The server's own refusal text, shown under "Failed to generate summary".
    pub error_detail: Option<String>,
    pub error_hint: Option<Msg>,
    pub export_busy: Option<DocumentFormat>,
    pub export_error: Option<Msg>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
enum Phase {
    #[default]
    Idle,
    Checking { reconcile: bool },
    Starting { key: RequestKey, replayed: bool },
    Watching,
    Completed,
    Interrupted,
    Cancelled,
    Failed { unknown_key: Option<RequestKey>, detail: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisPanelState {
    job: Option<JobId>,
    job_name: Option<String>,
    file_name: Option<String>,
    has_lines: bool,
    consent: bool,
    /// A key from the current opening of the tab, held until it can start.
    armed: Option<RequestKey>,
    /// `hasTriggeredAnalysis`: one automatic start per job.
    triggered: bool,
    checked: bool,
    phase: Phase,
    generation: Option<GenerationId>,
    progress: AnalysisProgress,
    /// A running generation found by the cache read, for AI-008 labels.
    resumed: Option<GenerationView>,
    /// Attached to another tab's running analysis (`ai_busy`).
    attached_existing: bool,
    saw_events: bool,
    reconnected: bool,
    export_busy: Option<DocumentFormat>,
    export_error: Option<Msg>,
}

impl AnalysisPanelState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Selected { job, job_name, file_name, has_lines } => {
                let mut effects = self.detach();
                let consent = self.consent;
                *self = AnalysisPanelState { job: Some(job), job_name, file_name, has_lines, consent, ..AnalysisPanelState::default() };
                effects.extend(self.check());
                effects
            }
            Event::LinesChanged { has_lines } => {
                self.has_lines = has_lines;
                self.maybe_start()
            }
            Event::ConsentChanged { granted } => {
                if granted == self.consent {
                    return vec![];
                }
                self.consent = granted;
                if !granted {
                    let effects = self.detach();
                    self.clear_content();
                    self.phase = Phase::Idle;
                    self.checked = false;
                    self.triggered = false;
                    return effects;
                }
                self.check()
            }
            Event::Shown { key } => {
                self.armed = Some(key);
                self.maybe_start()
            }
            Event::Hidden => {
                self.armed = None;
                vec![]
            }
            Event::AnalysisLoaded { job, result } => {
                if self.job != Some(job) {
                    return vec![];
                }
                let Phase::Checking { reconcile } = self.phase.clone() else { return vec![] };
                self.loaded(reconcile, result)
            }
            Event::StartAccepted { key, generation } => {
                if !matches!(self.phase, Phase::Starting { key: k, .. } if k == key) {
                    return vec![];
                }
                self.clear_content();
                self.watch(generation)
            }
            Event::StartRejected { key, failure: _, body } => {
                if !matches!(self.phase, Phase::Starting { key: k, .. } if k == key) {
                    return vec![];
                }
                if let Some(running) = body.as_ref().filter(|b| b.code == "ai_busy").and_then(|b| b.generation_id) {
                    self.clear_content();
                    let effects = self.watch(running);
                    self.attached_existing = true;
                    return effects;
                }
                let detail = body.map(|b| b.error).filter(|e| !e.is_empty());
                self.phase = Phase::Failed { unknown_key: None, detail };
                vec![]
            }
            Event::StartUnknown { key } => {
                let Phase::Starting { key: k, replayed } = self.phase.clone() else { return vec![] };
                if k != key {
                    return vec![];
                }
                if replayed {
                    self.phase = Phase::Failed { unknown_key: Some(key), detail: None };
                    return vec![];
                }
                self.phase = Phase::Starting { key, replayed: true };
                vec![self.start_effect(key)]
            }
            Event::WatchAnalysis { generation, wire } => {
                if self.generation != Some(generation) || !matches!(self.phase, Phase::Watching) {
                    return vec![];
                }
                self.saw_events = true;
                self.reconnected = false;
                self.apply_wire(wire);
                vec![]
            }
            Event::WatchEnded { generation, result: _ } => {
                if self.generation != Some(generation) || !matches!(self.phase, Phase::Watching) {
                    return vec![];
                }
                if self.reconnected {
                    self.phase = Phase::Interrupted;
                    return vec![];
                }
                self.reconnected = true;
                self.phase = Phase::Checking { reconcile: true };
                self.job.map(|job| Effect::LoadAnalysis { job }).into_iter().collect()
            }
            Event::RetryRequested { key } => self.retry(key),
            Event::CopyRequested { at, utc_offset_minutes } => match self.capture(at, utc_offset_minutes, false) {
                Some(document) => vec![Effect::Copy { text: document.text(), html: document.html() }],
                None => vec![],
            },
            Event::CopySectionRequested { section } => match self.progress.analysis.as_ref().and_then(|a| section_copy(a, section)) {
                Some((text, html)) => vec![Effect::Copy { text, html }],
                None => vec![],
            },
            Event::ExportRequested { format, at, utc_offset_minutes, phone } => {
                if self.export_busy.is_some() {
                    return vec![];
                }
                if !self.has_data() {
                    return vec![];
                }
                match self.capture(at, utc_offset_minutes, phone) {
                    Some(document) => {
                        self.export_busy = Some(format);
                        self.export_error = None;
                        vec![Effect::Export { document: Box::new(document), format }]
                    }
                    None => {
                        self.export_error = Some(export_failed(format));
                        vec![]
                    }
                }
            }
            Event::ExportFinished { format, result } => {
                if self.export_busy != Some(format) {
                    return vec![];
                }
                self.export_busy = None;
                self.export_error = result.err().map(|_| export_failed(format));
                vec![]
            }
            Event::ExportErrorDismissed => {
                self.export_error = None;
                vec![]
            }
        }
    }

    /// The source title rule per layout: desktop `job_name || fileName ||
    /// "Analysis"`, phone `fileName || "Analysis"`.
    pub fn export_title(&self, phone: bool) -> String {
        let name = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
        let title = if phone { name(&self.file_name) } else { name(&self.job_name).or_else(|| name(&self.file_name)) };
        title.unwrap_or_else(|| DEFAULT_TITLE.to_string())
    }

    pub fn view(&self, now: Micros) -> AnalysisPanelVm {
        let has_data = self.has_data();
        let stale = self.is_stale(now);
        let state = match &self.phase {
            Phase::Idle => PanelState::Idle,
            Phase::Checking { reconcile: false } => PanelState::CheckingCache,
            Phase::Checking { reconcile: true } | Phase::Starting { .. } => PanelState::Loading,
            Phase::Watching if stale => PanelState::Interrupted,
            Phase::Watching => PanelState::Loading,
            Phase::Completed => PanelState::Ready,
            Phase::Interrupted => PanelState::Interrupted,
            Phase::Cancelled => PanelState::Cancelled,
            Phase::Failed { .. } => PanelState::Error,
        };
        let (error_detail, error_hint) = match &self.phase {
            Phase::Failed { unknown_key: Some(_), .. } => (None, Some(app("The AI request could not be confirmed. Check your connection and retry."))),
            Phase::Failed { detail, .. } => (detail.clone(), None),
            _ => (None, None),
        };
        AnalysisPanelVm {
            state,
            no_transcript: state == PanelState::Idle && !self.has_lines,
            detected: self.progress.detected,
            summary: self.progress.summary.clone(),
            analysis: self.progress.analysis.clone().map(displayed),
            has_data,
            progress: if state == PanelState::Loading { Some(self.progress_vm(now, has_data)) } else { None },
            can_retry: matches!(state, PanelState::Error | PanelState::Interrupted),
            error_detail,
            error_hint,
            export_busy: self.export_busy,
            export_error: self.export_error.clone(),
        }
    }

    fn progress_vm(&self, now: Micros, has_data: bool) -> ProgressVm {
        if self.saw_events {
            let received = sections(&self.progress).len();
            return match self.progress.expected_content_sections.filter(|t| *t > 0) {
                Some(total) => ProgressVm {
                    stage: ai("Analyzing ({{received}}/{{total}} sections)").number("received", count(received)).number("total", count(total)),
                    percentage: percent(received, total),
                },
                None => ProgressVm { stage: ai("Analyzing ({{received}} sections)").number("received", count(received)), percentage: 10 },
            };
        }
        match &self.resumed {
            Some(_) if has_data => ProgressVm { stage: ai("Analysis in progress… (you may have had another tab open)"), percentage: 50 },
            Some(view) if staleness(view, now) == Staleness::Warning => {
                ProgressVm { stage: ai("Analysis in progress… taking longer than expected"), percentage: 30 }
            }
            Some(_) => ProgressVm { stage: ai("Analysis in progress…"), percentage: 10 },
            None if self.attached_existing => ProgressVm { stage: ai("Analysis in progress…"), percentage: 10 },
            None => ProgressVm { stage: ai("Starting..."), percentage: 0 },
        }
    }

    /// AI-008: a resumed generation five minutes past creation with no new
    /// event is presented as interrupted; the server's recovery fences it.
    fn is_stale(&self, now: Micros) -> bool {
        !self.saw_events && self.resumed.as_ref().is_some_and(|v| staleness(v, now) == Staleness::Stale)
    }

    fn has_data(&self) -> bool {
        !sections(&self.progress).is_empty()
    }

    fn check(&mut self) -> Vec<Effect> {
        let Some(job) = self.job.filter(|_| self.consent && !self.checked) else { return vec![] };
        self.checked = true;
        self.phase = Phase::Checking { reconcile: false };
        vec![Effect::LoadAnalysis { job }]
    }

    fn loaded(&mut self, reconcile: bool, result: Result<Option<GenerationView>, ApiFailure>) -> Vec<Effect> {
        let view = match result {
            Ok(view) => view,
            Err(_) if reconcile => {
                self.phase = Phase::Interrupted;
                return vec![];
            }
            Err(_) => None,
        };
        let Some(view) = view.filter(|v| Some(v.job) == self.job) else {
            if reconcile {
                self.phase = Phase::Interrupted;
                return vec![];
            }
            self.phase = Phase::Idle;
            return self.maybe_start();
        };
        let progress = match (&view.final_content, &view.draft) {
            (Some(FinalContent::Analysis { result }), _) => Some(AnalysisProgress {
                detected: Some(result.detected),
                summary: Some(result.summary.clone()),
                analysis: Some(result.analysis.clone()),
                content_sections: 0,
                expected_content_sections: None,
                warnings: result.warnings.clone(),
            }),
            (_, Draft::Analysis { progress }) => Some(progress.as_ref().clone()),
            _ => None,
        };
        match view.status {
            GenerationStatus::Completed => match progress.filter(|p| !sections(p).is_empty()) {
                Some(p) => {
                    self.progress = p;
                    self.generation = Some(view.id);
                    self.phase = Phase::Completed;
                    vec![]
                }
                // AI-007: a completed row without content is a cache miss.
                None if reconcile => {
                    self.phase = Phase::Interrupted;
                    vec![]
                }
                None => {
                    self.phase = Phase::Idle;
                    self.maybe_start()
                }
            },
            GenerationStatus::Accepted | GenerationStatus::Running => {
                self.progress = progress.unwrap_or_default();
                let id = view.id;
                let mut resumed = view;
                resumed.draft = Draft::Empty;
                resumed.final_content = None;
                self.resumed = Some(resumed);
                self.saw_events = false;
                self.generation = Some(id);
                self.phase = Phase::Watching;
                vec![Effect::Watch { generation: id, kind: WatchKind::Analysis, after: None }]
            }
            GenerationStatus::Interrupted | GenerationStatus::Cancelled => {
                self.progress = progress.unwrap_or_default();
                self.generation = Some(view.id);
                self.phase = if view.status == GenerationStatus::Cancelled { Phase::Cancelled } else { Phase::Interrupted };
                vec![]
            }
        }
    }

    fn maybe_start(&mut self) -> Vec<Effect> {
        let Some(key) = self.armed else { return vec![] };
        if self.triggered || !self.consent || !self.has_lines || self.job.is_none() || self.phase != Phase::Idle {
            return vec![];
        }
        self.triggered = true;
        self.armed = None;
        self.phase = Phase::Starting { key, replayed: false };
        vec![self.start_effect(key)]
    }

    fn retry(&mut self, key: RequestKey) -> Vec<Effect> {
        let now_stale = matches!(self.phase, Phase::Watching) && self.resumed.is_some() && !self.saw_events;
        let reuse = match &self.phase {
            Phase::Failed { unknown_key, .. } => *unknown_key,
            Phase::Interrupted => None,
            // A stale resumed generation is presented as interrupted.
            Phase::Watching if now_stale => None,
            _ => return vec![],
        };
        if self.job.is_none() || !self.consent {
            return vec![];
        }
        let mut effects = if matches!(self.phase, Phase::Watching) { self.detach() } else { vec![] };
        let key = reuse.unwrap_or(key);
        self.clear_content();
        self.phase = Phase::Starting { key, replayed: reuse.is_some() };
        effects.push(self.start_effect(key));
        effects
    }

    fn start_effect(&self, key: RequestKey) -> Effect {
        Effect::StartAnalysis { job: self.job.expect("a start needs a selected job"), key }
    }

    fn watch(&mut self, generation: GenerationId) -> Vec<Effect> {
        self.generation = Some(generation);
        self.phase = Phase::Watching;
        self.saw_events = false;
        self.reconnected = false;
        vec![Effect::Watch { generation, kind: WatchKind::Analysis, after: None }]
    }

    fn detach(&mut self) -> Vec<Effect> {
        match (&self.phase, self.generation) {
            (Phase::Watching | Phase::Checking { reconcile: true }, Some(generation)) => vec![Effect::Detach { generation }],
            _ => vec![],
        }
    }

    fn clear_content(&mut self) {
        self.progress = AnalysisProgress::default();
        self.generation = None;
        self.resumed = None;
        self.attached_existing = false;
        self.saw_events = false;
        self.reconnected = false;
    }

    fn apply_wire(&mut self, wire: AnalysisWire) {
        match wire {
            AnalysisWire::SectionCount { total } => self.progress.expected_content_sections = Some(total),
            AnalysisWire::TypeDetected { kind, confidence } => {
                self.progress.detected = Some(DetectedType { kind, confidence });
                if self.progress.analysis.as_ref().is_none_or(|a| kind_of(a) != kind) {
                    self.progress.analysis = Some(empty(kind));
                }
                self.mirror_generic_summary();
            }
            AnalysisWire::SectionReady { section_type, data } => {
                apply_section(&mut self.progress, &section_type, data);
                self.mirror_generic_summary();
                self.progress.content_sections = sections(&self.progress).len();
            }
            AnalysisWire::Complete { .. } => self.phase = Phase::Completed,
            AnalysisWire::Error { code, .. } => {
                self.phase = if code == "ANALYSIS_CANCELLED" { Phase::Cancelled } else { Phase::Interrupted };
            }
        }
    }

    fn mirror_generic_summary(&mut self) {
        if let (Some(Analysis::Generic(g)), Some(summary)) = (self.progress.analysis.as_mut(), self.progress.summary.as_ref()) {
            g.summary = Some(summary.clone());
        }
    }

    fn capture(&self, at: Micros, utc_offset_minutes: i16, phone: bool) -> Option<AnalysisDocument> {
        if !self.has_data() {
            return None;
        }
        let status = match self.phase {
            Phase::Completed => GenerationStatus::Completed,
            Phase::Interrupted | Phase::Failed { .. } => GenerationStatus::Interrupted,
            Phase::Cancelled => GenerationStatus::Cancelled,
            Phase::Watching if self.resumed.as_ref().is_some_and(|v| v.status == GenerationStatus::Accepted) && !self.saw_events => {
                GenerationStatus::Accepted
            }
            _ => GenerationStatus::Running,
        };
        let metadata = AnalysisMetadata { title: self.export_title(phone), job_id: self.job, exported_at: at, utc_offset_minutes };
        AnalysisDocument::capture(&self.progress, status, metadata).ok()
    }
}

/// What the tab renders: the source sorts interview next steps by priority
/// before display (`InterviewNextSteps`); export keeps the captured order.
fn displayed(analysis: Analysis) -> Analysis {
    match analysis {
        Analysis::Interview(mut v) => {
            v.next_steps = v.next_steps.as_deref().map(by_priority);
            Analysis::Interview(v)
        }
        other => other,
    }
}

fn export_failed(format: DocumentFormat) -> Msg {
    match format {
        DocumentFormat::Docx => app("Failed to export as DOCX"),
        DocumentFormat::Pdf => app("Failed to export as PDF"),
        DocumentFormat::Txt | DocumentFormat::Md => app("Export failed"),
    }
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The source's `round(received / total * 90) + 10`, bounded for display.
fn percent(received: usize, total: usize) -> u8 {
    let value = (received as f64 / total as f64 * 90.0).round() + 10.0;
    value.min(100.0) as u8
}

fn kind_of(analysis: &Analysis) -> AnalysisType {
    match analysis {
        Analysis::Generic(_) => AnalysisType::Generic,
        Analysis::Lecture(_) => AnalysisType::Lecture,
        Analysis::Meeting(_) => AnalysisType::Meeting,
        Analysis::Sales(_) => AnalysisType::Sales,
        Analysis::Support(_) => AnalysisType::Support,
        Analysis::Interview(_) => AnalysisType::Interview,
    }
}

fn empty(kind: AnalysisType) -> Analysis {
    match kind {
        AnalysisType::Generic => Analysis::Generic(GenericAnalysis::default()),
        AnalysisType::Lecture => Analysis::Lecture(LectureAnalysis::default()),
        AnalysisType::Meeting => Analysis::Meeting(MeetingAnalysis::default()),
        AnalysisType::Sales => Analysis::Sales(SalesAnalysis::default()),
        AnalysisType::Support => Analysis::Support(SupportAnalysis::default()),
        AnalysisType::Interview => Analysis::Interview(InterviewAnalysis::default()),
    }
}

/// The inverse of `crate::ai::analysis::sections` for one `sectionReady`: the
/// same names, typed by the detected shape. An undecodable or unknown section
/// changes nothing; a repeat replaces the same field.
fn apply_section(progress: &mut AnalysisProgress, name: &str, data: Value) {
    if name == "summary" {
        if let Value::String(summary) = data {
            progress.summary = Some(summary);
        }
        return;
    }
    let Some(analysis) = progress.analysis.as_mut() else { return };
    macro_rules! set {
        ($field:expr) => {
            if let Ok(value) = serde_json::from_value(data) {
                $field = Some(value);
            }
        };
    }
    match (analysis, name) {
        (Analysis::Meeting(m), "actionItems") => set!(m.action_items),
        (Analysis::Meeting(m), "decisions") => set!(m.decisions),
        (Analysis::Meeting(m), "discussionPoints") => set!(m.discussion_points),
        (Analysis::Meeting(m), "nextSteps") => set!(m.next_steps),
        (Analysis::Lecture(l), "topics") => set!(l.topics),
        (Analysis::Lecture(l), "concepts") => set!(l.concepts),
        (Analysis::Lecture(l), "reviewQuestions") => set!(l.review_questions),
        (Analysis::Lecture(l), "studyGuide") => set!(l.study_guide),
        (Analysis::Sales(s), "callMetrics") => set!(s.call_metrics),
        (Analysis::Sales(s), "coachingTips") => set!(s.coaching_tips),
        (Analysis::Sales(s), "customerInsights") => set!(s.customer_insights),
        (Analysis::Sales(s), "nextSteps") => set!(s.next_steps),
        (Analysis::Support(s), "issueResolution") => set!(s.issue_resolution),
        (Analysis::Support(s), "sentiment") => set!(s.sentiment),
        (Analysis::Support(s), "insights") => set!(s.insights),
        (Analysis::Support(s), "kbUpdates") => set!(s.kb_updates),
        (Analysis::Support(s), "followUpActions") => set!(s.follow_up_actions),
        (Analysis::Interview(i), "assessment") => set!(i.assessment),
        (Analysis::Interview(i), "skillsAssessment") => set!(i.skills_assessment),
        (Analysis::Interview(i), "notableResponses") => set!(i.notable_responses),
        (Analysis::Interview(i), "nextSteps") => set!(i.next_steps),
        (Analysis::Generic(g), "highlights") => set!(g.highlights),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
