//! Shared presentation of generations: the application frame a fresh or
//! reconnecting attachment starts from, the source analysis event stream
//! derived from typed progress, and the AI-008 staleness labels. Shells render
//! these; they never re-derive product meaning.

use llm_core::{ApplicationFrame, Persistence, PublicError, StreamIdentity, StreamStatus, TextEvent};
use serde_json::Value;

use crate::ai::analysis::{AnalysisProgress, AnalysisType};
use crate::ai::ports::{Draft, FinalContent, GenerationKind, GenerationStatus, Interruption};
use crate::ai::GenerationView;
use crate::ids::{MICROS_PER_MINUTE, Micros};

/// The public code and message of an interruption.
pub fn public_error(reason: &Interruption) -> PublicError {
    let (code, message) = match reason {
        Interruption::Provider { message } => ("provider", message.clone()),
        Interruption::Unsuccessful { reason } => ("unsuccessful", format!("The model stopped early ({reason})")),
        Interruption::Malformed { message } => ("malformed", message.clone()),
        Interruption::Limit { message } => ("limit", message.clone()),
        Interruption::Deadline => ("deadline", "The AI request took too long".into()),
        Interruption::Revoked => ("revoked", "AI consent was withdrawn".into()),
        Interruption::SourceChanged => ("source_changed", "The transcript changed".into()),
        Interruption::Erased => ("erased", "The account is being erased".into()),
        Interruption::WorkerLost => ("interrupted", "The AI request was interrupted".into()),
        Interruption::NotSaved { message } => ("not_saved", message.clone()),
    };
    PublicError { code: code.into(), message }
}

impl GenerationView {
    /// The authoritative replacement frame of a text generation (chat answer or
    /// completion summary) for a fresh, reconnecting or other-replica
    /// attachment. Terminal views keep their retained partial text; the
    /// detailed reason stays in the view. `None` for analysis (see
    /// `analysis_events`) and before any execution claimed it.
    pub fn frame(&self) -> Option<ApplicationFrame> {
        if self.kind == GenerationKind::Analysis {
            return None;
        }
        let execution = self.execution?;
        let identity = StreamIdentity {
            generation: self.id.to_string(),
            content_revision: u64::try_from(self.source_revision).unwrap_or(0),
            consent_epoch: u64::try_from(self.consent_epoch).unwrap_or(0),
            execution: execution.to_string(),
        };
        let draft = match &self.draft {
            Draft::Text { text } => text.clone(),
            _ => String::new(),
        };
        let event = match self.status {
            GenerationStatus::Completed => match &self.final_content {
                Some(FinalContent::Text { text }) => TextEvent::Final { text: text.clone(), persistence: Persistence::Saved },
                _ => return None,
            },
            GenerationStatus::Accepted | GenerationStatus::Running => {
                TextEvent::Snapshot { text: draft, status: StreamStatus::Streaming, persistence: Persistence::Provisional }
            }
            GenerationStatus::Interrupted => {
                let status = if self.interruption == Some(Interruption::Revoked) { StreamStatus::Revoked } else { StreamStatus::Interrupted };
                TextEvent::Snapshot { text: draft, status, persistence: Persistence::Saved }
            }
            GenerationStatus::Cancelled => TextEvent::Snapshot { text: draft, status: StreamStatus::Cancelled, persistence: Persistence::Saved },
        };
        Some(ApplicationFrame { identity, seq: u64::try_from(self.sequence).unwrap_or(0), event })
    }
}

/// One source analysis SSE event (AI-003): `sectionCount{total}`,
/// `typeDetected{type, confidence}`, `sectionReady{sectionType, data}`,
/// `complete{tokensUsed, processingTime}` and `error{code, message}`.
#[derive(Clone, Debug, PartialEq)]
pub enum AnalysisEvent {
    /// The advisory content-section total (D1: `max(documentCount - 2, 0)`).
    SectionCount { total: usize },
    TypeDetected { kind: AnalysisType, confidence: f64 },
    SectionReady { section_type: &'static str, data: Value },
    Complete { tokens_used: i64, processing_time_ms: i64 },
    Error { code: String, message: String },
}

fn progress_of(view: &GenerationView) -> Option<AnalysisProgress> {
    match (&view.final_content, &view.draft) {
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
    }
}

/// The events that move a client from `previous` (or nothing) to `current`,
/// in source order: count, detected type, each newly validated section, then
/// the terminal event. Sections are compared by source type name.
pub fn analysis_events(previous: Option<&GenerationView>, current: &GenerationView) -> Vec<AnalysisEvent> {
    let mut events = Vec::new();
    if current.kind != GenerationKind::Analysis {
        return events;
    }
    let before = previous.filter(|p| p.id == current.id).and_then(progress_of);
    let now = progress_of(current);
    if let Some(p) = &now {
        let known_total = before.as_ref().and_then(|b| b.expected_content_sections);
        if let Some(total) = p.expected_content_sections.filter(|t| Some(*t) != known_total) {
            events.push(AnalysisEvent::SectionCount { total });
        }
        if let Some(d) = p.detected.filter(|_| before.as_ref().is_none_or(|b| b.detected.is_none())) {
            events.push(AnalysisEvent::TypeDetected { kind: d.kind, confidence: d.confidence });
        }
        let seen: Vec<&'static str> = before.as_ref().map(|b| crate::ai::analysis::sections(b).into_iter().map(|(name, _)| name).collect()).unwrap_or_default();
        for (name, data) in crate::ai::analysis::sections(p) {
            if !seen.contains(&name) {
                events.push(AnalysisEvent::SectionReady { section_type: name, data });
            }
        }
    }
    let was_terminal = previous.filter(|p| p.id == current.id).is_some_and(|p| p.status.is_terminal());
    if !was_terminal {
        match current.status {
            GenerationStatus::Completed => {
                let processing_time_ms = current.finished_at.map_or(0, |f| (f.0 - current.created_at.0) / 1000);
                events.push(AnalysisEvent::Complete { tokens_used: current.tokens_used.unwrap_or(0), processing_time_ms });
            }
            GenerationStatus::Interrupted => {
                let e = current.interruption.as_ref().map(public_error);
                events.push(AnalysisEvent::Error {
                    code: "ANALYSIS_FAILED".into(),
                    message: e.map_or_else(|| "Analysis failed".into(), |e| e.message),
                });
            }
            GenerationStatus::Cancelled => events.push(AnalysisEvent::Error { code: "ANALYSIS_CANCELLED".into(), message: "Analysis cancelled".into() }),
            GenerationStatus::Accepted | GenerationStatus::Running => {}
        }
    }
    events
}

/// AI-008 labels for a non-terminal generation, by age from creation: an
/// accepted one warns after one minute; either is stale after five (recovery
/// interrupts it once its worker lease expires).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staleness {
    Fresh,
    Warning,
    Stale,
}

pub fn staleness(view: &GenerationView, now: Micros) -> Staleness {
    if view.status.is_terminal() {
        return Staleness::Fresh;
    }
    let age = now.0 - view.created_at.0;
    if age >= 5 * MICROS_PER_MINUTE {
        Staleness::Stale
    } else if view.status == GenerationStatus::Accepted && age > MICROS_PER_MINUTE {
        Staleness::Warning
    } else {
        Staleness::Fresh
    }
}
