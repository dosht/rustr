#![cfg(not(target_arch = "wasm32"))]
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use axum::{Router, body::{Body, Bytes}, extract::State, http::{HeaderMap, StatusCode, Uri}, response::Response, routing::post};
use llm_core::*;
use llm_http::*;
use serde_json::{Value, json};
use tokio::sync::mpsc;

struct Reply { status: StatusCode, body: Body, retry: bool }
impl Reply { fn sse(body: impl Into<String>) -> Self { Self { status: StatusCode::OK, body: Body::from(body.into()), retry: false } } }
#[derive(Clone)]
struct ServerState { replies: Arc<Mutex<VecDeque<Reply>>>, requests: Arc<Mutex<Vec<(String, Value, String)>>> }
struct Server { base: String, state: ServerState, job: tokio::task::JoinHandle<()> }
impl Drop for Server { fn drop(&mut self) { self.job.abort(); } }
async fn handler(State(state): State<ServerState>, uri: Uri, headers: HeaderMap, bytes: Bytes) -> Response {
    state.requests.lock().unwrap().push((uri.path().to_owned(), serde_json::from_slice(&bytes).unwrap(), headers.get("authorization").unwrap().to_str().unwrap().to_owned()));
    let reply = state.replies.lock().unwrap().pop_front().expect("unexpected provider replay");
    let mut out = Response::builder().status(reply.status).header("Content-Type", "text/event-stream").header("x-request-id", "request-123");
    if reply.retry { out = out.header("retry-after", "3"); }
    out.body(reply.body).unwrap()
}
async fn server(replies: Vec<Reply>) -> Server {
    let state = ServerState { replies: Arc::new(Mutex::new(replies.into())), requests: Arc::new(Mutex::new(Vec::new())) };
    let app = Router::new().route("/chat/completions", post(handler)).route("/v1/chat/completions", post(handler)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let job = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    Server { base, state, job }
}
fn request() -> CompletionRequest { CompletionRequest::new("deepseek-chat", vec![Message { role: Role::User, content: "Please discuss json in this transcript ş😀".into() }]) }
fn delta(text: &str) -> String { format!("data: {}\n\n", json!({"id":"completion-456","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})) }
fn finish(reason: &str) -> String { format!("data: {}\n\ndata: [DONE]\n\n", json!({"id":"completion-456","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}})) }
async fn collect(stream: &mut dyn CompletionStream) -> (String, Option<CompletionFinish>, Option<LlmError>) {
    let mut content = String::new(); let mut finish = None;
    loop {
        match stream.next().await {
            Ok(Some(CompletionEvent::Delta(s))) => content.push_str(&s),
            Ok(Some(CompletionEvent::Finished(f))) => finish = Some(f),
            Ok(Some(CompletionEvent::Metadata { .. })) => {}
            Ok(None) => return (content, finish, None),
            Err(e) => return (content, finish, Some(e)),
        }
    }
}

#[tokio::test]
async fn provider_response_mode_is_explicit_and_privacy_flags_are_provider_specific() {
    let provider = server(vec![Reply::sse(delta("first") + &finish("stop")), Reply::sse(delta("second") + &finish("stop"))]).await;
    let capacity = DispatchCapacity::new(8).unwrap();
    let deepseek = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), capacity.clone()).unwrap();
    let mut input = request(); input.response_format = Some(ResponseFormat::JsonObject);
    let mut stream = deepseek.open(input).await.unwrap();
    let (text, final_result, error) = collect(stream.as_mut()).await;
    assert_eq!(text, "first"); assert!(error.is_none()); assert!(final_result.unwrap().reason.is_success());
    let openai = HttpLlm::openai(ProviderConfig::new(format!("{}/v1", provider.base), "e4-key"), capacity).unwrap();
    let mut stream = openai.open(request()).await.unwrap();
    assert_eq!(collect(stream.as_mut()).await.0, "second");
    let requests = provider.state.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "/chat/completions");
    assert_eq!(requests[0].1["response_format"], json!({"type":"json_object"}));
    assert_eq!(requests[0].1["store"], false); assert_eq!(requests[0].1["ephemeral"], true);
    assert_eq!(requests[0].1["model"], "deepseek-chat");
    assert_eq!(requests[1].0, "/v1/chat/completions");
    assert!(requests[1].1.get("response_format").is_none(), "word json in user content must not enable structured output");
    assert!(requests[1].1.get("store").is_none()); assert!(requests[1].1.get("ephemeral").is_none());
}

#[tokio::test]
async fn partial_is_observed_before_terminal_capacity_refuses_without_queue_and_cancel_releases() {
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    let body = Body::from_stream(futures_util::stream::unfold(receiver, |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) }));
    let provider = server(vec![Reply { status: StatusCode::OK, body, retry: false }, Reply::sse(delta("next") + &finish("stop"))]).await;
    sender.send(Ok(Bytes::from(delta("early ")))).await.unwrap();
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(1).unwrap()).unwrap();
    let mut first = adapter.open(request()).await.unwrap();
    loop { if let Some(CompletionEvent::Delta(text)) = first.next().await.unwrap() { assert_eq!(text, "early "); break; } }
    let denied = match adapter.open(request()).await { Err(e) => e, Ok(_) => panic!("capacity must not wait or dispatch") };
    assert_eq!(denied.kind, ErrorKind::Capacity); assert_eq!(denied.remote, RemoteOutcome::NotDispatched);
    assert_eq!(provider.state.requests.lock().unwrap().len(), 1);
    first.cancel();
    assert_eq!(first.next().await.unwrap_err().kind, ErrorKind::Cancelled);
    assert!(first.next().await.unwrap().is_none());
    let mut next = adapter.open(request()).await.unwrap();
    assert_eq!(collect(next.as_mut()).await.0, "next");
    assert_eq!(provider.state.requests.lock().unwrap().len(), 2);
    drop(sender);
}

#[tokio::test]
async fn incomplete_or_malformed_upstream_preserves_delivered_partial_and_never_retries() {
    for tail in ["", "data: {broken}\n\n", "data: [DONE]\n\n", "data: {\"unfinished\":true}\n", "data: {\"unexpected\":\"shape\"}\n\n", "data: {\"choices\":[{}]}\n\n"] {
        let provider = server(vec![Reply::sse(delta("partial") + tail)]).await;
        let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
        let mut stream = adapter.open(request()).await.unwrap();
        let (text, finish, error) = collect(stream.as_mut()).await;
        assert_eq!(text, "partial"); assert!(finish.is_none());
        let error = error.unwrap(); assert_eq!(error.kind, ErrorKind::Protocol); assert_eq!(error.remote, RemoteOutcome::Unknown);
        assert_eq!(provider.state.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn length_filter_and_tool_finish_are_observed_but_never_success() {
    for reason in ["length", "content_filter", "tool_calls"] {
        let provider = server(vec![Reply::sse(delta("limited") + &finish(reason))]).await;
        let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
        let mut stream = adapter.open(request()).await.unwrap();
        let (text, finish, error) = collect(stream.as_mut()).await;
        assert_eq!(text, "limited"); assert!(error.is_none()); assert!(!finish.unwrap().reason.is_success());
    }
}

#[tokio::test]
async fn crlf_multiline_data_keepalive_and_usage_are_decoded_over_real_http() {
    let wire = ": keepalive\r\nid: a\r\nevent: message\r\ndata: {\"id\":\"unicode\",\"choices\":[{\"index\":0,\r\ndata: \"delta\":{\"content\":\"ş😀\"},\"finish_reason\":null}]}\r\n\r\n".to_owned() + &finish("stop");
    let provider = server(vec![Reply::sse(wire)]).await;
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
    let mut stream = adapter.open(request()).await.unwrap();
    let (text, finish, error) = collect(stream.as_mut()).await;
    assert_eq!(text, "ş😀"); assert!(error.is_none());
    let usage = finish.unwrap().usage.unwrap(); assert_eq!(usage.output_tokens, Some(4));
}

#[tokio::test]
async fn http_rejection_and_retry_after_are_typed_without_automatic_retry() {
    let provider = server(vec![Reply { status: StatusCode::TOO_MANY_REQUESTS, body: Body::from("secret provider detail"), retry: true }]).await;
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
    let error = match adapter.open(request()).await { Err(error) => error, Ok(_) => panic!("expected HTTP refusal") };
    assert_eq!(error.kind, ErrorKind::Http { status: 429, retry_after: Some("3".into()) });
    assert_eq!(error.remote, RemoteOutcome::Rejected);
    assert!(!error.to_string().contains("secret provider detail"));
    assert_eq!(provider.state.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn deadline_output_record_and_depth_limits_stop_local_consumption() {
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    sender.send(Ok(Bytes::from_static(b": heartbeat\n\n"))).await.unwrap();
    let body = Body::from_stream(futures_util::stream::unfold(receiver, |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) }));
    let provider = server(vec![Reply { status: StatusCode::OK, body, retry: false }]).await;
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
    let mut input = request(); input.timeout = Duration::from_millis(200);
    let mut stream = adapter.open(input).await.unwrap();
    let error = collect(stream.as_mut()).await.2.unwrap();
    assert_eq!(error.kind, ErrorKind::Timeout); assert_eq!(error.remote, RemoteOutcome::Unknown); drop(sender);
    for (limits, wire, expected) in [
        (Limits { output_bytes: 4, ..Limits::default() }, delta("12345") + &finish("stop"), ErrorKind::Limit),
        (Limits { sse_bytes: 8, ..Limits::default() }, delta("x"), ErrorKind::Limit),
        (Limits { json_depth: 2, ..Limits::default() }, delta("x"), ErrorKind::Protocol),
    ] {
        let provider = server(vec![Reply::sse(wire)]).await;
        let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
        let mut input = request(); input.limits = limits;
        let mut stream = adapter.open(input).await.unwrap();
        let (_, finish, error) = collect(stream.as_mut()).await;
        assert!(finish.is_none()); assert_eq!(error.unwrap().kind, expected);
    }
}

#[tokio::test]
async fn oversized_input_is_refused_before_provider_dispatch() {
    let provider = server(vec![]).await;
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
    let mut input = request(); input.limits.input_bytes = 8;
    let error = match adapter.open(input).await { Err(error) => error, Ok(_) => panic!("expected input bound") };
    assert_eq!(error.kind, ErrorKind::Limit); assert_eq!(error.remote, RemoteOutcome::NotDispatched);
    assert!(provider.state.requests.lock().unwrap().is_empty());
}

// The buffered-tail deadline boundary lives in `src/provider/tests.rs` with a controlled
// single chunk and paused time; it must not depend on how loopback TCP splits a body.

#[tokio::test]
async fn continuous_stream_still_times_out_and_reads_nothing_after_expiry() {
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let feeder = tokio::spawn(async move {
        while sender.send(Ok(Bytes::from(delta("x")))).await.is_ok() { tokio::time::sleep(Duration::from_millis(20)).await; }
    });
    let body = Body::from_stream(futures_util::stream::unfold(receiver, |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) }));
    let provider = server(vec![Reply { status: StatusCode::OK, body, retry: false }]).await;
    let adapter = HttpLlm::deepseek(ProviderConfig::new(&provider.base, "e4-key"), DispatchCapacity::new(8).unwrap()).unwrap();
    let mut input = request(); input.timeout = Duration::from_millis(250);
    let mut stream = adapter.open(input).await.unwrap();
    let (_, finish, error) = collect(stream.as_mut()).await;
    assert!(finish.is_none());
    let error = error.unwrap(); assert_eq!(error.kind, ErrorKind::Timeout); assert_eq!(error.remote, RemoteOutcome::Unknown);
    assert!(stream.next().await.unwrap().is_none(), "no new reads after expiry");
    assert_eq!(provider.state.requests.lock().unwrap().len(), 1, "no retry");
    feeder.abort();
}
