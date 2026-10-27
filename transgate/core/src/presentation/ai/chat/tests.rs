use llm_core::{ApplicationFrame, Persistence, StreamIdentity, StreamStatus, TextEvent};

use super::*;
use crate::ai::{ChatHistory, ChatMessage, Draft, FinalContent, GenerationKind, GenerationStatus, InFlightChat, Interruption, LimitUsage, LimitsView};
use crate::ai::ports::Period;
use crate::ids::{AccountId, JobId, MICROS_PER_MINUTE, Micros};

const JOB: JobId = JobId(9);
const NOW: Micros = Micros(1_800_000_000_000_000);
const HOUR: i64 = 60 * MICROS_PER_MINUTE;

fn key(n: u128) -> RequestKey {
    uuid::Uuid::from_u128(n)
}

fn generation(n: u128) -> GenerationId {
    GenerationId(uuid::Uuid::from_u128(0xc4a7 + n))
}

fn message(order: i64, role: ChatRole, content: &str, g: GenerationId) -> ChatMessage {
    ChatMessage {
        id: uuid::Uuid::from_u128(order as u128 + 1),
        account: AccountId(uuid::Uuid::nil()),
        job: JOB,
        generation: g,
        order,
        role,
        content: content.into(),
        consent_epoch: 1,
        created_at: NOW,
    }
}

fn frame(g: GenerationId, seq: u64, event: TextEvent) -> ApplicationFrame {
    let identity = StreamIdentity { generation: g.to_string(), content_revision: 1, consent_epoch: 1, execution: "exec".into() };
    ApplicationFrame { identity, seq, event }
}

fn snapshot(g: GenerationId, seq: u64, text: &str) -> ApplicationFrame {
    frame(g, seq, TextEvent::Snapshot { text: text.into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional })
}

fn generation_view(g: GenerationId, status: GenerationStatus, draft: &str, final_text: Option<&str>, interruption: Option<Interruption>) -> crate::ai::GenerationView {
    crate::ai::GenerationView {
        id: g,
        job: JOB,
        kind: GenerationKind::Chat,
        status,
        sequence: 4,
        source_revision: 1,
        consent_epoch: 1,
        draft: Draft::Text { text: draft.into() },
        final_content: final_text.map(|t| FinalContent::Text { text: t.into() }),
        durable: true,
        remote_outcome_unknown: false,
        execution: Some(uuid::Uuid::nil()),
        tokens_used: None,
        interruption,
        created_at: NOW,
        finished_at: None,
    }
}

/// A granted panel with the loaded history.
fn panel(history: Vec<ChatMessage>) -> ChatPanelState {
    let mut s = ChatPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    assert_eq!(s.update(Event::Selected { job: JOB }), vec![Effect::LoadHistory { job: JOB }, Effect::LoadLimits { job: JOB }]);
    s.update(Event::HistoryLoaded { job: JOB, result: Ok(ChatHistory { messages: history, in_flight: None }) });
    s.update(Event::LimitsLoaded { job: JOB, result: Ok(LimitsView { enabled: true, ..Default::default() }) });
    s
}

fn send(s: &mut ChatPanelState, text: &str, k: RequestKey) -> Vec<Effect> {
    s.update(Event::SendRequested { text: text.into(), key: k })
}

#[test]
fn history_loads_on_selection_and_the_welcome_card_follows_user_interaction() {
    let s = panel(vec![]);
    let vm = s.view(NOW, false);
    assert!(vm.show_welcome && vm.lines.is_empty() && !vm.show_history_header);
    let g = generation(1);
    let s = panel(vec![message(1, ChatRole::User, "What?", g), message(2, ChatRole::Assistant, "This.", g)]);
    let vm = s.view(NOW, false);
    assert!(!vm.show_welcome && vm.show_history_header);
    assert_eq!(vm.lines.iter().map(|l| (l.role, l.content.as_str())).collect::<Vec<_>>(), vec![(ChatRole::User, "What?"), (ChatRole::Assistant, "This.")]);
    assert!(vm.lines[1].show_actions);
}

#[test]
fn nothing_loads_without_consent() {
    let mut s = ChatPanelState::default();
    assert!(s.update(Event::Selected { job: JOB }).is_empty());
    assert_eq!(s.update(Event::ConsentChanged { granted: true }), vec![Effect::LoadHistory { job: JOB }, Effect::LoadLimits { job: JOB }]);
}

#[test]
fn sending_posts_the_original_question_with_prior_turns_then_watches_the_header_generation() {
    let g = generation(1);
    let mut s = panel(vec![message(1, ChatRole::User, "Q1", g), message(2, ChatRole::Assistant, "A1", g)]);
    assert!(send(&mut s, "   ", key(5)).is_empty(), "a blank question is not sent");
    let effects = send(&mut s, "  What next?  ", key(5));
    let history = vec![HistoryEntry { role: ChatRole::User, content: "Q1".into() }, HistoryEntry { role: ChatRole::Assistant, content: "A1".into() }];
    assert_eq!(effects, vec![Effect::StartChat { job: JOB, key: key(5), message: "  What next?  ".into(), history }]);
    let vm = s.view(NOW, false);
    assert_eq!(vm.lines.last().unwrap().content, "  What next?  ", "D5: the original accepted question is shown");
    assert!(vm.typing && !vm.can_send);
    assert!(send(&mut s, "again", key(6)).is_empty(), "one question at a time");
    let next = generation(2);
    assert_eq!(s.update(Event::StartAccepted { key: key(5), generation: next }), vec![Effect::Watch { generation: next, kind: WatchKind::Text, after: None }]);
}

#[test]
fn streamed_text_renders_progressively_and_the_saved_final_replaces_it() {
    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 1, "Hel") });
    let vm = s.view(NOW, false);
    assert!(!vm.typing, "the typing dots give way to the streaming bubble");
    let last = vm.lines.last().unwrap();
    assert_eq!((last.role, last.content.as_str(), last.streaming, last.show_actions), (ChatRole::Assistant, "Hel", true, false));
    let effects = s.update(Event::WatchFrame { generation: g, frame: frame(g, 2, TextEvent::Final { text: "Hello.".into(), persistence: Persistence::Saved }) });
    assert_eq!(effects, vec![Effect::LoadLimits { job: JOB }], "limits refresh after a successful answer");
    let vm = s.view(NOW, false);
    let last = vm.lines.last().unwrap();
    assert_eq!((last.content.as_str(), last.streaming, last.show_actions), ("Hello.", false, true));
    assert!(vm.can_send && vm.error.is_none());
}

// REPAIR-P5-5, P5 D6: a tab that opens a job while its answer runs elsewhere shows the held
// partial as the streaming answer and attaches the watch. No start, retry, key or quota.
#[test]
fn an_answer_running_in_another_tab_is_shown_and_watched_without_a_new_request() {
    let saved = generation(1);
    let g = generation(2);
    let mut s = ChatPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(Event::Selected { job: JOB });
    let in_flight = InFlightChat { question: "  What next?  ".into(), generation: generation_view(g, GenerationStatus::Running, "The team reviewed", None, None) };
    let history = ChatHistory { messages: vec![message(1, ChatRole::User, "Q1", saved), message(2, ChatRole::Assistant, "A1", saved)], in_flight: Some(in_flight) };
    let effects = s.update(Event::HistoryLoaded { job: JOB, result: Ok(history) });
    assert_eq!(effects, vec![Effect::Watch { generation: g, kind: WatchKind::Text, after: None }], "exactly one watch");
    let vm = s.view(NOW, false);
    let shown: Vec<_> = vm.lines.iter().map(|l| (l.role, l.content.as_str(), l.streaming, l.show_actions)).collect();
    assert_eq!(shown[2..], [(ChatRole::User, "  What next?  ", false, false), (ChatRole::Assistant, "The team reviewed", true, false)]);
    assert!(!vm.can_send && !vm.typing && !vm.show_welcome);
    assert!(send(&mut s, "another", key(9)).is_empty(), "one question at a time, across tabs");

    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 5, "The team reviewed the budget") });
    assert_eq!(s.view(NOW, false).lines[3].content, "The team reviewed the budget");
    let effects = s.update(Event::WatchFrame { generation: g, frame: frame(g, 6, TextEvent::Final { text: "The team reviewed the budget.".into(), persistence: Persistence::Saved }) });
    assert_eq!(effects, vec![Effect::LoadLimits { job: JOB }]);
    let vm = s.view(NOW, false);
    assert_eq!((vm.lines.len(), vm.lines[3].streaming, vm.lines[3].show_actions, vm.can_send), (4, false, true, true));
    assert_eq!(s.update(Event::RetryRequested { index: 3, key: key(10) }), vec![
        Effect::RetryChat { generation: g, key: key(10) },
        Effect::Telemetry { event: TelemetryEvent::new("ai_chat_message_retried").number("jobId", JOB.0 as f64).number("messageIndex", 3.0) },
    ], "the attached answer becomes an ordinary saved answer");
}

#[test]
fn a_running_answer_without_text_yet_shows_typing_and_leaving_detaches() {
    let g = generation(2);
    let mut s = ChatPanelState::default();
    s.update(Event::ConsentChanged { granted: true });
    s.update(Event::Selected { job: JOB });
    let in_flight = InFlightChat { question: "Q".into(), generation: generation_view(g, GenerationStatus::Accepted, "", None, None) };
    let effects = s.update(Event::HistoryLoaded { job: JOB, result: Ok(ChatHistory { messages: vec![], in_flight: Some(in_flight) }) });
    assert_eq!(effects, vec![Effect::Watch { generation: g, kind: WatchKind::Text, after: None }]);
    let vm = s.view(NOW, false);
    assert_eq!(vm.lines.iter().map(|l| (l.role, l.content.as_str())).collect::<Vec<_>>(), vec![(ChatRole::User, "Q")]);
    assert!(vm.typing && !vm.can_send);
    assert_eq!(s.update(Event::Selected { job: JobId(10) })[0], Effect::Detach { generation: g }, "another job closes the attachment");
}

#[test]
fn a_quota_refusal_removes_the_question_and_shows_the_source_limit_message() {
    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    let body = StartRejection {
        error: "You have reached the maximum of 50 questions for this transcript.".into(),
        code: "TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED".into(),
        current: Some(50),
        max: Some(50),
        ..Default::default()
    };
    s.update(Event::StartRejected { key: key(1), failure: ApiFailure::http(429, None), body: Some(body) });
    let vm = s.view(NOW, false);
    assert!(vm.lines.is_empty(), "source onError drops the last message");
    let error = vm.error.unwrap();
    assert_eq!(error.message, Some(ai("TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED").number("maxQuestionsPerTranscript", 50)));
    assert!(vm.can_send);
    s.update(Event::ErrorDismissed);
    assert!(s.view(NOW, false).error.is_none());
}

/// The question box is locked by core, per layout: the phone locks while an
/// error is shown (source `MobileChatView`), the desktop only when the host
/// disabled chat (source `DesktopChatView`); both lock on a 503.
#[test]
fn the_input_lock_follows_the_layout_and_the_shown_error() {
    let mut s = panel(vec![]);
    assert!(!s.view(NOW, true).input_locked && !s.view(NOW, false).input_locked);
    send(&mut s, "Q", key(1));
    s.update(Event::StartRejected { key: key(1), failure: ApiFailure::http(500, None), body: None });
    assert!(s.view(NOW, true).error.is_some());
    assert!(s.view(NOW, true).input_locked, "phone: locked while the error is shown");
    assert!(!s.view(NOW, false).input_locked, "desktop: the error never locks the box");
    s.update(Event::ErrorDismissed);
    assert!(!s.view(NOW, true).input_locked, "dismissing the error unlocks the phone box");
    s.update(Event::LimitsLoaded { job: JOB, result: Err(ApiFailure::http(503, Some("chat_disabled"))) });
    assert!(s.view(NOW, true).input_locked && s.view(NOW, false).input_locked, "a disabled host locks both layouts");
}

#[test]
fn an_unknown_start_replays_the_same_key_once() {
    let mut s = panel(vec![]);
    let first = send(&mut s, "Q", key(1));
    assert_eq!(s.update(Event::StartUnknown { key: key(1) }), first, "exact replay of the same logical request");
    assert!(s.update(Event::StartUnknown { key: key(1) }).is_empty());
    let vm = s.view(NOW, false);
    assert!(vm.lines.is_empty());
    assert_eq!(vm.error.unwrap().message, Some(app("The AI request could not be confirmed. Check your connection and retry.")));
}

#[test]
fn an_interrupted_stream_is_read_back_before_deciding_failure_or_unsaved_answer() {
    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 1, "Partial") });
    let ended = frame(g, 2, TextEvent::Snapshot { text: "Partial".into(), status: StreamStatus::Interrupted, persistence: Persistence::Saved });
    assert_eq!(s.update(Event::WatchFrame { generation: g, frame: ended }), vec![Effect::LoadGeneration { generation: g }]);
    let not_saved = generation_view(g, GenerationStatus::Interrupted, "Partial answer", None, Some(Interruption::NotSaved { message: "save failed".into() }));
    s.update(Event::GenerationLoaded { generation: g, result: Ok(not_saved) });
    let vm = s.view(NOW, false);
    assert_eq!(vm.lines.last().unwrap().content, "Partial answer", "the visible answer stays");
    assert_eq!(vm.sync_warning, Some(app("This answer could not be saved. It may disappear after refresh.")), "never shown as durably saved");

    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    s.update(Event::StartAccepted { key: key(1), generation: g });
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 1, "Partial") });
    s.update(Event::WatchFrame { generation: g, frame: frame(g, 2, TextEvent::Snapshot { text: "Partial".into(), status: StreamStatus::Interrupted, persistence: Persistence::Saved }) });
    let failed = generation_view(g, GenerationStatus::Interrupted, "Partial", None, Some(Interruption::Deadline));
    s.update(Event::GenerationLoaded { generation: g, result: Ok(failed) });
    let vm = s.view(NOW, false);
    assert_eq!(vm.lines.iter().map(|l| l.role).collect::<Vec<_>>(), vec![ChatRole::User], "source drops the partial answer");
    assert_eq!(vm.error.unwrap().detail.as_deref(), Some("The AI request took too long"));
}

#[test]
fn a_lost_watch_reattaches_through_the_generation_snapshot_without_regenerating() {
    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 1, "Hello ") });
    assert_eq!(s.update(Event::WatchEnded { generation: g, result: Err(WatchFailure::Disconnected) }), vec![Effect::LoadGeneration { generation: g }]);
    let running = generation_view(g, GenerationStatus::Running, "Hello", None, None);
    assert_eq!(s.update(Event::GenerationLoaded { generation: g, result: Ok(running) }), vec![Effect::Watch { generation: g, kind: WatchKind::Text, after: None }]);
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 2, "Hello") });
    assert!(s.update(Event::WatchFrame { generation: g, frame: frame(g, 3, TextEvent::Delta("world".into())) }).is_empty());
    assert_eq!(s.view(NOW, false).lines.last().unwrap().content, "Hello", "never a raw suffix on a sanitized checkpoint");
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 4, "Hello world") });
    assert_eq!(s.view(NOW, false).lines.last().unwrap().content, "Hello world");
}

#[test]
fn frames_of_another_generation_or_after_a_job_change_are_ignored() {
    let mut s = panel(vec![]);
    send(&mut s, "Q", key(1));
    let g = generation(1);
    s.update(Event::StartAccepted { key: key(1), generation: g });
    let other = generation(2);
    s.update(Event::WatchFrame { generation: other, frame: snapshot(other, 1, "foreign") });
    assert!(s.view(NOW, false).typing);
    assert_eq!(s.update(Event::Selected { job: JobId(10) }), vec![Effect::Detach { generation: g }, Effect::LoadHistory { job: JobId(10) }, Effect::LoadLimits { job: JobId(10) }]);
    s.update(Event::WatchFrame { generation: g, frame: snapshot(g, 1, "late") });
    assert!(s.view(NOW, false).lines.is_empty());
}

#[test]
fn retry_streams_into_the_answer_in_place_through_the_retry_route() {
    let g = generation(1);
    let mut s = panel(vec![message(1, ChatRole::User, "Q", g), message(2, ChatRole::Assistant, "Old", g)]);
    let effects = s.update(Event::RetryRequested { index: 1, key: key(7) });
    assert_eq!(effects, vec![
        Effect::RetryChat { generation: g, key: key(7) },
        Effect::Telemetry { event: TelemetryEvent::new("ai_chat_message_retried").number("jobId", 9.0).number("messageIndex", 1.0) },
    ]);
    assert_eq!(s.view(NOW, false).lines[1].content, "", "the old answer becomes the streaming placeholder");
    assert!(s.update(Event::RetryRequested { index: 0, key: key(8) }).is_empty(), "only answers retry");
    let next = generation(2);
    s.update(Event::StartAccepted { key: key(7), generation: next });
    s.update(Event::WatchFrame { generation: next, frame: frame(next, 1, TextEvent::Final { text: "New".into(), persistence: Persistence::Saved }) });
    let vm = s.view(NOW, false);
    assert_eq!(vm.lines.len(), 2);
    assert_eq!(vm.lines[1].content, "New");
}

#[test]
fn copy_rate_and_download_follow_the_source_actions() {
    let g = generation(1);
    let mut s = panel(vec![message(1, ChatRole::User, "Q", g), message(2, ChatRole::Assistant, "A", g)]);
    assert_eq!(s.update(Event::CopyMessageRequested { index: 1 }), vec![
        Effect::CopyText { text: "A".into() },
        Effect::Telemetry { event: TelemetryEvent::new("ai_chat_message_copied").number("jobId", 9.0).number("messageIndex", 1.0) },
    ]);
    assert_eq!(s.update(Event::RateRequested { index: 1, rating: Rating::Dislike }), vec![
        Effect::Telemetry { event: TelemetryEvent::new("ai_chat_message_rated").number("jobId", 9.0).number("messageIndex", 1.0).text("rating", "dislike") },
    ], "rating is analytics only");
    assert_eq!(s.update(Event::CopyChatRequested), vec![Effect::CopyChat { text: "Question: Q\nAnswer: A\n".into() }]);
    s.update(Event::ChatCopied { ok: true });
    assert_eq!(s.view(NOW, false).notice, Some(ai("Copied!")));
    s.update(Event::ChatCopied { ok: false });
    assert_eq!(s.view(NOW, false).notice, Some(ai("Failed to copy")));
    assert_eq!(s.update(Event::DownloadRequested { at: Micros(1_700_000_000_123_000) }), vec![
        Effect::DownloadText { filename: "chat-export-1700000000123.txt".into(), text: "Question: Q\nAnswer: A\n".into() },
    ]);
    let mut empty = panel(vec![]);
    assert!(empty.update(Event::DownloadRequested { at: NOW }).is_empty(), "an empty chat downloads nothing");
}

#[test]
fn limits_prefer_the_transcript_cap_and_show_the_user_reset() {
    let mut s = panel(vec![]);
    let limits = LimitsView {
        enabled: true,
        has_limits: true,
        questions_per_transcript: Some(LimitUsage { current: 46, max: 50, exceeded: false, reset_at: None, reset_period: None }),
        questions_per_user: Some(LimitUsage { current: 1, max: 500, exceeded: false, reset_at: Some(Micros(NOW.0 + 3 * HOUR)), reset_period: Some(Period::Daily) }),
        tokens_per_transcript: None,
        tokens_per_user: Some(LimitUsage { current: 10_000, max: 50_000, exceeded: false, reset_at: Some(Micros(NOW.0 + 3 * HOUR - 1)), reset_period: Some(Period::Daily) }),
    };
    s.update(Event::LimitsLoaded { job: JOB, result: Ok(limits) });
    let l = s.view(NOW, false).limits.unwrap();
    assert_eq!(l.questions, Some(UsageVm { current: 46, max: 50, width_percent: 92, tone: Tone::Danger }));
    assert_eq!(l.tokens, Some(UsageVm { current: 10_000, max: 50_000, width_percent: 20, tone: Tone::Accent }));
    assert_eq!(l.reset, Some(ResetLabel::InHours { hours: 3 }), "the user token cap supplies the reset");
    assert_eq!(s.view(Micros(NOW.0 + 3 * HOUR - 30 * 60 * 1_000_000), false).limits.unwrap().reset, Some(ResetLabel::InHours { hours: 1 }), "Math.ceil of a partial hour");
    assert_eq!(s.view(Micros(NOW.0 + 3 * HOUR), false).limits.unwrap().reset, Some(ResetLabel::LessThanHour), "only a reached reset is under one hour");
    assert_eq!(s.view(Micros(NOW.0 - 3 * 24 * HOUR), false).limits.unwrap().reset, Some(ResetLabel::InDays { days: 4 }));
    assert_eq!(s.view(Micros(NOW.0 - 10 * 24 * HOUR), false).limits.unwrap().reset, Some(ResetLabel::On { at: Micros(NOW.0 + 3 * HOUR - 1) }));
    s.update(Event::LimitsLoaded { job: JOB, result: Ok(LimitsView { enabled: true, has_limits: false, ..Default::default() }) });
    assert!(s.view(NOW, false).limits.is_none(), "paid callers have no limits panel");
    s.update(Event::LimitsLoaded { job: JOB, result: Err(ApiFailure::http(503, Some("chat_disabled"))) });
    assert!(!s.view(NOW, false).enabled, "503 means chat is disabled");
    s.update(Event::LimitsLoaded { job: JOB, result: Err(ApiFailure::http(401, None)) });
    assert!(s.view(NOW, false).enabled && s.view(NOW, false).limits.is_none(), "source defaults on 401");
}

#[test]
fn revoked_consent_clears_the_conversation() {
    let g = generation(1);
    let mut s = panel(vec![message(1, ChatRole::User, "Q", g), message(2, ChatRole::Assistant, "A", g)]);
    s.update(Event::ConsentChanged { granted: false });
    assert!(s.view(NOW, false).lines.is_empty());
}
