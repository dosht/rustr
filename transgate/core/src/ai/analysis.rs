//! Source six-shape analysis contract and incremental multi-document decoder.
//! Successful parsing is not persistence: the AI use case commits before success delivery.

mod types;
mod parser;
pub use types::*;
pub use parser::AnalysisParser;

use serde::{Deserialize, Serialize};

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
pub const MAX_JSON_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DetectedType {
    pub kind: AnalysisType,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisResult {
    pub detected: DetectedType,
    /// The common summary is retained for every specialized shape.
    pub summary: String,
    pub analysis: Analysis,
    pub warnings: Vec<AnalysisWarning>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisProgress {
    pub detected: Option<DetectedType>,
    pub summary: Option<String>,
    pub analysis: Option<Analysis>,
    /// Unique validated content sections, including the common summary.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub content_sections: usize,
    /// Advisory `max(documentCount - 2, 0)`, never a reason to invent content.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub expected_content_sections: Option<usize>,
    pub warnings: Vec<AnalysisWarning>,
}

/// Source `sectionReady` values for a validated snapshot, in schema order:
/// common summary first, then the detected type's optional fields. Arrival
/// order is not persisted. Missing fields are omitted; present empty arrays or
/// objects remain present. Generic's mirrored summary is emitted only once.
pub fn sections(progress: &AnalysisProgress) -> Vec<(&'static str, serde_json::Value)> {
    let mut result = Vec::with_capacity(progress.content_sections.min(6));
    macro_rules! section {
        ($name:literal, $value:expr) => {
            if let Some(value) = $value {
                result.push(($name, serde_json::to_value(value)
                    .expect("validated closed analysis types serialize to JSON")));
            }
        };
    }
    section!("summary", progress.summary.as_ref());
    match progress.analysis.as_ref() {
        Some(Analysis::Meeting(m)) => {
            section!("actionItems", m.action_items.as_ref());
            section!("decisions", m.decisions.as_ref());
            section!("discussionPoints", m.discussion_points.as_ref());
            section!("nextSteps", m.next_steps.as_ref());
        }
        Some(Analysis::Lecture(l)) => {
            section!("topics", l.topics.as_ref());
            section!("concepts", l.concepts.as_ref());
            section!("reviewQuestions", l.review_questions.as_ref());
            section!("studyGuide", l.study_guide.as_ref());
        }
        Some(Analysis::Sales(s)) => {
            section!("callMetrics", s.call_metrics.as_ref());
            section!("coachingTips", s.coaching_tips.as_ref());
            section!("customerInsights", s.customer_insights.as_ref());
            section!("nextSteps", s.next_steps.as_ref());
        }
        Some(Analysis::Support(s)) => {
            section!("issueResolution", s.issue_resolution.as_ref());
            section!("sentiment", s.sentiment.as_ref());
            section!("insights", s.insights.as_ref());
            section!("kbUpdates", s.kb_updates.as_ref());
            section!("followUpActions", s.follow_up_actions.as_ref());
        }
        Some(Analysis::Interview(i)) => {
            section!("assessment", i.assessment.as_ref());
            section!("skillsAssessment", i.skills_assessment.as_ref());
            section!("notableResponses", i.notable_responses.as_ref());
            section!("nextSteps", i.next_steps.as_ref());
        }
        Some(Analysis::Generic(g)) => {
            section!("highlights", g.highlights.as_ref());
        }
        None => {}
    }
    result
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum AnalysisWarning {
    DocumentCountMismatch {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expected: usize,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        actual: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum AnalysisError {
    OutputTooLarge,
    DocumentTooLarge,
    TooDeep,
    MalformedDocument,
    InvalidSection,
    ConflictingDuplicate,
    MissingDetectedType,
    MissingSummary,
    UnsuccessfulFinish,
    AlreadyFinished,
}

/// Serializable caller-owned failure payload, not the parser's Result error.
/// Construct with the small finish error and parser.into_progress().
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisFailure {
    pub error: AnalysisError,
    pub partial: AnalysisProgress,
}

impl AnalysisError {
    pub fn message(self) -> &'static str {
        match self {
            Self::OutputTooLarge => "Analysis output exceeds the UTF-8 byte limit.",
            Self::DocumentTooLarge => "Analysis document exceeds the UTF-8 byte limit.",
            Self::TooDeep => "Analysis JSON exceeds the nesting limit.",
            Self::MalformedDocument => "Analysis contains a malformed or incomplete JSON document.",
            Self::InvalidSection => "Analysis section does not match the detected type's schema.",
            Self::ConflictingDuplicate => "Analysis contains conflicting duplicate sections.",
            Self::MissingDetectedType => "Analysis has no usable detected type.",
            Self::MissingSummary => "Analysis has no usable summary.",
            Self::UnsuccessfulFinish => "Analysis did not finish with a successful provider stop.",
            Self::AlreadyFinished => "Analysis parser has already finished.",
        }
    }
}
impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message()) }
}
impl std::error::Error for AnalysisError {}

impl AnalysisFailure {
    pub fn message(&self) -> &'static str { self.error.message() }
}
impl std::fmt::Display for AnalysisFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message()) }
}
impl std::error::Error for AnalysisFailure {}

#[cfg(test)]
#[path = "analysis/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "analysis/sanitize_tests.rs"]
mod sanitize_tests;
