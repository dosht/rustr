//! The actual summary driver over the real store with a scripted provider port: claim before
//! dispatch, quiet-open fences, provider identities, truthful interruption and replay order.
//! Requires TASKR_WORK_TEST_DATABASE_URL like the other summary PG tests.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use billing_core::erasure::open_account_erasure;
use billing_core::types::{AccountId, Micros as BillingMicros};
use billing_postgres::PgBillingStore;
use llm_core::{BoxFuture, CompletionEvent, CompletionFinish, CompletionRequest, CompletionStream, FinishReason, Llm, LlmError};
use media_ports::Micros;
use uuid::Uuid;

use crate::app::system_clock;
use crate::summary::app::SummaryService;
use crate::summary::domain::GenerationStatus;
use crate::summary::error::SummaryError;
use super::{Fixture, fixture};

type Event = Result<Option<CompletionEvent>, LlmError>;

enum Plan {
    /// The request was sent and the provider never answers headers.
    Quiet,
    Stream(Vec<Event>),
}

struct SetOnDrop(Arc<AtomicBool>);
impl Drop for SetOnDrop {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

/// Counts dispatches at `open` itself, so a recreated open future would count twice.
struct ScriptLlm {
    plans: Mutex<VecDeque<Plan>>,
    opens: AtomicUsize,
    open_dropped: Arc<AtomicBool>,
    timeouts: Mutex<Vec<Duration>>,
}

impl ScriptLlm {
    fn new(plans: Vec<Plan>) -> Arc<Self> {
        Arc::new(Self { plans: Mutex::new(plans.into()), opens: AtomicUsize::new(0), open_dropped: Arc::new(AtomicBool::new(false)), timeouts: Mutex::new(Vec::new()) })
    }
    fn opens(&self) -> usize { self.opens.load(Ordering::SeqCst) }
    fn open_dropped(&self) -> bool { self.open_dropped.load(Ordering::SeqCst) }
}

impl Llm for ScriptLlm {
    fn open(&self, request: CompletionRequest) -> BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.timeouts.lock().unwrap().push(request.timeout);
        let plan = self.plans.lock().unwrap().pop_front().unwrap_or(Plan::Quiet);
        let dropped = self.open_dropped.clone();
        Box::pin(async move {
            match plan {
                Plan::Quiet => {
                    let _dropped = SetOnDrop(dropped);
                    std::future::pending().await
                }
                Plan::Stream(events) => Ok(Box::new(ScriptStream(events.into())) as Box<dyn CompletionStream>),
            }
        })
    }
}

/// Yields its events, then stays quiet like an open connection.
struct ScriptStream(VecDeque<Event>);
impl CompletionStream for ScriptStream {
    fn next(&mut self) -> BoxFuture<'_, Event> {
        let next = self.0.pop_front();
        Box::pin(async move {
            match next {
                Some(event) => event,
                None => std::future::pending().await,
            }
        })
    }
    fn cancel(&mut self) { self.0.clear(); }
}

fn metadata(id: &str) -> Event { Ok(Some(CompletionEvent::Metadata { remote_id: Some(id.into()), usage: None })) }
fn delta(text: &str) -> Event { Ok(Some(CompletionEvent::Delta(text.into()))) }
fn stop(id: &str) -> Event { Ok(Some(CompletionEvent::Finished(CompletionFinish { reason: FinishReason::Stop, usage: None, remote_id: Some(id.into()) }))) }

fn service(f: &Fixture, llm: Option<Arc<ScriptLlm>>) -> Arc<SummaryService> {
    Arc::new(SummaryService::new(f.store.clone(), llm.map(|l| l as Arc<dyn Llm>), "deepseek".into(), "fixture-model".into(), system_clock()))
}

fn now_us() -> i64 { (system_clock())().0 }

macro_rules! eventually {
    ($condition:expr, $why:expr) => {{
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if $condition { break; }
            assert!(Instant::now() < deadline, $why);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }};
}

async fn status(service: &SummaryService, f: &Fixture, id: Uuid) -> GenerationStatus {
    service.generation(f.account, f.work, id).await.unwrap().status
}

async fn erase(f: &Fixture) {
    f.store.open_barrier(f.account, Micros(now_us())).await.unwrap();
    open_account_erasure(&PgBillingStore::new(f.store.pool().clone()), AccountId(f.account), BillingMicros(now_us())).await.unwrap();
    f.store.erase_data(f.account, Micros(now_us()), Micros(now_us() + 900_000_000)).await.unwrap();
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_quiet_open_observes_another_replicas_cancel_and_erasure_without_a_second_dispatch() {
    let f = fixture().await;
    let llm = ScriptLlm::new(vec![Plan::Quiet]);
    let replica_a = service(&f, Some(llm.clone()));
    // Another replica: same database, its own local signals and no provider at all.
    let replica_b = service(&f, None);
    let (g, created) = replica_a.start(f.account, f.work, Uuid::new_v4(), true).await.unwrap();
    assert!(created);
    eventually!(llm.opens() == 1, "the claimed generation dispatches exactly one request");
    eventually!(status(&replica_a, &f, g.generation_id).await == GenerationStatus::Running, "claimed before the provider answers");
    assert_eq!(replica_b.cancel(f.account, f.work, g.generation_id).await.unwrap().status, GenerationStatus::Cancelled);
    eventually!(llm.open_dropped(), "a quiet open sees a cancel committed by another replica");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(llm.opens(), 1, "observing fences never recreates the pending open");
    assert_eq!(replica_a.generation(f.account, f.work, g.generation_id).await.unwrap().text, "");

    let erased = fixture().await;
    let llm = ScriptLlm::new(vec![Plan::Quiet]);
    let replica = service(&erased, Some(llm.clone()));
    replica.start(erased.account, erased.work, Uuid::new_v4(), true).await.unwrap();
    eventually!(llm.opens() == 1, "dispatched");
    erased.store.open_barrier(erased.account, Micros(now_us())).await.unwrap();
    eventually!(llm.open_dropped(), "a quiet open sees the erasure barrier");
    assert_eq!(llm.opens(), 1);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn distinct_header_and_body_provider_ids_survive_completion_replay_and_erasure() {
    let f = fixture().await;
    let llm = ScriptLlm::new(vec![Plan::Stream(vec![
        metadata("hdr-1"), delta("Hello "), metadata("body-1"), delta("world"), stop("body-1"),
    ])]);
    let summaries = service(&f, Some(llm.clone()));
    let request = Uuid::new_v4();
    let (g, _) = summaries.start(f.account, f.work, request, true).await.unwrap();
    eventually!(status(&summaries, &f, g.generation_id).await == GenerationStatus::Completed, "the scripted completion settles");
    assert_eq!(summaries.generation(f.account, f.work, g.generation_id).await.unwrap().text, "Hello world");
    assert_eq!(llm.timeouts.lock().unwrap().as_slice(), [Duration::from_secs(60)], "one 60 s provider deadline");
    let ids: Option<serde_json::Value> = sqlx::query_scalar("SELECT payload->'remote_ids' FROM taskr_work.summary_generations WHERE id = $1")
        .bind(g.generation_id).fetch_one(f.store.pool()).await.unwrap();
    assert_eq!(ids, Some(serde_json::json!(["hdr-1", "body-1"])), "the header id is not overwritten by the body id");

    let (replayed, created) = summaries.start(f.account, f.work, request, true).await.unwrap();
    assert!(!created && replayed.generation_id == g.generation_id);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(llm.opens(), 1, "a replay never dispatches again");

    erase(&f).await;
    let resources = f.store.erasure_resources(f.account).await.unwrap();
    let external = resources.iter().find(|r| r.kind == "llm").expect("dispatched work stays explicit after the cascade");
    assert_eq!(external.target["remote_ids"], serde_json::json!(["hdr-1", "body-1"]));
    assert!(external.target.get("text").is_none());
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_oversized_provider_identity_interrupts_with_its_own_reason_not_storage_advice() {
    let f = fixture().await;
    let llm = ScriptLlm::new(vec![Plan::Stream(vec![metadata(&"x".repeat(2000))])]);
    let summaries = service(&f, Some(llm.clone()));
    let (g, _) = summaries.start(f.account, f.work, Uuid::new_v4(), true).await.unwrap();
    eventually!(status(&summaries, &f, g.generation_id).await == GenerationStatus::Interrupted, "the limit interrupts");
    let error = summaries.generation(f.account, f.work, g.generation_id).await.unwrap().error.unwrap();
    assert!(error.contains("identity") && error.contains("limit"), "{error}");
    assert!(!error.contains("Reconnect"), "nothing is left to reconcile: {error}");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_existing_request_replays_before_disclosure_or_configuration_and_ownership_comes_first() {
    let f = fixture().await;
    let request = Uuid::new_v4();
    let admitted = f.store.admit_summary(f.account, f.work, request, now_us()).await.unwrap().generation;
    let unconfigured = service(&f, None);
    let (replayed, created) = unconfigured.start(f.account, f.work, request, true).await.unwrap();
    assert!(!created && replayed.generation_id == admitted.id, "reading back a request does not need a provider");
    let (again, _) = unconfigured.start(f.account, f.work, request, false).await.unwrap();
    assert_eq!(again.generation_id, admitted.id, "the disclosure gates new work, not a replay");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(status(&unconfigured, &f, admitted.id).await, GenerationStatus::Accepted, "a host without a provider never claims");
    assert!(matches!(unconfigured.start(f.account, f.work, Uuid::new_v4(), false).await, Err(SummaryError::Invalid(_))));
    assert!(matches!(unconfigured.start(f.account, f.work, Uuid::new_v4(), true).await, Err(SummaryError::NotConfigured)));
    let stranger = fixture().await;
    assert!(matches!(unconfigured.start(stranger.account, f.work, request, false).await, Err(SummaryError::NotFound)), "ownership precedes the disclosure check");
}
