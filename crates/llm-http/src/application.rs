//! Portable host-to-shell text frames. No provider credentials or product payloads.
use llm_core::{ApplicationFrame, Limits, Persistence, PublicError, StreamIdentity, StreamStatus, TextEvent};
use serde::{Deserialize, Serialize};
use crate::sse::{CodecError, SseDecoder, check_json};

#[derive(Serialize)]
struct IdentityRef<'a> { generation: &'a str, content_revision: u64, consent_epoch: u64, execution: &'a str }
#[derive(Serialize)]
struct FrameRef<'a> { version: u8, identity: IdentityRef<'a>, seq: u64, event: EventRef<'a> }
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventRef<'a> {
    Delta { text: &'a str },
    Snapshot { text: &'a str, status: &'static str, persistence: &'static str },
    Final { text: &'a str, persistence: &'static str },
    Interrupted { code: &'a str, message: &'a str },
    Revoked,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityWire { generation: String, content_revision: u64, consent_epoch: u64, execution: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameWire { version: u8, identity: IdentityWire, seq: u64, event: EventWire }
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum EventWire {
    Delta { text: String },
    Snapshot { text: String, status: String, persistence: String },
    Final { text: String, persistence: String },
    Interrupted { code: String, message: String },
    Revoked,
}
fn status(s: StreamStatus) -> &'static str {
    match s { StreamStatus::Starting => "starting", StreamStatus::Streaming => "streaming", StreamStatus::Completed => "completed", StreamStatus::Interrupted => "interrupted", StreamStatus::Cancelled => "cancelled", StreamStatus::Revoked => "revoked" }
}
fn persistence(p: Persistence) -> &'static str {
    match p { Persistence::Provisional => "provisional", Persistence::Unsaved => "unsaved", Persistence::Saved => "saved" }
}
fn parse_status(s: &str) -> Result<StreamStatus, CodecError> {
    match s { "starting" => Ok(StreamStatus::Starting), "streaming" => Ok(StreamStatus::Streaming), "completed" => Ok(StreamStatus::Completed), "interrupted" => Ok(StreamStatus::Interrupted), "cancelled" => Ok(StreamStatus::Cancelled), "revoked" => Ok(StreamStatus::Revoked), _ => Err(CodecError::Envelope) }
}
fn parse_persistence(s: &str) -> Result<Persistence, CodecError> {
    match s { "provisional" => Ok(Persistence::Provisional), "unsaved" => Ok(Persistence::Unsaved), "saved" => Ok(Persistence::Saved), _ => Err(CodecError::Envelope) }
}
fn valid_id(s: &str) -> bool { !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control) }
const MAX_ERROR_CODE_BYTES: usize = 256;
const MAX_ERROR_MESSAGE_BYTES: usize = 4096;
/// JSON escapes a control byte as six bytes. The largest encodable payload is either the
/// output-limited text or the bounded public error; 4096 covers identity (control
/// characters are rejected, so at most 2x escaping), keys and the SSE id/event lines.
fn wire_limit(limits: Limits) -> usize {
    limits.output_bytes.max(MAX_ERROR_CODE_BYTES + MAX_ERROR_MESSAGE_BYTES).saturating_mul(6).saturating_add(4096)
}
fn validate(frame: &ApplicationFrame, limits: Limits) -> Result<(), CodecError> {
    if !valid_id(&frame.identity.generation) || !valid_id(&frame.identity.execution) { return Err(CodecError::Envelope); }
    match &frame.event {
        TextEvent::Delta(s) | TextEvent::Final { text: s, .. } | TextEvent::Snapshot { text: s, .. } if s.len() > limits.output_bytes => return Err(CodecError::Limit),
        TextEvent::Interrupted(e) if e.code.len() > MAX_ERROR_CODE_BYTES || e.message.len() > MAX_ERROR_MESSAGE_BYTES => return Err(CodecError::Limit),
        _ => {}
    }
    if matches!(frame.event, TextEvent::Snapshot { status: StreamStatus::Completed, persistence: Persistence::Provisional | Persistence::Unsaved, .. }) {
        return Err(CodecError::Envelope);
    }
    Ok(())
}
/// JSON snapshot form. Text is borrowed during serialization, not cloned into a tree.
pub fn encode_application_json(frame: &ApplicationFrame, limits: Limits) -> Result<String, CodecError> {
    validate(frame, limits)?;
    let event = match &frame.event {
        TextEvent::Delta(text) => EventRef::Delta { text },
        TextEvent::Snapshot { text, status: s, persistence: p } => EventRef::Snapshot { text, status: status(*s), persistence: persistence(*p) },
        TextEvent::Final { text, persistence: p } => EventRef::Final { text, persistence: persistence(*p) },
        TextEvent::Interrupted(e) => EventRef::Interrupted { code: &e.code, message: &e.message },
        TextEvent::Revoked => EventRef::Revoked,
    };
    let identity = IdentityRef { generation: &frame.identity.generation, content_revision: frame.identity.content_revision,
        consent_epoch: frame.identity.consent_epoch, execution: &frame.identity.execution };
    let json = serde_json::to_string(&FrameRef { version: 1, identity, seq: frame.seq, event }).map_err(|_| CodecError::Json)?;
    if json.len() > wire_limit(limits) { return Err(CodecError::Limit); }
    Ok(json)
}
pub fn decode_application_json(bytes: &[u8], limits: Limits) -> Result<ApplicationFrame, CodecError> {
    if bytes.len() > wire_limit(limits) { return Err(CodecError::Limit); }
    check_json(bytes, limits.json_depth)?;
    let wire: FrameWire = serde_json::from_slice(bytes).map_err(|_| CodecError::Json)?;
    if wire.version != 1 { return Err(CodecError::Envelope); }
    let event = match wire.event {
        EventWire::Delta { text } => TextEvent::Delta(text),
        EventWire::Snapshot { text, status, persistence } => TextEvent::Snapshot { text, status: parse_status(&status)?, persistence: parse_persistence(&persistence)? },
        EventWire::Final { text, persistence } => TextEvent::Final { text, persistence: parse_persistence(&persistence)? },
        EventWire::Interrupted { code, message } => TextEvent::Interrupted(PublicError { code, message }),
        EventWire::Revoked => TextEvent::Revoked,
    };
    let i = wire.identity;
    let frame = ApplicationFrame { identity: StreamIdentity { generation: i.generation, content_revision: i.content_revision,
        consent_epoch: i.consent_epoch, execution: i.execution }, seq: wire.seq, event };
    validate(&frame, limits)?; Ok(frame)
}
/// SSE uses `event: text`, `id: generation:seq`, one JSON data record.
pub fn encode_frame(frame: &ApplicationFrame, limits: Limits) -> Result<String, CodecError> {
    let json = encode_application_json(frame, limits)?;
    let wire = format!("event: text\nid: {}:{}\ndata: {json}\n\n", frame.identity.generation, frame.seq);
    // Never emit a record the matching ApplicationDecoder would refuse.
    if wire.len() > wire_limit(limits) { return Err(CodecError::Limit); }
    Ok(wire)
}
pub struct ApplicationDecoder { sse: SseDecoder, limits: Limits }
impl ApplicationDecoder {
    pub fn new(limits: Limits) -> Self {
        // Provider records are 256 KiB; application snapshots may contain 1 MiB of
        // text. Reserve JSON's worst-case escape expansion without raising text limits.
        Self { sse: SseDecoder::new(wire_limit(limits)), limits }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(usize, Option<ApplicationFrame>), CodecError> {
        let (used, event) = self.sse.push(bytes)?;
        let Some(event) = event else { return Ok((used, None)); };
        if !event.event.is_empty() && event.event != "text" { return Err(CodecError::Event); }
        let frame = decode_application_json(event.data.as_bytes(), self.limits)?;
        if event.id.as_ref().is_some_and(|id| *id != format!("{}:{}", frame.identity.generation, frame.seq)) { return Err(CodecError::Envelope); }
        Ok((used, Some(frame)))
    }
    pub fn finish(&self) -> Result<(), CodecError> { self.sse.finish() }
}
