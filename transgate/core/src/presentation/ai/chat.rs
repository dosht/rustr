//! The Ask AI chat (WEB-029/030, AI-F07..F10, AI-010..021, FRE-024): one per
//! selected job. History and limits load on selection, a question streams
//! through the shared `llm_core::StreamState` (the same reducer every shell
//! uses), and the saved final replaces the provisional text.
//!
//! Source behavior kept on purpose: a refused or failed question drops the last
//! displayed message and shows the error card; retry streams into the answer in
//! place while the server appends another pair; copy, retry and rating fire the
//! source analytics events (rating is analytics only). An answer the server
//! could not save stays visible with the sync warning, never as durable.

use llm_core::{ApplicationFrame, StreamState, StreamStatus, StreamUpdate};
use serde::{Deserialize, Serialize};

use crate::ai::presentation::public_error;
use crate::ai::{ChatHistory, ChatRole, Draft, FinalContent, GenerationId, GenerationStatus, GenerationView, InFlightChat, Interruption, LimitUsage, LimitsView};
use crate::ids::{JobId, MICROS_PER_MINUTE, Micros};
use crate::presentation::ai::{ApiFailure, RequestKey, StartRejection, WatchFailure, WatchKind};
use crate::presentation::msg::{Msg, ai, app};
use crate::presentation::telemetry::TelemetryEvent;

/// One prior turn sent with a question (AI-013); the server validates roles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Like,
    Dislike,
}

/// Not serializable: it carries `llm_core::ApplicationFrame`, which the
/// std-only shared crate deliberately keeps free of serde.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Selected { job: JobId },
    ConsentChanged { granted: bool },
    HistoryLoaded { job: JobId, result: Result<ChatHistory, ApiFailure> },
    LimitsLoaded { job: JobId, result: Result<LimitsView, ApiFailure> },
    SendRequested { text: String, key: RequestKey },
    /// `index` counts displayed messages, as the source's message actions do.
    RetryRequested { index: usize, key: RequestKey },
    StartAccepted { key: RequestKey, generation: GenerationId },
    StartRejected { key: RequestKey, failure: ApiFailure, body: Option<StartRejection> },
    StartUnknown { key: RequestKey },
    WatchFrame { generation: GenerationId, frame: ApplicationFrame },
    WatchEnded { generation: GenerationId, result: Result<(), WatchFailure> },
    GenerationLoaded { generation: GenerationId, result: Result<GenerationView, ApiFailure> },
    CopyMessageRequested { index: usize },
    RateRequested { index: usize, rating: Rating },
    CopyChatRequested,
    ChatCopied { ok: bool },
    /// The click instant, which names the source download file.
    DownloadRequested { at: Micros },
    ErrorDismissed,
    NoticeDismissed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// `GET /api/ai/jobs/{job}/chat`.
    LoadHistory { job: JobId },
    /// `GET /api/ai/chat-limits?jobId={job}`.
    LoadLimits { job: JobId },
    /// `POST /api/v1/ai/chat` with `requestId = key`; header, drop, then `StartAccepted`.
    StartChat { job: JobId, key: RequestKey, message: String, history: Vec<HistoryEntry> },
    /// `POST /api/ai/generations/{generation}/retry` with `requestId = key`.
    RetryChat { generation: GenerationId, key: RequestKey },
    Watch { generation: GenerationId, kind: WatchKind, after: Option<u64> },
    Detach { generation: GenerationId },
    /// `GET /api/ai/generations/{generation}`: why a stream ended.
    LoadGeneration { generation: GenerationId },
    /// One message, plain text; the copy button shows its own check.
    CopyText { text: String },
    /// The whole conversation; answered by `ChatCopied`.
    CopyChat { text: String },
    /// UTF-8 `text/plain`; share on touch devices, otherwise download.
    DownloadText { filename: String, text: String },
    Telemetry { event: TelemetryEvent },
}

/// The usage bar's tone: the source's sky, yellow (75%) and red (90%) bars as
/// the shared semantic tones.
pub use crate::presentation::job_list::Tone;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UsageVm {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub current: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max: i64,
    /// The bar width, `min(current / max, 1)` in percent.
    pub width_percent: u8,
    pub tone: Tone,
}

/// The source's reset sentence: "Resets in less than 1 hour", "Resets in" N
/// "hour(s)"/"day(s)", or "Resets on" a date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResetLabel {
    LessThanHour,
    InHours {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        hours: i64,
    },
    InDays {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        days: i64,
    },
    /// Epoch microseconds of the reset instant, shown as a local date.
    On {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: Micros,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LimitsVm {
    pub questions: Option<UsageVm>,
    pub tokens: Option<UsageVm>,
    pub reset: Option<ResetLabel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatLineVm {
    pub role: ChatRole,
    pub content: String,
    /// The blinking cursor on the answer being streamed.
    pub streaming: bool,
    /// Copy, retry and rating under a finished answer.
    pub show_actions: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatErrorVm {
    /// A catalog message (a limit, or a state the source never named).
    pub message: Option<Msg>,
    /// The server's own text, shown as the source showed it.
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPanelVm {
    /// False when the host reports chat disabled (503).
    pub enabled: bool,
    /// The question box and Send are locked: on a disabled host in both
    /// layouts, and on the phone while an error is shown (source
    /// `MobileChatView`; the desktop never locked on an error).
    pub input_locked: bool,
    pub loading_history: bool,
    pub lines: Vec<ChatLineVm>,
    /// "Chat History" with copy and download.
    pub show_history_header: bool,
    pub show_welcome: bool,
    /// Typing dots before the first streamed text.
    pub typing: bool,
    pub can_send: bool,
    pub limits: Option<LimitsVm>,
    pub error: Option<ChatErrorVm>,
    pub sync_warning: Option<Msg>,
    /// A transient toast after copying the whole chat.
    pub notice: Option<Msg>,
}

#[derive(Clone, Debug, PartialEq)]
struct Line {
    role: ChatRole,
    content: String,
    generation: Option<GenerationId>,
}

#[derive(Clone, Debug, PartialEq)]
enum Request {
    Send { key: RequestKey, message: String, history: Vec<HistoryEntry> },
    Retry { key: RequestKey, of: GenerationId },
    /// An answer another tab started, reattached from history (REPAIR-P5-5).
    /// This tab never sent it, so it has no request key and never replays it.
    Attach,
}

impl Request {
    fn key(&self) -> Option<RequestKey> {
        match self {
            Request::Send { key, .. } | Request::Retry { key, .. } => Some(*key),
            Request::Attach => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Starting { replayed: bool },
    Streaming { generation: GenerationId, started: bool, reconnected: bool },
    Reading { generation: GenerationId, reconnected: bool },
}

#[derive(Clone, Debug, PartialEq)]
struct Pending {
    request: Request,
    /// The answer line this request streams into.
    index: usize,
    step: Step,
}

/// Holds the shared `llm_core::StreamState`, which is deliberately neither
/// `Clone` nor serializable; the app keeps this state by value.
#[derive(Debug, Default)]
pub struct ChatPanelState {
    job: Option<JobId>,
    consent: bool,
    loading_history: bool,
    history_loaded: bool,
    lines: Vec<Line>,
    /// The host answered 503: chat is turned off.
    disabled: bool,
    limits: Option<LimitsView>,
    pending: Option<Pending>,
    stream: StreamState,
    error: Option<ChatErrorVm>,
    sync_warning: bool,
    notice: Option<Msg>,
}

impl ChatPanelState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Selected { job } => {
                let effects = self.detach();
                let consent = self.consent;
                *self = ChatPanelState { job: Some(job), consent, ..ChatPanelState::default() };
                [effects, self.load()].concat()
            }
            Event::ConsentChanged { granted } => {
                if granted == self.consent {
                    return vec![];
                }
                self.consent = granted;
                if granted {
                    return self.load();
                }
                let effects = self.detach();
                let job = self.job;
                *self = ChatPanelState { job, ..ChatPanelState::default() };
                effects
            }
            Event::HistoryLoaded { job, result } => {
                if self.job != Some(job) || !self.loading_history {
                    return vec![];
                }
                self.loading_history = false;
                self.history_loaded = true;
                match result {
                    Ok(history) => {
                        self.lines = history
                            .messages
                            .into_iter()
                            .map(|m| Line { role: m.role, content: m.content, generation: Some(m.generation) })
                            .collect();
                        // A question this tab sent meanwhile owns the lane; the
                        // server then reports that same answer, already watched.
                        if let Some(in_flight) = history.in_flight.filter(|_| self.pending.is_none()) {
                            return self.attach(in_flight);
                        }
                    }
                    Err(_) => self.error = Some(ChatErrorVm { message: Some(app("Failed to load chat history")), detail: None }),
                }
                vec![]
            }
            Event::LimitsLoaded { job, result } => {
                if self.job != Some(job) {
                    return vec![];
                }
                match result {
                    Ok(limits) => {
                        self.disabled = !limits.enabled;
                        self.limits = Some(limits);
                    }
                    Err(failure) if failure.status == Some(503) => {
                        self.disabled = true;
                        self.limits = None;
                    }
                    // The source treats 401 and other failures as enabled with no limits.
                    Err(_) => {
                        self.disabled = false;
                        self.limits = None;
                    }
                }
                vec![]
            }
            Event::SendRequested { text, key } => self.send(text, key),
            Event::RetryRequested { index, key } => self.retry(index, key),
            Event::StartAccepted { key, generation } => {
                let Some(p) = self.pending.as_mut().filter(|p| p.request.key() == Some(key) && matches!(p.step, Step::Starting { .. })) else {
                    return vec![];
                };
                p.step = Step::Streaming { generation, started: false, reconnected: false };
                self.stream.clear();
                vec![Effect::Watch { generation, kind: WatchKind::Text, after: None }]
            }
            Event::StartRejected { key, failure: _, body } => {
                if !self.pending.as_ref().is_some_and(|p| p.request.key() == Some(key) && matches!(p.step, Step::Starting { .. })) {
                    return vec![];
                }
                self.fail(rejection_error(body));
                vec![]
            }
            Event::StartUnknown { key } => {
                let Some(p) = self.pending.as_mut().filter(|p| p.request.key() == Some(key)) else { return vec![] };
                let Step::Starting { replayed } = p.step else { return vec![] };
                if replayed {
                    self.fail(ChatErrorVm { message: Some(unconfirmed()), detail: None });
                    return vec![];
                }
                p.step = Step::Starting { replayed: true };
                let p = p.clone();
                vec![self.request_effect(&p)]
            }
            Event::WatchFrame { generation, frame } => self.frame(generation, frame),
            Event::WatchEnded { generation, result: _ } => {
                let Some(p) = self.pending.as_mut() else { return vec![] };
                let Step::Streaming { generation: g, reconnected, .. } = p.step else { return vec![] };
                if g != generation {
                    return vec![];
                }
                if reconnected {
                    self.fail(ChatErrorVm { message: Some(unconfirmed()), detail: None });
                    return vec![];
                }
                p.step = Step::Reading { generation, reconnected: true };
                vec![Effect::LoadGeneration { generation }]
            }
            Event::GenerationLoaded { generation, result } => self.generation_loaded(generation, result),
            Event::CopyMessageRequested { index } => {
                let Some(line) = self.lines.get(index) else { return vec![] };
                vec![Effect::CopyText { text: line.content.clone() }, self.telemetry("ai_chat_message_copied", index)]
            }
            Event::RateRequested { index, rating } => {
                if self.lines.get(index).is_none_or(|l| l.role != ChatRole::Assistant) {
                    return vec![];
                }
                let rating = match rating {
                    Rating::Like => "like",
                    Rating::Dislike => "dislike",
                };
                let Effect::Telemetry { event } = self.telemetry("ai_chat_message_rated", index) else { unreachable!() };
                vec![Effect::Telemetry { event: event.text("rating", rating) }]
            }
            Event::CopyChatRequested => {
                let text = self.serialize();
                if text.trim().is_empty() {
                    return vec![];
                }
                vec![Effect::CopyChat { text }]
            }
            Event::ChatCopied { ok } => {
                self.notice = Some(if ok { ai("Copied!") } else { ai("Failed to copy") });
                vec![]
            }
            Event::DownloadRequested { at } => {
                let text = self.serialize();
                if text.trim().is_empty() {
                    return vec![];
                }
                vec![Effect::DownloadText { filename: format!("chat-export-{}.txt", at.0 / 1000), text }]
            }
            Event::ErrorDismissed => {
                self.error = None;
                vec![]
            }
            Event::NoticeDismissed => {
                self.notice = None;
                vec![]
            }
        }
    }

    /// `phone`: the phone AI layout (`Layouts::phone_ai`), which locks the
    /// question box while an error is shown.
    pub fn view(&self, now: Micros, phone: bool) -> ChatPanelVm {
        let streaming_index = match &self.pending {
            // A reattached answer streams from its held partial before any frame.
            Some(Pending { index, request, step: Step::Streaming { started, .. } }) if *started || *request == Request::Attach => Some(*index),
            _ => None,
        };
        let sending = self.pending.is_some();
        let last = self.lines.len().checked_sub(1);
        let lines = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, l)| ChatLineVm {
                role: l.role,
                content: l.content.clone(),
                streaming: Some(i) == streaming_index,
                show_actions: l.role == ChatRole::Assistant && !(sending && (Some(i) == last || Some(i) == self.pending.as_ref().map(|p| p.index))),
            })
            .collect();
        ChatPanelVm {
            enabled: !self.disabled,
            input_locked: self.disabled || (phone && self.error.is_some()),
            loading_history: self.loading_history,
            lines,
            show_history_header: !self.lines.is_empty(),
            show_welcome: self.history_loaded && !self.lines.iter().any(|l| l.role == ChatRole::User),
            typing: sending && self.lines.last().is_none_or(|l| l.role != ChatRole::Assistant),
            // The source sends nothing while history loads (`DesktopChatView.tsx:142,307`).
            can_send: !sending && !self.loading_history,
            limits: self.limits.as_ref().filter(|l| l.has_limits).and_then(|l| limits_vm(l, now)),
            error: self.error.clone(),
            sync_warning: self.sync_warning.then(|| app("This answer could not be saved. It may disappear after refresh.")),
            notice: self.notice.clone(),
        }
    }

    /// History loads only on `Selected` (which resets any request) and on a
    /// consent grant (which follows the reset of a refusal), and nothing is sent
    /// while it loads: a history answer never lands over a pending question
    /// (REPAIR-P5-5b), and a late duplicate is ignored once loaded.
    fn load(&mut self) -> Vec<Effect> {
        let Some(job) = self.job.filter(|_| self.consent) else { return vec![] };
        self.loading_history = true;
        vec![Effect::LoadHistory { job }, Effect::LoadLimits { job }]
    }

    fn send(&mut self, text: String, key: RequestKey) -> Vec<Effect> {
        if text.trim().is_empty() || self.pending.is_some() || self.loading_history || !self.consent {
            return vec![];
        }
        let Some(_) = self.job else { return vec![] };
        let history = self.lines.iter().map(|l| HistoryEntry { role: l.role, content: l.content.clone() }).collect();
        self.lines.push(Line { role: ChatRole::User, content: text.clone(), generation: None });
        self.error = None;
        self.sync_warning = false;
        let pending = Pending { request: Request::Send { key, message: text, history }, index: self.lines.len(), step: Step::Starting { replayed: false } };
        let effect = self.request_effect(&pending);
        self.pending = Some(pending);
        vec![effect]
    }

    fn retry(&mut self, index: usize, key: RequestKey) -> Vec<Effect> {
        if self.pending.is_some() || self.loading_history || !self.consent || index == 0 {
            return vec![];
        }
        let Some(of) = self.lines.get(index).filter(|l| l.role == ChatRole::Assistant).and_then(|l| l.generation) else { return vec![] };
        if self.lines.get(index - 1).is_none_or(|l| l.role != ChatRole::User) {
            return vec![];
        }
        self.lines[index].content.clear();
        self.error = None;
        self.sync_warning = false;
        let pending = Pending { request: Request::Retry { key, of }, index, step: Step::Starting { replayed: false } };
        let effects = vec![self.request_effect(&pending), self.telemetry("ai_chat_message_retried", index)];
        self.pending = Some(pending);
        effects
    }

    fn request_effect(&self, p: &Pending) -> Effect {
        match &p.request {
            Request::Send { key, message, history } => Effect::StartChat {
                job: self.job.expect("a question needs a selected job"),
                key: *key,
                message: message.clone(),
                history: history.clone(),
            },
            Request::Retry { key, of } => Effect::RetryChat { generation: *of, key: *key },
            Request::Attach => unreachable!("an attachment has no key, so it is never started or replayed"),
        }
    }

    /// Shows another tab's running answer as this tab's streaming answer and
    /// attaches its watch (P5 D6): no start, retry, key or quota. The held
    /// partial is shown at once; the watch's first snapshot replaces it.
    fn attach(&mut self, in_flight: InFlightChat) -> Vec<Effect> {
        let generation = in_flight.generation.id;
        self.lines.push(Line { role: ChatRole::User, content: in_flight.question, generation: None });
        let index = self.lines.len();
        if let Draft::Text { text } = in_flight.generation.draft
            && !text.is_empty()
        {
            self.lines.push(Line { role: ChatRole::Assistant, content: text, generation: None });
        }
        self.stream.clear();
        self.pending = Some(Pending { request: Request::Attach, index, step: Step::Streaming { generation, started: false, reconnected: false } });
        vec![Effect::Watch { generation, kind: WatchKind::Text, after: None }]
    }

    fn frame(&mut self, generation: GenerationId, frame: ApplicationFrame) -> Vec<Effect> {
        let Some(p) = self.pending.as_mut() else { return vec![] };
        let Step::Streaming { generation: g, started, reconnected } = p.step else { return vec![] };
        if g != generation || frame.identity.generation != generation.to_string() {
            return vec![];
        }
        if !started {
            self.stream.start(frame.identity.clone());
            p.step = Step::Streaming { generation, started: true, reconnected };
        }
        let index = p.index;
        match self.stream.update(frame) {
            Ok(StreamUpdate::Applied { .. }) => {}
            Ok(StreamUpdate::Ignored | StreamUpdate::ResnapshotRequired) => return vec![],
            Err(_) => {
                self.fail(ChatErrorVm { message: Some(unconfirmed()), detail: None });
                return vec![];
            }
        }
        let (text, status) = {
            let view = self.stream.view();
            (view.text.to_string(), view.status)
        };
        if !text.is_empty() || index < self.lines.len() {
            self.set_answer(index, text);
        }
        match status {
            StreamStatus::Completed => self.succeed(generation),
            StreamStatus::Interrupted | StreamStatus::Cancelled | StreamStatus::Revoked => {
                if let Some(p) = self.pending.as_mut() {
                    p.step = Step::Reading { generation, reconnected };
                }
                vec![Effect::LoadGeneration { generation }]
            }
            StreamStatus::Starting | StreamStatus::Streaming => vec![],
        }
    }

    fn generation_loaded(&mut self, generation: GenerationId, result: Result<GenerationView, ApiFailure>) -> Vec<Effect> {
        let Some(p) = self.pending.as_ref() else { return vec![] };
        let Step::Reading { generation: g, reconnected } = p.step else { return vec![] };
        if g != generation {
            return vec![];
        }
        let index = p.index;
        let view = match result {
            Ok(view) => view,
            Err(_) => {
                self.fail(ChatErrorVm { message: Some(unconfirmed()), detail: None });
                return vec![];
            }
        };
        let draft = match &view.draft {
            Draft::Text { text } => text.clone(),
            _ => String::new(),
        };
        match view.status {
            GenerationStatus::Accepted | GenerationStatus::Running => {
                if let Some(p) = self.pending.as_mut() {
                    p.step = Step::Streaming { generation, started: true, reconnected };
                }
                self.stream.resume();
                vec![Effect::Watch { generation, kind: WatchKind::Text, after: None }]
            }
            GenerationStatus::Completed => {
                if let Some(FinalContent::Text { text }) = &view.final_content {
                    self.set_answer(index, text.clone());
                }
                self.succeed(generation)
            }
            GenerationStatus::Interrupted if matches!(view.interruption, Some(Interruption::NotSaved { .. })) && !draft.is_empty() => {
                self.set_answer(index, draft);
                self.pending = None;
                self.sync_warning = true;
                vec![]
            }
            GenerationStatus::Interrupted | GenerationStatus::Cancelled => {
                let detail = view.interruption.as_ref().map(|i| public_error(i).message);
                self.fail(ChatErrorVm { message: None, detail });
                vec![]
            }
        }
    }

    fn set_answer(&mut self, index: usize, text: String) {
        if let Some(line) = self.lines.get_mut(index) {
            line.content = text;
        } else if index == self.lines.len() {
            self.lines.push(Line { role: ChatRole::Assistant, content: text, generation: None });
        }
    }

    fn succeed(&mut self, generation: GenerationId) -> Vec<Effect> {
        let Some(p) = self.pending.take() else { return vec![] };
        if let Some(answer) = self.lines.get_mut(p.index) {
            answer.generation = Some(generation);
        }
        match p.request {
            Request::Send { .. } | Request::Attach => {
                if let Some(question) = p.index.checked_sub(1).and_then(|i| self.lines.get_mut(i)) {
                    question.generation = Some(generation);
                }
                self.job.map(|job| Effect::LoadLimits { job }).into_iter().collect()
            }
            Request::Retry { .. } => vec![],
        }
    }

    /// The source's failure: a new question loses its last displayed message;
    /// a retried answer keeps its emptied placeholder.
    fn fail(&mut self, error: ChatErrorVm) {
        if let Some(p) = self.pending.take()
            && matches!(p.request, Request::Send { .. } | Request::Attach)
        {
            self.lines.pop();
        }
        self.error = Some(error);
    }

    fn detach(&self) -> Vec<Effect> {
        match self.pending.as_ref().map(|p| &p.step) {
            Some(Step::Streaming { generation, .. }) => vec![Effect::Detach { generation: *generation }],
            _ => vec![],
        }
    }

    fn telemetry(&self, name: &'static str, index: usize) -> Effect {
        let job = self.job.map_or(0.0, |j| j.0 as f64);
        Effect::Telemetry { event: TelemetryEvent::new(name).number("jobId", job).number("messageIndex", index as f64) }
    }

    /// `Question: …` / `Answer: …` plus a blank line, joined by newlines.
    fn serialize(&self) -> String {
        let mut out: Vec<String> = Vec::with_capacity(self.lines.len() * 2);
        for line in &self.lines {
            match line.role {
                ChatRole::User => out.push(format!("Question: {}", line.content)),
                ChatRole::Assistant => {
                    out.push(format!("Answer: {}", line.content));
                    out.push(String::new());
                }
            }
        }
        out.join("\n")
    }
}

fn unconfirmed() -> Msg {
    app("The AI request could not be confirmed. Check your connection and retry.")
}

/// A chat cap maps to the source's limit key with its interpolation argument;
/// any other refusal shows the server's text.
fn rejection_error(body: Option<StartRejection>) -> ChatErrorVm {
    let Some(body) = body else { return ChatErrorVm { message: Some(unconfirmed()), detail: None } };
    let limit = match body.code.as_str() {
        "TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED" => Some(("TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED", "maxQuestionsPerTranscript")),
        "USER_QUESTIONS_LIMIT_EXCEEDED" => Some(("USER_QUESTIONS_LIMIT_EXCEEDED", "maxQuestionsPerUser")),
        "TRANSCRIPT_TOKENS_LIMIT_EXCEEDED" => Some(("TRANSCRIPT_TOKENS_LIMIT_EXCEEDED", "maxTokensPerTranscript")),
        "USER_TOKENS_LIMIT_EXCEEDED" => Some(("USER_TOKENS_LIMIT_EXCEEDED", "maxTokensPerUser")),
        _ => None,
    };
    match (limit, body.max) {
        (Some((key, argument)), Some(max)) => ChatErrorVm { message: Some(ai(key).number(argument, max)), detail: None },
        _ => ChatErrorVm { message: None, detail: Some(body.error).filter(|e| !e.is_empty()) },
    }
}

/// `formatLimitsForDisplay`: the per-transcript cap wins for each counter;
/// only a per-user cap carries a reset.
fn limits_vm(limits: &LimitsView, now: Micros) -> Option<LimitsVm> {
    let mut reset_at = None;
    let mut pick = |transcript: &Option<LimitUsage>, user: &Option<LimitUsage>| match (transcript, user) {
        (Some(t), _) => Some(usage(t)),
        (None, Some(u)) => {
            reset_at = u.reset_at.or(reset_at);
            Some(usage(u))
        }
        (None, None) => None,
    };
    let questions = pick(&limits.questions_per_transcript, &limits.questions_per_user);
    let tokens = pick(&limits.tokens_per_transcript, &limits.tokens_per_user);
    let questions = questions.filter(|u| u.max > 0);
    let tokens = tokens.filter(|u| u.max > 0);
    if questions.is_none() && tokens.is_none() {
        return None;
    }
    Some(LimitsVm { questions, tokens, reset: reset_at.map(|at| reset_label(at, now)) })
}

fn usage(u: &LimitUsage) -> UsageVm {
    let percent = if u.max > 0 { u.current as f64 / u.max as f64 * 100.0 } else { 0.0 };
    let tone = if percent >= 90.0 {
        Tone::Danger
    } else if percent >= 75.0 {
        Tone::Progress
    } else {
        Tone::Accent
    };
    UsageVm { current: u.current, max: u.max, width_percent: percent.clamp(0.0, 100.0).round() as u8, tone }
}

fn reset_label(at: Micros, now: Micros) -> ResetLabel {
    const HOUR: i64 = 60 * MICROS_PER_MINUTE;
    let diff = at.0 - now.0;
    let hours = ceil_div(diff, HOUR);
    let days = ceil_div(diff, 24 * HOUR);
    if hours < 1 {
        ResetLabel::LessThanHour
    } else if hours < 24 {
        ResetLabel::InHours { hours }
    } else if days < 7 {
        ResetLabel::InDays { days }
    } else {
        ResetLabel::On { at }
    }
}

/// `Math.ceil(a / b)` for a positive divisor.
fn ceil_div(a: i64, b: i64) -> i64 {
    let q = a.div_euclid(b);
    if a.rem_euclid(b) == 0 { q } else { q + 1 }
}

#[cfg(test)]
mod tests;
