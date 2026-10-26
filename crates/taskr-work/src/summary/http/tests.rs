//! The actual watch route over the real store: attachment admission, unlocked fence polling with
//! fresh session/barrier checks, and reconciliation of real staged work only.
//! Requires TASKR_WORK_TEST_DATABASE_URL like the other summary PG tests.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::BodyDataStream;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use billing_core::types::{AccountId, BoxFuture};
use billing_http::{AuthError, BillingIdentity, Caller};
use futures_util::StreamExt;
use llm_core::{ApplicationFrame, Limits, Persistence, StreamStatus, TextEvent};
use llm_http::ApplicationDecoder;
use media_ports::Micros;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::app::system_clock;
use crate::summary::app::SummaryService;
use crate::summary::domain::Generation;
use crate::summary::pg_tests::{Fixture, fixture};
use super::{SummaryHttp, WatchQuery, watch};

/// Bearer token = the account UUID; a revoked token stops verifying, like an ended session. Every
/// authentication is counted and announced, so a test can tell how many polls actually ran.
#[derive(Default)]
struct Tokens { revoked: Mutex<HashSet<String>>, calls: AtomicUsize, called: Notify }

impl Tokens {
    fn revoke(&self, account: Uuid) { self.revoked.lock().unwrap().insert(account.to_string()); }
    fn calls(&self) -> usize { self.calls.load(Ordering::SeqCst) }
    /// Waits for at least `target` authentications; the timeout only bounds a failure.
    async fn until_calls(&self, target: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                // Created before the check: `notify_waiters` reaches it even before it is polled.
                let called = self.called.notified();
                if self.calls() >= target { return; }
                called.await;
            }
        }).await.expect("authenticated polls kept running");
    }
}

impl BillingIdentity for Tokens {
    fn authenticate<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Caller, AuthError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.called.notify_waiters();
            let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
                .ok_or_else(|| AuthError::Unauthenticated("no bearer token".into()))?;
            if self.revoked.lock().unwrap().contains(token) { return Err(AuthError::Unauthenticated("session ended".into())); }
            let account = Uuid::parse_str(token).map_err(|_| AuthError::Unauthenticated("unknown token".into()))?;
            Ok(Caller { account: AccountId(account), email: "owner@example.test".into(), full_name: None, is_admin: false })
        })
    }
}

fn now_us() -> i64 { (system_clock())().0 }

/// A host without a provider: the watch route never dispatches, so none is needed.
fn route(f: &Fixture) -> (Arc<SummaryHttp>, Arc<Tokens>) {
    let tokens = Arc::new(Tokens::default());
    let service = Arc::new(SummaryService::new(f.store.clone(), None, String::new(), String::new(), system_clock()));
    (Arc::new(SummaryHttp { service, identity: tokens.clone() }), tokens)
}

fn bearer(account: Uuid) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {account}")).unwrap());
    headers
}

async fn attach(state: &Arc<SummaryHttp>, f: &Fixture, id: Uuid) -> Response {
    watch(State(state.clone()), bearer(f.account), Path((f.work, id)), Query(WatchQuery { after: None })).await
}

/// A claimed generation whose provider is quiet: nothing changes while it is watched.
async fn running(f: &Fixture) -> Generation {
    let now = now_us();
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), now).await.unwrap().generation.id;
    f.store.claim_summary(f.account, f.work, id, now + 1, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap().0
}

struct Frames { body: BodyDataStream, decoder: ApplicationDecoder, pending: Vec<u8> }

impl Frames {
    fn new(response: Response) -> Self {
        assert_eq!(response.status(), StatusCode::OK);
        Self { body: response.into_body().into_data_stream(), decoder: ApplicationDecoder::new(Limits::default()), pending: Vec::new() }
    }
    async fn next(&mut self) -> ApplicationFrame {
        loop {
            while !self.pending.is_empty() {
                let (used, frame) = self.decoder.push(&self.pending).unwrap();
                self.pending.drain(..used);
                if let Some(frame) = frame { return frame; }
                if used == 0 { break; }
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.body.next()).await
                .expect("a frame within five seconds").expect("the attachment is still open").unwrap();
            self.pending.extend_from_slice(&chunk);
        }
    }
}

async fn refusal(response: Response) -> serde_json::Value {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn attachments_are_refused_immediately_past_four_per_account_and_thirty_two_per_process() {
    let f = fixture().await;
    let id = running(&f).await.id;
    let (state, _) = route(&f);
    let mut held = Vec::new();
    for _ in 0..4 {
        let response = attach(&state, &f, id).await;
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    let refused = refusal(attach(&state, &f, id).await).await;
    assert_eq!(refused["error"]["code"], "watch_limit", "an explicit refusal, not a queued attachment");
    drop(held.pop());
    let reopened = attach(&state, &f, id).await;
    assert_eq!(reopened.status(), StatusCode::OK, "dropping an attachment's body frees its slot");
    held.push(reopened);

    // Seven more owners with four attachments each fill the process-wide 32.
    for _ in 0..7 {
        let other = fixture().await;
        let other_id = running(&other).await.id;
        for _ in 0..4 {
            let response = attach(&state, &other, other_id).await;
            assert_eq!(response.status(), StatusCode::OK);
            held.push(response);
        }
    }
    let last = fixture().await;
    let last_id = running(&last).await.id;
    assert_eq!(refusal(attach(&state, &last, last_id).await).await["error"]["code"], "watch_limit");
    drop(held);
    assert_eq!(attach(&state, &last, last_id).await.status(), StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn unchanged_authorized_polls_traverse_the_database_under_held_account_and_work_locks_then_see_session_loss() {
    let f = fixture().await;
    let id = running(&f).await.id;
    let (state, tokens) = route(&f);
    let mut frames = Frames::new(attach(&state, &f, id).await);
    assert!(matches!(frames.next().await.event, TextEvent::Snapshot { status: StreamStatus::Streaming, .. }));

    // A long account-locked operation on the same work (an edit, a callback, erasure's barrier).
    let mut lock = f.store.begin().await.unwrap();
    lock.lock_account(f.account).await.unwrap();
    lock.load_work(f.work).await.unwrap().unwrap();
    // The attachment polls only while its body is being read, so keep reading under the locks.
    let reader = tokio::spawn(async move { frames.next().await });
    // A poll authenticates, then reads the database, then sleeps. Authentication `before + 3`
    // starting proves polls `before + 1` and `before + 2` both finished their database read while
    // still authorized and unchanged, behind the held account and work locks.
    let before = tokens.calls();
    tokens.until_calls(before + 3).await;
    assert!(!reader.is_finished(), "an unchanged generation yields no frame");
    tokens.revoke(f.account);
    let revoked = tokio::time::timeout(Duration::from_secs(5), reader).await.expect("the next poll sees the ended session").unwrap();
    assert_eq!(revoked.event, TextEvent::Revoked);
    drop(lock);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_watch_reconciles_a_real_staged_final_and_stops_at_a_fresh_barrier() {
    let f = fixture().await;
    let claimed = running(&f).await;
    let id = claimed.id;
    f.store.checkpoint_summary(&claimed, "Hello".into(), false, now_us()).await.unwrap();
    f.store.checkpoint_summary(&claimed, "Hello world".into(), true, now_us()).await.unwrap();
    let (state, _) = route(&f);
    let mut frames = Frames::new(attach(&state, &f, id).await);
    assert_eq!(frames.next().await.event, TextEvent::Final { text: "Hello world".into(), persistence: Persistence::Saved },
        "a durable staged final completes locally on its first read");
    assert_eq!(f.store.read_summary(f.account, f.work, now_us()).await.unwrap().summary.map(|s| s.text), Some("Hello world".to_string()));

    let erased = fixture().await;
    let id = running(&erased).await.id;
    let (state, _) = route(&erased);
    let mut frames = Frames::new(attach(&state, &erased, id).await);
    frames.next().await;
    erased.store.open_barrier(erased.account, Micros(now_us())).await.unwrap();
    assert_eq!(frames.next().await.event, TextEvent::Revoked, "the barrier is read fresh on every poll");
}
