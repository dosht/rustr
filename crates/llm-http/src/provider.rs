//! Native-only upstream clients. No automatic retries or model/provider fallback.
use std::collections::VecDeque;
use std::fmt;
use std::io::Write;
use std::sync::{Arc, LazyLock};
use bytes::Bytes;
use llm_core::*;
use reqwest::{Client, Response, Url};
use serde::{Deserialize, Serialize, ser::SerializeSeq};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, timeout_at};
use crate::sse::{SseDecoder, check_json};

pub struct ProviderConfig { pub base_url: String, api_key: String }
impl ProviderConfig {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), api_key: api_key.into() }
    }
}
impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig").field("api_key", &"[REDACTED]").finish_non_exhaustive()
    }
}
#[derive(Clone)]
pub struct DispatchCapacity { semaphore: Arc<Semaphore> }
impl Default for DispatchCapacity {
    fn default() -> Self {
        static PROCESS_CAPACITY: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(8)));
        Self { semaphore: PROCESS_CAPACITY.clone() }
    }
}
impl DispatchCapacity {
    pub fn new(maximum: usize) -> Result<Self, LlmError> {
        if maximum == 0 || maximum > Semaphore::MAX_PERMITS { return Err(error(ErrorKind::Configuration, RemoteOutcome::NotDispatched)); }
        Ok(Self { semaphore: Arc::new(Semaphore::new(maximum)) })
    }
    fn acquire(&self) -> Result<OwnedSemaphorePermit, LlmError> {
        self.semaphore.clone().try_acquire_owned().map_err(|_| error(ErrorKind::Capacity, RemoteOutcome::NotDispatched))
    }
}
#[derive(Clone, Copy)]
enum Provider { DeepSeek, OpenAi }
pub struct HttpLlm { client: Client, endpoint: Url, key: String, provider: Provider, capacity: DispatchCapacity }
impl fmt::Debug for HttpLlm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("HttpLlm").finish_non_exhaustive() }
}
fn error(kind: ErrorKind, remote: RemoteOutcome) -> LlmError { LlmError::new(kind, remote) }
pub(crate) fn configured_url(value: &str, query: bool) -> Result<Url, LlmError> {
    let url = Url::parse(value).map_err(|_| error(ErrorKind::Configuration, RemoteOutcome::NotDispatched))?;
    let local = url.host_str().and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok()).is_some_and(|ip| ip.is_loopback());
    if url.host_str().is_none() || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || (!query && url.query().is_some()) {
        return Err(error(ErrorKind::Configuration, RemoteOutcome::NotDispatched));
    }
    Ok(url)
}
pub(crate) fn http_client() -> Result<Client, LlmError> {
    Client::builder().connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never()).no_proxy().referer(false)
        .build().map_err(|_| error(ErrorKind::Configuration, RemoteOutcome::NotDispatched))
}
impl HttpLlm {
    pub fn deepseek(config: ProviderConfig, capacity: DispatchCapacity) -> Result<Self, LlmError> { Self::new(config, capacity, Provider::DeepSeek) }
    pub fn openai(config: ProviderConfig, capacity: DispatchCapacity) -> Result<Self, LlmError> { Self::new(config, capacity, Provider::OpenAi) }
    fn new(config: ProviderConfig, capacity: DispatchCapacity, provider: Provider) -> Result<Self, LlmError> {
        if config.api_key.is_empty() || config.api_key.chars().any(char::is_control) {
            return Err(error(ErrorKind::Configuration, RemoteOutcome::NotDispatched));
        }
        let mut endpoint = configured_url(&config.base_url, false)?;
        endpoint.path_segments_mut().map_err(|_| error(ErrorKind::Configuration, RemoteOutcome::NotDispatched))?
            .pop_if_empty().push("chat").push("completions");
        Ok(Self { client: http_client()?, endpoint, key: config.api_key, provider, capacity })
    }
}

struct Messages<'a>(&'a [Message]);
#[derive(Serialize)]
struct MessageRef<'a> { role: &'static str, content: &'a str }
impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for message in self.0 {
            let role = match message.role { Role::System => "system", Role::User => "user", Role::Assistant => "assistant" };
            seq.serialize_element(&MessageRef { role, content: &message.content })?;
        }
        seq.end()
    }
}
#[derive(Serialize)]
struct FormatRef { r#type: &'static str }
#[derive(Serialize)]
struct RequestRef<'a> {
    model: &'a str, messages: Messages<'a>, temperature: f64, max_tokens: u32, stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")] store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] ephemeral: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] response_format: Option<FormatRef>,
}
struct BoundedBody { bytes: Vec<u8>, limit: usize }
impl Write for BoundedBody {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > self.limit.saturating_sub(self.bytes.len()) { return Err(std::io::Error::other("completion input limit")); }
        self.bytes.extend_from_slice(buf); Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn request_body(request: &CompletionRequest, provider: Provider) -> Result<Vec<u8>, LlmError> {
    if request.model.is_empty() || request.model.len() > 256 || request.messages.is_empty() || request.timeout.is_zero()
        || !request.temperature.is_finite() || !(0.0..=2.0).contains(&request.temperature) || request.max_output_tokens == 0
        || request.limits.input_bytes == 0 || request.limits.output_bytes == 0 || request.limits.sse_bytes == 0 || request.limits.json_depth == 0 {
        return Err(error(ErrorKind::InvalidRequest, RemoteOutcome::NotDispatched));
    }
    let body = RequestRef { model: &request.model, messages: Messages(&request.messages), temperature: request.temperature,
        max_tokens: request.max_output_tokens, stream: true, store: matches!(provider, Provider::DeepSeek).then_some(false),
        ephemeral: matches!(provider, Provider::DeepSeek).then_some(true),
        response_format: request.response_format.map(|ResponseFormat::JsonObject| FormatRef { r#type: "json_object" }) };
    let mut out = BoundedBody { bytes: Vec::new(), limit: request.limits.input_bytes };
    serde_json::to_writer(&mut out, &body).map_err(|_| error(ErrorKind::Limit, RemoteOutcome::NotDispatched))?;
    Ok(out.bytes)
}
fn header_id(response: &Response) -> Option<String> {
    response.headers().get("x-request-id").and_then(|v| v.to_str().ok()).filter(|s| s.len() <= 256 && !s.chars().any(char::is_control)).map(str::to_owned)
}
impl Llm for HttpLlm {
    fn open(&self, request: CompletionRequest) -> BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>> {
        Box::pin(async move {
            let permit = self.capacity.acquire()?;
            let deadline = Instant::now().checked_add(request.timeout).ok_or_else(|| error(ErrorKind::InvalidRequest, RemoteOutcome::NotDispatched))?;
            let body = request_body(&request, self.provider)?;
            let send = self.client.post(self.endpoint.clone()).bearer_auth(&self.key)
                .header("Content-Type", "application/json").header("Accept", "text/event-stream").body(body).send();
            let response = timeout_at(deadline, send).await
                .map_err(|_| error(ErrorKind::Timeout, RemoteOutcome::Unknown))?
                .map_err(|_| error(ErrorKind::Transport, RemoteOutcome::Unknown))?;
            let remote_id = header_id(&response);
            if !response.status().is_success() {
                let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).filter(|s| s.len() <= 128).map(str::to_owned);
                return Err(LlmError { kind: ErrorKind::Http { status: response.status().as_u16(), retry_after }, remote: RemoteOutcome::Rejected, remote_id });
            }
            if !response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(';').next().is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))) {
                return Err(LlmError { kind: ErrorKind::Protocol, remote: RemoteOutcome::Unknown, remote_id });
            }
            let mut pending = VecDeque::new();
            if remote_id.is_some() { pending.push_back(CompletionEvent::Metadata { remote_id: remote_id.clone(), usage: None }); }
            Ok(Box::new(HttpCompletion { response: Some(response), permit: Some(permit), chunk: Bytes::new(), offset: 0,
                decoder: SseDecoder::new(request.limits.sse_bytes), pending, limits: request.limits, deadline,
                output_bytes: 0, remote_id, usage: None, reason: None, done: false, cancelled: false }) as Box<dyn CompletionStream>)
        })
    }
}

#[derive(Deserialize)]
struct Chunk { id: Option<String>, choices: Vec<Choice>, usage: Option<WireUsage>, error: Option<serde_json::Value> }
#[derive(Deserialize)]
struct Choice { index: Option<u32>, delta: Option<Delta>, finish_reason: Option<String> }
#[derive(Deserialize)]
struct Delta { content: Option<String>, role: Option<String>, tool_calls: Option<serde_json::Value>, function_call: Option<serde_json::Value>, refusal: Option<String>, reasoning_content: Option<String> }
#[derive(Deserialize)]
struct WireUsage { prompt_tokens: Option<u64>, completion_tokens: Option<u64>, total_tokens: Option<u64> }

struct HttpCompletion {
    response: Option<Response>, permit: Option<OwnedSemaphorePermit>, chunk: Bytes, offset: usize,
    decoder: SseDecoder, pending: VecDeque<CompletionEvent>, limits: Limits, deadline: Instant,
    output_bytes: usize, remote_id: Option<String>, usage: Option<Usage>, reason: Option<FinishReason>,
    done: bool, cancelled: bool,
}
impl HttpCompletion {
    fn failure(&mut self, kind: ErrorKind) -> LlmError {
        self.release(); self.done = true;
        LlmError { kind, remote: RemoteOutcome::Unknown, remote_id: self.remote_id.clone() }
    }
    fn release(&mut self) { self.response = None; self.permit = None; self.chunk = Bytes::new(); self.offset = 0; self.pending.clear(); }
    fn parse(&mut self, data: &str) -> Result<(), LlmError> {
        if data.trim() == "[DONE]" {
            let Some(reason) = self.reason.take() else { return Err(self.failure(ErrorKind::Protocol)); };
            let finish = CompletionFinish { reason, usage: self.usage.take(), remote_id: self.remote_id.clone() };
            self.release(); self.done = true; self.pending.push_back(CompletionEvent::Finished(finish)); return Ok(());
        }
        check_json(data.as_bytes(), self.limits.json_depth).map_err(|_| self.failure(ErrorKind::Protocol))?;
        let parsed: Chunk = serde_json::from_str(data).map_err(|_| self.failure(ErrorKind::Protocol))?;
        if parsed.error.is_some() || parsed.choices.len() > 1 { return Err(self.failure(ErrorKind::Protocol)); }
        let mut metadata = false;
        if let Some(id) = parsed.id {
            if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) { return Err(self.failure(ErrorKind::Protocol)); }
            if self.remote_id.as_ref() != Some(&id) { self.remote_id = Some(id); metadata = true; }
        }
        if let Some(usage) = parsed.usage {
            self.usage = Some(Usage { input_tokens: usage.prompt_tokens, output_tokens: usage.completion_tokens, total_tokens: usage.total_tokens }); metadata = true;
        }
        let mut delta = None;
        if let Some(choice) = parsed.choices.into_iter().next() {
            if choice.index.is_some_and(|i| i != 0) { return Err(self.failure(ErrorKind::Protocol)); }
            if choice.delta.is_none() && choice.finish_reason.is_none() { return Err(self.failure(ErrorKind::Protocol)); }
            if let Some(part) = choice.delta {
                if part.tool_calls.is_some() || part.function_call.is_some() || part.refusal.as_ref().is_some_and(|s| !s.is_empty())
                    || part.reasoning_content.as_ref().is_some_and(|s| !s.is_empty()) || part.role.as_ref().is_some_and(|s| s != "assistant") {
                    return Err(self.failure(ErrorKind::Protocol));
                }
                if let Some(content) = part.content.filter(|s| !s.is_empty()) {
                    if self.reason.is_some() { return Err(self.failure(ErrorKind::Protocol)); }
                    if content.len() > self.limits.output_bytes.saturating_sub(self.output_bytes) { return Err(self.failure(ErrorKind::Limit)); }
                    self.output_bytes += content.len(); delta = Some(content);
                }
            }
            if let Some(reason) = choice.finish_reason {
                let reason = match reason.as_str() { "stop" => FinishReason::Stop, "length" => FinishReason::Length,
                    "content_filter" => FinishReason::ContentFilter, "tool_calls" | "function_call" => FinishReason::ToolCalls,
                    _ if reason.len() <= 128 => FinishReason::Other(reason), _ => return Err(self.failure(ErrorKind::Protocol)) };
                if self.reason.as_ref().is_some_and(|old| *old != reason) { return Err(self.failure(ErrorKind::Protocol)); }
                self.reason = Some(reason);
            }
        }
        if metadata { self.pending.push_back(CompletionEvent::Metadata { remote_id: self.remote_id.clone(), usage: self.usage.clone() }); }
        if let Some(delta) = delta { self.pending.push_back(CompletionEvent::Delta(delta)); }
        Ok(())
    }
    async fn read_next(&mut self) -> Result<Option<CompletionEvent>, LlmError> {
        if self.cancelled { self.cancelled = false; return Err(self.failure(ErrorKind::Cancelled)); }
        loop {
            if let Some(event) = self.pending.pop_front() { return Ok(Some(event)); }
            if self.done { return Ok(None); }
            // Bytes already received are parsed even if the consumer resumed late; D4 bounds
            // network waiting, so the deadline gates only fetching more bytes.
            if self.offset < self.chunk.len() {
                let parsed = self.decoder.push(&self.chunk[self.offset..]);
                let (used, event) = parsed.map_err(|e| self.failure(if e == crate::CodecError::Limit { ErrorKind::Limit } else { ErrorKind::Protocol }))?;
                self.offset += used;
                if let Some(event) = event { self.parse(&event.data)?; }
                continue;
            }
            // Explicit pre-fetch check: timeout_at polls the read first, so a continuously
            // ready stream would otherwise never expire.
            if Instant::now() >= self.deadline { return Err(self.failure(ErrorKind::Timeout)); }
            let Some(response) = self.response.as_mut() else { return Err(self.failure(ErrorKind::Protocol)); };
            let read = timeout_at(self.deadline, response.chunk()).await;
            match read {
                Ok(Ok(Some(chunk))) => { self.chunk = chunk; self.offset = 0; }
                Ok(Ok(None)) => return Err(self.failure(ErrorKind::Protocol)),
                Ok(Err(_)) => return Err(self.failure(ErrorKind::Transport)),
                Err(_) => return Err(self.failure(ErrorKind::Timeout)),
            }
        }
    }
}
impl CompletionStream for HttpCompletion {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<CompletionEvent>, LlmError>> { Box::pin(self.read_next()) }
    fn cancel(&mut self) { if !self.done { self.release(); self.done = true; self.cancelled = true; } }
}

#[cfg(test)]
mod tests;
