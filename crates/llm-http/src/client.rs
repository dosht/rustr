//! Native host-stream client. wasm shells feed fetch ReadableStream bytes into
//! the same portable ApplicationDecoder; provider transports never compile there.
use bytes::Bytes;
use llm_core::{ApplicationFrame, Limits, Persistence, StreamStatus, TextEvent};
use reqwest::{Client, Response};
use tokio::time::{Instant, timeout_at};
use std::time::Duration;
use crate::{ApplicationDecoder, CodecError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError { Configuration, Http(u16), Transport, Timeout, Codec(CodecError), Incomplete, Cancelled }
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "host stream {self:?}") }
}
impl std::error::Error for ClientError {}
#[derive(Clone)]
pub struct ApplicationClient { client: Client, limits: Limits, timeout: Duration }
impl ApplicationClient {
    pub fn new(limits: Limits, timeout: Duration) -> Result<Self, ClientError> {
        if timeout.is_zero() || limits.output_bytes == 0 || limits.json_depth == 0 { return Err(ClientError::Configuration); }
        Ok(Self { client: crate::provider::http_client().map_err(|_| ClientError::Configuration)?, limits, timeout })
    }
    /// `url` is the published host watch endpoint, including its numeric cursor.
    /// Bearer credentials stay in headers; this method does not start generation.
    pub async fn open(&self, url: &str, bearer: Option<&str>) -> Result<ApplicationStream, ClientError> {
        let url = crate::provider::configured_url(url, true).map_err(|_| ClientError::Configuration)?;
        if url.query_pairs().any(|(k, _)| k != "after" && k != "after_seq") { return Err(ClientError::Configuration); }
        if url.query_pairs().any(|(_, v)| v.parse::<u64>().is_err()) { return Err(ClientError::Configuration); }
        let deadline = Instant::now().checked_add(self.timeout).ok_or(ClientError::Configuration)?;
        let mut request = self.client.get(url).header("Accept", "text/event-stream");
        if let Some(bearer) = bearer { request = request.bearer_auth(bearer); }
        let response = timeout_at(deadline, request.send()).await.map_err(|_| ClientError::Timeout)?.map_err(|_| ClientError::Transport)?;
        if !response.status().is_success() { return Err(ClientError::Http(response.status().as_u16())); }
        if !response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|s| s.split(';').next().is_some_and(|s| s.trim().eq_ignore_ascii_case("text/event-stream"))) {
            return Err(ClientError::Codec(CodecError::Event));
        }
        Ok(ApplicationStream { response: Some(response), decoder: ApplicationDecoder::new(self.limits),
            chunk: Bytes::new(), offset: 0, deadline, terminal: false, cancelled: false })
    }
}
pub struct ApplicationStream {
    response: Option<Response>, decoder: ApplicationDecoder, chunk: Bytes, offset: usize,
    deadline: Instant, terminal: bool, cancelled: bool,
}
impl ApplicationStream {
    pub fn cancel(&mut self) { self.response = None; self.chunk = Bytes::new(); self.offset = 0; self.terminal = true; self.cancelled = true; }
    pub async fn next(&mut self) -> Result<Option<ApplicationFrame>, ClientError> {
        if self.cancelled { self.cancelled = false; return Err(ClientError::Cancelled); }
        if self.terminal { return Ok(None); }
        let result = self.read_next().await;
        if result.is_err() { self.response = None; self.chunk = Bytes::new(); self.terminal = true; }
        result
    }
    async fn read_next(&mut self) -> Result<Option<ApplicationFrame>, ClientError> {
        loop {
            if Instant::now() >= self.deadline { return Err(ClientError::Timeout); }
            if self.offset < self.chunk.len() {
                let (used, frame) = self.decoder.push(&self.chunk[self.offset..]).map_err(ClientError::Codec)?;
                self.offset += used;
                if let Some(frame) = frame {
                    self.terminal = matches!(&frame.event,
                        TextEvent::Final { persistence: Persistence::Saved, .. } | TextEvent::Interrupted(_) | TextEvent::Revoked
                        | TextEvent::Snapshot { status: StreamStatus::Completed | StreamStatus::Interrupted | StreamStatus::Cancelled | StreamStatus::Revoked, .. });
                    if self.terminal { self.response = None; self.chunk = Bytes::new(); self.offset = 0; }
                    return Ok(Some(frame));
                }
                continue;
            }
            let response = self.response.as_mut().ok_or(ClientError::Incomplete)?;
            match timeout_at(self.deadline, response.chunk()).await {
                Ok(Ok(Some(chunk))) => { self.chunk = chunk; self.offset = 0; }
                Ok(Ok(None)) => { self.decoder.finish().map_err(ClientError::Codec)?; return Err(ClientError::Incomplete); }
                Ok(Err(_)) => return Err(ClientError::Transport),
                Err(_) => return Err(ClientError::Timeout),
            }
        }
    }
}
