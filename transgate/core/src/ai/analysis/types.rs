//! Port of frontend/src/lib/ai/schemas.ts. Missing optional fields stay absent;
//! explicit null is not a source `optional()` value. Unknown object keys are ignored.
use serde::{Deserialize, Deserializer, Serialize};

fn optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where D: Deserializer<'de>, T: Deserialize<'de> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum AnalysisType { Generic, Lecture, Meeting, Sales, Support, Interview }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Analysis {
    Generic(GenericAnalysis),
    Lecture(LectureAnalysis),
    Meeting(MeetingAnalysis),
    Sales(SalesAnalysis),
    Support(SupportAnalysis),
    Interview(InterviewAnalysis),
}

impl Analysis {
    pub fn kind(&self) -> AnalysisType {
        match self {
            Self::Generic(_) => AnalysisType::Generic,
            Self::Lecture(_) => AnalysisType::Lecture,
            Self::Meeting(_) => AnalysisType::Meeting,
            Self::Sales(_) => AnalysisType::Sales,
            Self::Support(_) => AnalysisType::Support,
            Self::Interview(_) => AnalysisType::Interview,
        }
    }
    pub(super) fn empty(kind: AnalysisType) -> Self {
        match kind {
            AnalysisType::Generic => Self::Generic(GenericAnalysis::default()),
            AnalysisType::Lecture => Self::Lecture(LectureAnalysis::default()),
            AnalysisType::Meeting => Self::Meeting(MeetingAnalysis::default()),
            AnalysisType::Sales => Self::Sales(SalesAnalysis::default()),
            AnalysisType::Support => Self::Support(SupportAnalysis::default()),
            AnalysisType::Interview => Self::Interview(InterviewAnalysis::default()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus { #[default] Pending, InProgress, Done }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub task: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    #[serde(default)]
    pub status: ActionStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "AnalysisDecision", optional_fields))]
pub struct Decision {
    pub decision: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct MeetingAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub action_items: Option<Vec<ActionItem>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub decisions: Option<Vec<Decision>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub discussion_points: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub next_steps: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct Topic {
    pub title: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct Concept {
    pub concept: String,
    pub explanation: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct LectureAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Topic>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub concepts: Option<Vec<Concept>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub review_questions: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub study_guide: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct CallMetrics {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub talk_listen_ratio: Option<String>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub questions_asked: Option<f64>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub objections_handled: Option<f64>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub next_steps_confirmed: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum CoachingCategory { Strength, Improvement }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CoachingTip { pub category: CoachingCategory, pub tip: String }

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct CustomerInsights {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub pain_points: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub budget_signals: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub objections: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub buying_signals: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SalesAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub call_metrics: Option<CallMetrics>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub coaching_tips: Option<Vec<CoachingTip>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub customer_insights: Option<CustomerInsights>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub next_steps: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ResolutionStatus { Resolved, Unresolved, Escalated }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct IssueResolution {
    pub problem: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub root_cause: Option<String>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    pub status: ResolutionStatus,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub resolution_time_minutes: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SentimentKind { Frustrated, Neutral, Satisfied, Relieved }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct SentimentMoment {
    pub timestamp: f64,
    pub sentiment: SentimentKind,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Sentiment {
    pub initial: String,
    pub r#final: String,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub key_moments: Option<Vec<SentimentMoment>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct SupportInsights {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub strengths: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub improvements: Option<Vec<String>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SupportAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub issue_resolution: Option<IssueResolution>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<Sentiment>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub insights: Option<SupportInsights>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub kb_updates: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub follow_up_actions: Option<Vec<String>>,
}

/// Source ratings are numbers in [1, 5], not integers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "AnalysisRating", type = "number"))]
#[serde(try_from = "f64", into = "f64")]
pub struct Rating(f64);
impl Rating { pub fn value(self) -> f64 { self.0 } }
impl TryFrom<f64> for Rating {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value.is_finite() && (1.0..=5.0).contains(&value) { Ok(Self(value)) }
        else { Err("rating must be between 1 and 5") }
    }
}
impl From<Rating> for f64 { fn from(value: Rating) -> Self { value.0 } }

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub overall_rating: Option<Rating>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub strengths: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub concerns: Option<Vec<String>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SkillLevel { Beginner, Intermediate, Advanced, Expert }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SkillAssessment { pub skill: String, pub rating: Rating, pub level: SkillLevel }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ResponseAssessment { Positive, Neutral, Negative }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotableResponse {
    pub question: String,
    pub answer: String,
    pub assessment: ResponseAssessment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Priority { High, Medium, Low }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InterviewNextStep { pub action: String, pub priority: Priority }
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct InterviewAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub assessment: Option<Assessment>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub skills_assessment: Option<Vec<SkillAssessment>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub notable_responses: Option<Vec<NotableResponse>>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub next_steps: Option<Vec<InterviewNextStep>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct GenericAnalysis {
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "optional", skip_serializing_if = "Option::is_none")]
    pub highlights: Option<Vec<String>>,
}
