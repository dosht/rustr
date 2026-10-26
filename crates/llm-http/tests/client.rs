#![cfg(not(target_arch = "wasm32"))]
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use axum::{Router, body::Body, extract::State, http::{HeaderMap, Uri}, response::Response, routing::get};
use llm_core::*;
use llm_http::*;

#[derive(Clone)]
struct Fixture { bodies: Arc<Mutex<VecDeque<String>>>, queries: Arc<Mutex<Vec<String>>> }
async fn watch(State(fixture): State<Fixture>, headers: HeaderMap, uri: Uri) -> Response {
    assert_eq!(headers.get("authorization").unwrap(), "Bearer session-not-provider-key");
    fixture.queries.lock().unwrap().push(uri.query().unwrap_or("").to_owned());
    let body = fixture.bodies.lock().unwrap().pop_front().expect("unexpected attachment");
    Response::builder().header("content-type", "text/event-stream").body(Body::from(body)).unwrap()
}
fn frame(seq: u64, event: TextEvent) -> ApplicationFrame {
    ApplicationFrame { identity: StreamIdentity { generation: "generation".into(), content_revision: 1, consent_epoch: 0, execution: "execution".into() }, seq, event }
}
fn snapshot(seq: u64, text: &str) -> ApplicationFrame { frame(seq, TextEvent::Snapshot { text: text.into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }) }
fn wire(frame: ApplicationFrame) -> String { encode_frame(&frame, Limits::default()).unwrap() }
async fn serve(bodies: Vec<String>) -> (String, Fixture, tokio::task::JoinHandle<()>) {
    let fixture = Fixture { bodies: Arc::new(Mutex::new(bodies.into())), queries: Arc::new(Mutex::new(Vec::new())) };
    let router = Router::new().route("/watch", get(watch)).with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/watch", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    (url, fixture, task)
}

#[tokio::test]
async fn native_reattach_is_snapshot_replacement_and_never_a_start_request() {
    let final_frame = frame(3, TextEvent::Final { text: "Hello world".into(), persistence: Persistence::Saved });
    let (url, fixture, task) = serve(vec![wire(snapshot(1, "Hello")), wire(snapshot(2, "Hello world")) + &wire(final_frame)]).await;
    let client = ApplicationClient::new(Limits::default(), Duration::from_secs(2)).unwrap();
    let mut state = StreamState::default(); state.start(frame(0, TextEvent::Revoked).identity);
    state.update(frame(1, TextEvent::Delta("Hello ".into()))).unwrap();
    let mut first = client.open(&format!("{url}?after=0"), Some("session-not-provider-key")).await.unwrap();
    state.resume(); state.update(first.next().await.unwrap().unwrap()).unwrap();
    assert_eq!(state.view().text, "Hello");
    first.cancel(); assert_eq!(first.next().await.unwrap_err(), ClientError::Cancelled);
    state.resume();
    let mut second = client.open(&format!("{url}?after=1"), Some("session-not-provider-key")).await.unwrap();
    while let Some(frame) = second.next().await.unwrap() { state.update(frame).unwrap(); }
    assert_eq!(state.view().text, "Hello world"); assert_eq!(state.view().status, StreamStatus::Completed);
    assert_eq!(*fixture.queries.lock().unwrap(), ["after=0", "after=1"]);
    task.abort();
}

#[tokio::test]
async fn host_eof_without_terminal_is_interruption_even_after_valid_snapshot() {
    let (url, _, task) = serve(vec![wire(snapshot(1, "partial"))]).await;
    let client = ApplicationClient::new(Limits::default(), Duration::from_secs(2)).unwrap();
    let mut stream = client.open(&url, Some("session-not-provider-key")).await.unwrap();
    assert_eq!(stream.next().await.unwrap(), Some(snapshot(1, "partial")));
    assert_eq!(stream.next().await.unwrap_err(), ClientError::Incomplete);
    assert!(stream.next().await.unwrap().is_none());
    task.abort();
}

#[tokio::test]
async fn watch_credentials_cannot_be_put_in_query_or_follow_redirects() {
    let (url, fixture, task) = serve(vec![]).await;
    let client = ApplicationClient::new(Limits::default(), Duration::from_secs(2)).unwrap();
    let result = client.open(&format!("{url}?access_token=private"), None).await;
    assert!(matches!(result, Err(ClientError::Configuration)));
    assert!(fixture.queries.lock().unwrap().is_empty());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_url = format!("http://{}/redirect", listener.local_addr().unwrap());
    let redirect = Router::new().route("/redirect", get(move || async move { axum::response::Redirect::temporary(&url) }));
    let redirect_task = tokio::spawn(async move { axum::serve(listener, redirect).await.unwrap(); });
    assert!(matches!(client.open(&redirect_url, Some("session-not-provider-key")).await, Err(ClientError::Http(307))));
    assert!(fixture.queries.lock().unwrap().is_empty());
    redirect_task.abort(); task.abort();
}
