use std::time::Duration;
use super::*;

fn delta(text: &str) -> String {
    format!("data: {}\n\n", serde_json::json!({"id":"completion-456","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}))
}
fn finish(reason: &str) -> String {
    format!("data: {}\n\ndata: [DONE]\n\n", serde_json::json!({"id":"completion-456","choices":[{"index":0,"delta":{},"finish_reason":reason}]}))
}
/// A completion whose provider bytes were already received as ONE chunk. No socket remains,
/// so any attempt to read the network again surfaces as an error instead of data.
fn received(tail: String, deadline: Instant) -> HttpCompletion {
    let limits = Limits::default();
    HttpCompletion { response: None, permit: None, chunk: Bytes::from(tail), offset: 0,
        decoder: SseDecoder::new(limits.sse_bytes), pending: VecDeque::new(), limits, deadline,
        output_bytes: 0, remote_id: None, usage: None, reason: None, done: false, cancelled: false }
}

#[tokio::test(start_paused = true)]
async fn buffered_terminal_tail_is_parsed_after_a_late_consumer_resumes() {
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut stream = received(delta("first ") + &delta("second") + &finish("stop"), deadline);
    let mut text = String::new();
    while text.is_empty() {
        if let Some(CompletionEvent::Delta(s)) = stream.read_next().await.unwrap() { text.push_str(&s); }
    }
    // The consumer spends longer than the whole deadline before asking again.
    tokio::time::advance(Duration::from_millis(500)).await;
    let mut finished = None;
    loop {
        match stream.read_next().await {
            Ok(Some(CompletionEvent::Delta(s))) => text.push_str(&s),
            Ok(Some(CompletionEvent::Finished(f))) => finished = Some(f),
            Ok(Some(CompletionEvent::Metadata { .. })) => {}
            Ok(None) => break,
            Err(error) => panic!("already received terminal evidence must not become {error:?}"),
        }
    }
    assert_eq!(text, "first second");
    assert!(finished.unwrap().reason.is_success());
}

#[tokio::test(start_paused = true)]
async fn an_incomplete_buffered_tail_still_times_out_instead_of_reading_again() {
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut stream = received(delta("first ") + &delta("second"), deadline);
    tokio::time::advance(Duration::from_millis(500)).await;
    let mut text = String::new();
    let error = loop {
        match stream.read_next().await {
            Ok(Some(CompletionEvent::Delta(s))) => text.push_str(&s),
            Ok(Some(_)) => {}
            Ok(None) => panic!("an unterminated stream is not a completion"),
            Err(error) => break error,
        }
    };
    // Buffered deltas are still delivered; fetching new bytes after expiry is a timeout.
    assert_eq!(text, "first second");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_eq!(error.remote, RemoteOutcome::Unknown);
}
