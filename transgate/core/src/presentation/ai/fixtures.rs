//! Named AI panel states for Storybook and component tests. Each one is
//! reached through the panel's own `update` from synthetic answers, so a story
//! can only show a state the presenter can produce. p1X folds these into
//! `presentation::fixtures::export` under the VM type names.

use serde_json::{Value, json};

use crate::ai::analysis::{Analysis, AnalysisResult, AnalysisType, DetectedType};
use crate::ai::ports::Period;
use crate::ai::{
    ChatHistory, ChatMessage, ChatRole, ConsentView, Draft, FinalContent, GenerationId, GenerationKind, GenerationStatus, GenerationView,
    InFlightChat,
    Interruption, LimitUsage, LimitsView,
};
use crate::documents::DocumentFormat;
use crate::ids::{AccountId, JobId, MICROS_PER_MINUTE, Micros};
use crate::presentation::ai::analysis::{self, AnalysisPanelState, AnalysisPanelVm, ExportFailure};
use crate::presentation::ai::chat::{self, ChatPanelState, ChatPanelVm};
use crate::presentation::ai::consent::{self, ConsentPanelState, ConsentPanelVm};
use crate::presentation::ai::{AnalysisWire, ApiFailure, StartRejection, WatchFailure};

/// The instant every fixture is viewed at.
pub const NOW: Micros = Micros(1_767_225_600_000_000);
const JOB: JobId = JobId(42);
const HOUR: i64 = 60 * MICROS_PER_MINUTE;

fn key(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(0xf1_0000 + n)
}

fn generation(n: u128) -> GenerationId {
    GenerationId(uuid::Uuid::from_u128(0xf2_0000 + n))
}

fn consent_view(granted: bool) -> ConsentView {
    ConsentView {
        has_consented: granted,
        consent_version: "1.0".into(),
        consent_date: granted.then_some(Micros(NOW.0 - 30 * 24 * HOUR)),
        revoked_date: None,
        privacy_policy_url: consent::DEFAULT_PRIVACY_POLICY_URL.into(),
        provider_name: "deepseek".into(),
        can_revoke: granted,
        cleanup_pending: false,
    }
}

pub fn consent() -> Vec<(&'static str, ConsentPanelVm)> {
    let loading = {
        let mut s = ConsentPanelState::default();
        s.update(consent::Event::Opened);
        s
    };
    let loaded = |granted: bool| {
        let mut s = loading.clone();
        s.update(consent::Event::Loaded { result: Ok(consent_view(granted)) });
        s
    };
    let enabling = {
        let mut s = loaded(false);
        s.update(consent::Event::EnableRequested);
        s
    };
    let disable_failed = {
        let mut s = loaded(true);
        s.update(consent::Event::DisableConfirmed);
        s.update(consent::Event::DecisionSaved { result: Err(ApiFailure::http(500, Some("internal"))) });
        s
    };
    let cleanup_pending = {
        let mut s = loaded(true);
        s.update(consent::Event::DisableConfirmed);
        s.update(consent::Event::DecisionSaved {
            result: Ok(ConsentView { has_consented: false, can_revoke: false, cleanup_pending: true, revoked_date: Some(NOW), ..consent_view(true) }),
        });
        s
    };
    let load_failed = {
        let mut s = loading.clone();
        s.update(consent::Event::Loaded { result: Err(ApiFailure::network()) });
        s
    };
    vec![
        ("loading", loading.view()),
        ("required", loaded(false).view()),
        ("granted", loaded(true).view()),
        ("enabling", enabling.view()),
        ("disable_failed", disable_failed.view()),
        ("cleanup_pending", cleanup_pending.view()),
        ("load_failed", load_failed.view()),
    ]
}

fn analysis_view(id: GenerationId, status: GenerationStatus, final_content: Option<FinalContent>) -> GenerationView {
    GenerationView {
        id,
        job: JOB,
        kind: GenerationKind::Analysis,
        status,
        sequence: 1,
        source_revision: 1,
        consent_epoch: 1,
        draft: Draft::Empty,
        final_content,
        durable: true,
        remote_outcome_unknown: false,
        execution: None,
        tokens_used: Some(1200),
        interruption: None,
        created_at: Micros(NOW.0 - HOUR),
        finished_at: Some(Micros(NOW.0 - HOUR + 30_000_000)),
    }
}

/// One synthetic, schema-valid result per specialized shape.
fn shape(kind: AnalysisType) -> (&'static str, Value) {
    match kind {
        AnalysisType::Generic => ("A wide-ranging conversation about the product roadmap.", json!({"type": "generic",
            "highlights": ["Mobile release moves to May", "Pricing review next quarter"]})),
        AnalysisType::Meeting => ("The team agreed the Q2 plan and assigned owners.", json!({"type": "meeting",
            "actionItems": [
                {"task": "Draft the launch memo", "owner": "Ada", "deadline": "Friday", "status": "in-progress"},
                {"task": "Book the venue", "status": "done"},
                {"task": "Collect feedback"}
            ],
            "decisions": [{"decision": "Ship the beta on 1 May", "timestamp": 754, "participants": ["Ada", "Lin"]}],
            "discussionPoints": ["Hiring plan", "Support load"],
            "nextSteps": ["Share the memo", "Schedule a retro"]})),
        AnalysisType::Lecture => ("An introduction to thermodynamics.", json!({"type": "lecture",
            "topics": [{"title": "Energy", "timestamp": 0, "duration": 420}, {"title": "Entropy", "timestamp": 420, "duration": 45}],
            "concepts": [{"concept": "Entropy", "explanation": "A measure of disorder.", "examples": ["Melting ice"]}],
            "reviewQuestions": ["What is the first law?"],
            "studyGuide": ["Re-read chapter 2"]})),
        AnalysisType::Sales => ("A discovery call with a mid-size retailer.", json!({"type": "sales",
            "callMetrics": {"talkListenRatio": "40:60", "questionsAsked": 12, "objectionsHandled": 2, "nextStepsConfirmed": true},
            "coachingTips": [{"category": "strength", "tip": "Clear agenda"}, {"category": "improvement", "tip": "Ask about budget earlier"}],
            "customerInsights": {"painPoints": ["Manual reporting"], "budgetSignals": ["Q3 budget approved"], "objections": ["Migration effort"], "buyingSignals": ["Asked for a trial"]},
            "nextSteps": ["Send the proposal"]})),
        AnalysisType::Support => ("A billing issue resolved on the call.", json!({"type": "support",
            "issueResolution": {"problem": "Double charge", "rootCause": "Retry bug", "resolution": "Refund issued", "status": "resolved", "resolutionTimeMinutes": 12},
            "sentiment": {"initial": "frustrated", "final": "satisfied", "keyMoments": [{"timestamp": 95, "sentiment": "relieved", "reason": "Refund confirmed"}]},
            "insights": {"strengths": ["Empathy"], "improvements": ["Confirm the account earlier"]},
            "kbUpdates": ["Document the refund path"],
            "followUpActions": ["Email the receipt"]})),
        AnalysisType::Interview => ("A senior engineer interview.", json!({"type": "interview",
            "assessment": {"position": "Senior Engineer", "overallRating": 4, "strengths": ["System design"], "concerns": ["Limited Rust"]},
            "skillsAssessment": [{"skill": "Distributed systems", "rating": 4.5, "level": "Expert"}],
            "notableResponses": [{"question": "Hardest bug?", "answer": "A clock-skew race.", "assessment": "positive"}],
            "nextSteps": [{"action": "Team interview", "priority": "high"}]})),
    }
}

fn granted_analysis(has_lines: bool) -> AnalysisPanelState {
    let mut s = AnalysisPanelState::default();
    s.update(analysis::Event::ConsentChanged { granted: true });
    s.update(analysis::Event::Selected { job: JOB, job_name: Some("Weekly sync".into()), file_name: Some("weekly-sync.mp3".into()), has_lines });
    s
}

fn completed(kind: AnalysisType) -> AnalysisPanelState {
    let (summary, value) = shape(kind);
    let analysis: Analysis = serde_json::from_value(value).expect("fixture shapes are schema-valid");
    let result = AnalysisResult { detected: DetectedType { kind, confidence: 91.0 }, summary: summary.into(), analysis, warnings: vec![] };
    let mut s = granted_analysis(true);
    s.update(analysis::Event::AnalysisLoaded {
        job: JOB,
        result: Ok(Some(analysis_view(generation(kind as u128), GenerationStatus::Completed, Some(FinalContent::Analysis { result: Box::new(result) })))),
    });
    s
}

/// Started and streaming: detected meeting, summary and one section of four.
fn streaming() -> AnalysisPanelState {
    let mut s = granted_analysis(true);
    s.update(analysis::Event::AnalysisLoaded { job: JOB, result: Ok(None) });
    s.update(analysis::Event::Shown { key: key(1) });
    let g = generation(100);
    s.update(analysis::Event::StartAccepted { key: key(1), generation: g });
    for wire in [
        AnalysisWire::SectionCount { total: 6 },
        AnalysisWire::TypeDetected { kind: AnalysisType::Meeting, confidence: 88.0 },
        AnalysisWire::SectionReady { section_type: "summary".into(), data: json!("The team agreed the Q2 plan.") },
        AnalysisWire::SectionReady { section_type: "actionItems".into(), data: json!([{"task": "Draft the launch memo", "owner": "Ada"}]) },
    ] {
        s.update(analysis::Event::WatchAnalysis { generation: g, wire });
    }
    s
}

pub fn analysis() -> Vec<(&'static str, AnalysisPanelVm)> {
    let checking = granted_analysis(true);
    let no_transcript = {
        let mut s = granted_analysis(false);
        s.update(analysis::Event::AnalysisLoaded { job: JOB, result: Ok(None) });
        s
    };
    let starting = {
        let mut s = granted_analysis(true);
        s.update(analysis::Event::AnalysisLoaded { job: JOB, result: Ok(None) });
        s.update(analysis::Event::Shown { key: key(1) });
        s
    };
    let interrupted = {
        let mut s = streaming();
        let g = generation(100);
        s.update(analysis::Event::WatchEnded { generation: g, result: Err(WatchFailure::Disconnected) });
        s.update(analysis::Event::AnalysisLoaded { job: JOB, result: Err(ApiFailure::network()) });
        s
    };
    let rejected = {
        let mut s = starting.clone();
        let body = StartRejection { error: "AI analysis is temporarily unavailable.".into(), code: "ai_unavailable".into(), ..Default::default() };
        s.update(analysis::Event::StartRejected { key: key(1), failure: ApiFailure::http(503, Some("ai_unavailable")), body: Some(body) });
        s
    };
    let unconfirmed = {
        let mut s = starting.clone();
        s.update(analysis::Event::StartUnknown { key: key(1) });
        s.update(analysis::Event::StartUnknown { key: key(1) });
        s
    };
    let exporting = {
        let mut s = completed(AnalysisType::Meeting);
        s.update(analysis::Event::ExportRequested { format: DocumentFormat::Pdf, at: NOW, utc_offset_minutes: 0, phone: false });
        s
    };
    let export_failed = {
        let mut s = exporting.clone();
        s.update(analysis::Event::ExportFinished { format: DocumentFormat::Pdf, result: Err(ExportFailure::Fonts) });
        s
    };
    vec![
        ("checking", checking.view(NOW)),
        ("no_transcript", no_transcript.view(NOW)),
        ("starting", starting.view(NOW)),
        ("streaming", streaming().view(NOW)),
        ("generic", completed(AnalysisType::Generic).view(NOW)),
        ("meeting", completed(AnalysisType::Meeting).view(NOW)),
        ("lecture", completed(AnalysisType::Lecture).view(NOW)),
        ("sales", completed(AnalysisType::Sales).view(NOW)),
        ("support", completed(AnalysisType::Support).view(NOW)),
        ("interview", completed(AnalysisType::Interview).view(NOW)),
        ("interrupted", interrupted.view(NOW)),
        ("rejected", rejected.view(NOW)),
        ("unconfirmed", unconfirmed.view(NOW)),
        ("exporting", exporting.view(NOW)),
        ("export_failed", export_failed.view(NOW)),
    ]
}

fn chat_message(order: i64, role: ChatRole, content: &str) -> ChatMessage {
    ChatMessage {
        id: uuid::Uuid::from_u128(0xf3_0000 + order as u128),
        account: AccountId(uuid::Uuid::nil()),
        job: JOB,
        generation: generation(200 + (order as u128).div_ceil(2)),
        order,
        role,
        content: content.into(),
        consent_epoch: 1,
        created_at: Micros(NOW.0 - HOUR),
    }
}

fn usage(current: i64, max: i64, reset_at: Option<Micros>) -> LimitUsage {
    LimitUsage { current, max, exceeded: current >= max, reset_at, reset_period: reset_at.map(|_| Period::Daily) }
}

fn chat_panel(history: Vec<ChatMessage>, limits: Result<LimitsView, ApiFailure>) -> ChatPanelState {
    let mut s = ChatPanelState::default();
    s.update(chat::Event::ConsentChanged { granted: true });
    s.update(chat::Event::Selected { job: JOB });
    s.update(chat::Event::HistoryLoaded { job: JOB, result: Ok(ChatHistory { messages: history, in_flight: None }) });
    s.update(chat::Event::LimitsLoaded { job: JOB, result: limits });
    s
}

fn limited() -> LimitsView {
    LimitsView {
        enabled: true,
        has_limits: true,
        questions_per_transcript: Some(usage(8, 10, None)),
        questions_per_user: None,
        tokens_per_transcript: None,
        tokens_per_user: Some(usage(9_500, 10_000, Some(Micros(NOW.0 + 5 * HOUR)))),
    }
}

fn history() -> Vec<ChatMessage> {
    vec![
        chat_message(1, ChatRole::User, "What did we decide about the launch?"),
        chat_message(2, ChatRole::Assistant, "You agreed to ship the beta on **1 May**, with Ada owning the memo."),
    ]
}

pub fn chat() -> Vec<(&'static str, ChatPanelVm)> {
    let unlimited = || Ok(LimitsView { enabled: true, ..Default::default() });
    let loading = {
        let mut s = ChatPanelState::default();
        s.update(chat::Event::ConsentChanged { granted: true });
        s.update(chat::Event::Selected { job: JOB });
        s
    };
    let typing = {
        let mut s = chat_panel(history(), Ok(limited()));
        s.update(chat::Event::SendRequested { text: "Who owns the venue?".into(), key: key(2) });
        s
    };
    let streaming = {
        let mut s = typing_accepted();
        let g = generation(300);
        s.update(chat::Event::WatchFrame { generation: g, frame: text_frame(g, 1, "The venue was booked by") });
        s
    };
    let sync_warning = {
        let mut s = typing_accepted();
        let g = generation(300);
        s.update(chat::Event::WatchEnded { generation: g, result: Err(WatchFailure::Disconnected) });
        let mut view = analysis_view(g, GenerationStatus::Interrupted, None);
        view.kind = GenerationKind::Chat;
        view.draft = Draft::Text { text: "The venue was booked by Lin.".into() };
        view.interruption = Some(Interruption::NotSaved { message: "The answer could not be saved.".into() });
        s.update(chat::Event::GenerationLoaded { generation: g, result: Ok(view) });
        s
    };
    let limit_reached = {
        let mut s = chat_panel(history(), Ok(limited()));
        s.update(chat::Event::SendRequested { text: "One more?".into(), key: key(3) });
        let body = StartRejection {
            error: "Question limit reached".into(),
            code: "TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED".into(),
            current: Some(10),
            max: Some(10),
            ..Default::default()
        };
        s.update(chat::Event::StartRejected { key: key(3), failure: ApiFailure::http(429, Some("TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED")), body: Some(body) });
        s
    };
    let copied = {
        let mut s = chat_panel(history(), unlimited());
        s.update(chat::Event::ChatCopied { ok: true });
        s
    };
    // REPAIR-P5-5: another tab's running answer, reattached from history.
    let reattached = {
        let mut s = ChatPanelState::default();
        s.update(chat::Event::ConsentChanged { granted: true });
        s.update(chat::Event::Selected { job: JOB });
        let mut generation = analysis_view(generation(301), GenerationStatus::Running, None);
        generation.kind = GenerationKind::Chat;
        generation.draft = Draft::Text { text: "The venue was booked by".into() };
        let in_flight = InFlightChat { question: "Who owns the venue?".into(), generation };
        s.update(chat::Event::HistoryLoaded { job: JOB, result: Ok(ChatHistory { messages: history(), in_flight: Some(in_flight) }) });
        s.update(chat::Event::LimitsLoaded { job: JOB, result: Ok(limited()) });
        s
    };
    // Desktop layout unless named `_phone`: the phone locks the question box
    // while an error is shown.
    vec![
        ("loading_history", loading.view(NOW, false)),
        ("welcome", chat_panel(vec![], unlimited()).view(NOW, false)),
        ("history", chat_panel(history(), Ok(limited())).view(NOW, false)),
        ("disabled", chat_panel(vec![], Err(ApiFailure::http(503, None))).view(NOW, false)),
        ("typing", typing.view(NOW, false)),
        ("streaming", streaming.view(NOW, false)),
        ("reattached", reattached.view(NOW, false)),
        ("sync_warning", sync_warning.view(NOW, false)),
        ("limit_reached", limit_reached.view(NOW, false)),
        ("limit_reached_phone", limit_reached.view(NOW, true)),
        ("copied", copied.view(NOW, false)),
    ]
}

fn typing_accepted() -> ChatPanelState {
    let mut s = chat_panel(history(), Ok(limited()));
    s.update(chat::Event::SendRequested { text: "Who owns the venue?".into(), key: key(2) });
    s.update(chat::Event::StartAccepted { key: key(2), generation: generation(300) });
    s
}

fn text_frame(g: GenerationId, seq: u64, text: &str) -> llm_core::ApplicationFrame {
    let identity = llm_core::StreamIdentity { generation: g.to_string(), content_revision: 1, consent_epoch: 1, execution: "fixture".into() };
    llm_core::ApplicationFrame {
        identity,
        seq,
        event: llm_core::TextEvent::Snapshot { text: text.into(), status: llm_core::StreamStatus::Streaming, persistence: llm_core::Persistence::Provisional },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::ai::analysis::PanelState;
    use crate::presentation::ai::consent::Gate;

    #[test]
    fn every_fixture_reaches_its_named_state() {
        let consent: std::collections::BTreeMap<_, _> = consent().into_iter().collect();
        assert_eq!(consent["loading"].gate, Gate::Loading);
        assert_eq!(consent["required"].gate, Gate::Required);
        assert!(consent["disable_failed"].has_consented && consent["disable_failed"].action_error.is_some(), "a failed DELETE keeps consent");
        assert!(consent["cleanup_pending"].cleanup_pending);

        let analysis: std::collections::BTreeMap<_, _> = analysis().into_iter().collect();
        for kind in ["generic", "meeting", "lecture", "sales", "support", "interview"] {
            assert_eq!(analysis[kind].state, PanelState::Ready, "{kind}");
            assert_eq!(analysis[kind].analysis.as_ref().map(|a| a.kind()), analysis[kind].detected.map(|d| d.kind), "{kind}");
        }
        assert!(analysis["streaming"].has_data && analysis["streaming"].progress.is_some());
        assert_eq!(analysis["interrupted"].state, PanelState::Interrupted);
        assert!(analysis["interrupted"].has_data, "partials stay on screen");
        assert!(analysis["rejected"].error_detail.is_some() && analysis["unconfirmed"].error_hint.is_some());
        assert!(analysis["export_failed"].export_error.is_some() && analysis["exporting"].export_busy.is_some());

        let chat: std::collections::BTreeMap<_, _> = chat().into_iter().collect();
        assert!(chat["loading_history"].loading_history && chat["welcome"].show_welcome && !chat["disabled"].enabled);
        assert!(chat["typing"].typing && chat["streaming"].lines.last().unwrap().streaming);
        assert!(chat["sync_warning"].sync_warning.is_some() && chat["limit_reached"].error.is_some() && chat["copied"].notice.is_some());
        assert!(chat["history"].limits.is_some());
        assert!(chat["limit_reached_phone"].input_locked && !chat["limit_reached"].input_locked && chat["disabled"].input_locked);
    }
}
