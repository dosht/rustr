//! Neutral completion ports and text-stream presentation. No I/O or dependencies.
//! A sanitized checkpoint replaces text; it is never a prefix for raw deltas.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role { System, User, Assistant }

#[derive(Clone, PartialEq, Eq)]
pub struct Message { pub role: Role, pub content: String }
impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message").field("role", &self.role).field("content_bytes", &self.content.len()).finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseFormat { JsonObject }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub sse_bytes: usize,
    pub json_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { input_bytes: 16 * 1024 * 1024, output_bytes: 1024 * 1024, sse_bytes: 256 * 1024, json_depth: 32 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f64,
    pub max_output_tokens: u32,
    pub response_format: Option<ResponseFormat>,
    pub timeout: Duration,
    pub limits: Limits,
}
impl CompletionRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self { model: model.into(), messages, temperature: 0.3, max_output_tokens: 1000,
            response_format: None, timeout: Duration::from_secs(60), limits: Limits::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteOutcome {
    /// No HTTP dispatch was attempted.
    NotDispatched,
    /// A non-success HTTP response was observed; this is not deletion evidence.
    Rejected,
    /// Dispatch/completion may have happened; local abort cannot resolve it.
    Unknown,
    /// A protocol terminal was observed, not proof of external non-retention.
    Finished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind { Configuration, InvalidRequest, Capacity, Http { status: u16, retry_after: Option<String> }, Transport, Timeout, Cancelled, Protocol, Limit }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmError { pub kind: ErrorKind, pub remote: RemoteOutcome, pub remote_id: Option<String> }
impl LlmError {
    pub fn new(kind: ErrorKind, remote: RemoteOutcome) -> Self { Self { kind, remote, remote_id: None } }
}
impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "completion {:?} ({:?})", self.kind, self.remote) }
}
impl std::error::Error for LlmError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage { pub input_tokens: Option<u64>, pub output_tokens: Option<u64>, pub total_tokens: Option<u64> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FinishReason { Stop, Length, ContentFilter, ToolCalls, Other(String) }
impl FinishReason { pub fn is_success(&self) -> bool { matches!(self, Self::Stop) } }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionFinish { pub reason: FinishReason, pub usage: Option<Usage>, pub remote_id: Option<String> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionEvent {
    Metadata { remote_id: Option<String>, usage: Option<Usage> },
    Delta(String),
    /// Emitted only after provider terminal evidence, not ordinary socket EOF.
    Finished(CompletionFinish),
}

pub trait Llm: Send + Sync {
    fn open(&self, request: CompletionRequest) -> BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>>;
}
pub trait CompletionStream: Send {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<CompletionEvent>, LlmError>>;
    /// Stops local consumption and releases transport capacity. No remote guarantee.
    fn cancel(&mut self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamIdentity {
    pub generation: String,
    pub content_revision: u64,
    pub consent_epoch: u64,
    pub execution: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamStatus { Starting, Streaming, Completed, Interrupted, Cancelled, Revoked }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persistence { Provisional, Unsaved, Saved }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryMode { RawDelta, SnapshotReplacement }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicError { pub code: String, pub message: String }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextEvent {
    Delta(String),
    Snapshot { text: String, status: StreamStatus, persistence: Persistence },
    Final { text: String, persistence: Persistence },
    Interrupted(PublicError),
    Revoked,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationFrame { pub identity: StreamIdentity, pub seq: u64, pub event: TextEvent }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamUpdate { Applied { discard_deltas: bool }, Ignored, ResnapshotRequired }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError { OutputLimit, InvalidSnapshot }
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "stream {self:?}") }
}
impl std::error::Error for StreamError {}

#[derive(Clone, Copy, Debug)]
pub struct StreamView<'a> {
    pub identity: Option<&'a StreamIdentity>,
    pub text: &'a str,
    pub status: StreamStatus,
    pub mode: DeliveryMode,
    pub persistence: Persistence,
    pub last_seq: Option<u64>,
    pub durable_seq: Option<u64>,
    pub error: Option<&'a PublicError>,
}

#[derive(Debug)]
pub struct StreamState {
    identity: Option<StreamIdentity>, text: String, status: StreamStatus,
    mode: DeliveryMode, persistence: Persistence, last_seq: Option<u64>,
    durable_seq: Option<u64>, error: Option<PublicError>, limit: usize,
    awaiting_snapshot: bool, detached: bool,
}
impl Default for StreamState { fn default() -> Self { Self::new(Limits::default().output_bytes) } }
impl StreamState {
    pub fn new(output_bytes: usize) -> Self {
        Self { identity: None, text: String::new(), status: StreamStatus::Starting,
            mode: DeliveryMode::RawDelta, persistence: Persistence::Provisional,
            last_seq: None, durable_seq: None, error: None, limit: output_bytes,
            awaiting_snapshot: false, detached: false }
    }
    pub fn start(&mut self, identity: StreamIdentity) {
        self.identity = Some(identity); self.text.clear(); self.status = StreamStatus::Starting;
        self.mode = DeliveryMode::RawDelta; self.persistence = Persistence::Provisional;
        self.last_seq = None; self.durable_seq = None; self.error = None;
        self.awaiting_snapshot = false; self.detached = false;
    }
    /// Called before reattachment. The first durable snapshot may drop a lost suffix.
    pub fn resume(&mut self) {
        if self.identity.is_some() && self.status != StreamStatus::Revoked {
            self.mode = DeliveryMode::SnapshotReplacement; self.awaiting_snapshot = true; self.detached = false;
        }
    }
    pub fn detach(&mut self) { self.detached = true; }
    pub fn cancel(&mut self) {
        self.detached = true;
        if !matches!(self.status, StreamStatus::Completed | StreamStatus::Revoked) { self.status = StreamStatus::Cancelled; }
    }
    pub fn clear(&mut self) {
        let limit = self.limit; *self = Self::new(limit);
    }
    pub fn view(&self) -> StreamView<'_> {
        StreamView { identity: self.identity.as_ref(), text: &self.text, status: self.status,
            mode: self.mode, persistence: self.persistence, last_seq: self.last_seq,
            durable_seq: self.durable_seq, error: self.error.as_ref() }
    }
    pub fn update(&mut self, frame: ApplicationFrame) -> Result<StreamUpdate, StreamError> {
        if self.identity.as_ref() != Some(&frame.identity) || self.detached || self.status == StreamStatus::Revoked {
            return Ok(StreamUpdate::Ignored);
        }
        let is_snapshot = matches!(&frame.event, TextEvent::Snapshot { .. });
        let is_final = matches!(&frame.event, TextEvent::Final { .. });
        let is_revoke = matches!(&frame.event, TextEvent::Revoked);
        // A host control interruption is authoritative like Final/Revoked: after resume it
        // is ordered against the durable checkpoint, not raw provisional sequence numbers.
        let is_interrupt = matches!(&frame.event, TextEvent::Interrupted(_));
        if self.status == StreamStatus::Completed && !is_revoke { return Ok(StreamUpdate::Ignored); }
        if matches!(self.status, StreamStatus::Interrupted | StreamStatus::Cancelled)
            && !self.awaiting_snapshot && !is_final && !is_revoke {
            return Ok(StreamUpdate::Ignored);
        }
        if self.awaiting_snapshot && !is_snapshot && !is_final && !is_revoke && !is_interrupt {
            return Ok(StreamUpdate::ResnapshotRequired);
        }
        let resumed_durable = self.awaiting_snapshot && (is_snapshot || is_interrupt);
        if self.last_seq.is_some_and(|last| frame.seq <= last) && !resumed_durable {
            return Ok(StreamUpdate::Ignored);
        }
        if self.awaiting_snapshot && is_snapshot && self.durable_seq.is_some_and(|last| frame.seq < last) {
            return Ok(StreamUpdate::Ignored);
        }
        if self.awaiting_snapshot && is_interrupt && self.durable_seq.is_some_and(|last| frame.seq <= last) {
            return Ok(StreamUpdate::Ignored);
        }
        let discard_deltas = is_snapshot || is_final || is_revoke;
        match frame.event {
            TextEvent::Delta(delta) => {
                if self.mode == DeliveryMode::SnapshotReplacement || self.last_seq.is_some_and(|last| last.checked_add(1) != Some(frame.seq)) {
                    self.awaiting_snapshot = true;
                    return Ok(StreamUpdate::ResnapshotRequired);
                }
                if delta.len() > self.limit.saturating_sub(self.text.len()) { return Err(StreamError::OutputLimit); }
                self.text.push_str(&delta); self.status = StreamStatus::Streaming;
                self.persistence = Persistence::Provisional;
            }
            TextEvent::Snapshot { text, status, persistence } => {
                if text.len() > self.limit { return Err(StreamError::OutputLimit); }
                if status == StreamStatus::Completed && persistence != Persistence::Saved { return Err(StreamError::InvalidSnapshot); }
                self.text = text; self.status = status; self.persistence = persistence;
                self.mode = DeliveryMode::SnapshotReplacement; self.awaiting_snapshot = false;
                self.durable_seq = Some(frame.seq); self.error = None;
                if status == StreamStatus::Revoked { self.text.clear(); }
            }
            TextEvent::Final { text, persistence } => {
                if text.len() > self.limit { return Err(StreamError::OutputLimit); }
                self.text = text; self.persistence = persistence;
                self.status = if persistence == Persistence::Saved { StreamStatus::Completed } else { StreamStatus::Interrupted };
                self.mode = DeliveryMode::SnapshotReplacement; self.awaiting_snapshot = false; self.error = None;
                if persistence == Persistence::Saved { self.durable_seq = Some(frame.seq); }
            }
            TextEvent::Interrupted(error) => { self.status = StreamStatus::Interrupted; self.error = Some(error); }
            TextEvent::Revoked => {
                self.text.clear(); self.error = None; self.status = StreamStatus::Revoked;
                self.mode = DeliveryMode::SnapshotReplacement; self.awaiting_snapshot = false;
                self.durable_seq = Some(frame.seq);
            }
        }
        // A control frame never moves the cursor backwards; resumed snapshots may replace a lost suffix.
        self.last_seq = Some(match self.last_seq { Some(last) if is_interrupt => last.max(frame.seq), _ => frame.seq });
        Ok(StreamUpdate::Applied { discard_deltas })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueError { SlowConsumer, OutputLimit }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueUpdate { Queued, Ignored, SnapshotRequired }

/// A single attachment's queue, not a dispatcher, log, or authorization boundary.
#[derive(Debug)]
pub struct AttachmentQueue {
    identity: StreamIdentity, deltas: VecDeque<ApplicationFrame>, replacement: Option<ApplicationFrame>,
    bytes: usize, delta_limit: usize, output_limit: usize, mode: DeliveryMode, detached: bool,
}
impl AttachmentQueue {
    pub fn new(identity: StreamIdentity, delta_bytes: usize, output_bytes: usize) -> Self {
        Self { identity, deltas: VecDeque::new(), replacement: None, bytes: 0,
            delta_limit: delta_bytes, output_limit: output_bytes, mode: DeliveryMode::RawDelta, detached: false }
    }
    pub fn push(&mut self, frame: ApplicationFrame) -> Result<QueueUpdate, QueueError> {
        if self.detached { return Err(QueueError::SlowConsumer); }
        if frame.identity != self.identity { return Ok(QueueUpdate::Ignored); }
        let replacement_text = match &frame.event {
            TextEvent::Snapshot { text, .. } | TextEvent::Final { text, .. } => Some(text.len()),
            TextEvent::Revoked => Some(0), _ => None,
        };
        if let Some(size) = replacement_text {
            if size > self.output_limit { self.detach(); return Err(QueueError::OutputLimit); }
            if self.replacement.as_ref().is_some_and(|last| last.seq > frame.seq) { return Ok(QueueUpdate::Ignored); }
            self.deltas.clear(); self.bytes = 0; self.mode = DeliveryMode::SnapshotReplacement;
            self.replacement = Some(frame); return Ok(QueueUpdate::Queued);
        }
        if matches!(&frame.event, TextEvent::Delta(_)) && self.mode == DeliveryMode::SnapshotReplacement {
            return Ok(QueueUpdate::SnapshotRequired);
        }
        // Charge envelope/identity storage too: zero-length deltas cannot grow an unbounded queue.
        let size = queued_bytes(&frame);
        if size > self.delta_limit.saturating_sub(self.bytes) { self.detach(); return Err(QueueError::SlowConsumer); }
        self.bytes += size; self.deltas.push_back(frame); Ok(QueueUpdate::Queued)
    }
    pub fn pop(&mut self) -> Option<ApplicationFrame> {
        if let Some(snapshot) = self.replacement.take() { return Some(snapshot); }
        let frame = self.deltas.pop_front()?;
        self.bytes = self.bytes.saturating_sub(queued_bytes(&frame));
        Some(frame)
    }
    pub fn detach(&mut self) { self.detached = true; self.deltas.clear(); self.replacement = None; self.bytes = 0; }
    pub fn is_detached(&self) -> bool { self.detached }
}

fn queued_bytes(frame: &ApplicationFrame) -> usize {
    let payload = match &frame.event {
        TextEvent::Delta(s) => s.capacity(),
        TextEvent::Interrupted(e) => e.code.capacity().saturating_add(e.message.capacity()),
        _ => 0,
    };
    payload.saturating_add(frame.identity.generation.capacity())
        .saturating_add(frame.identity.execution.capacity()).saturating_add(std::mem::size_of::<ApplicationFrame>())
}
