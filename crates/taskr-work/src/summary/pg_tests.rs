//! Real-PG lifecycle invariants, on the billing-runtime authority and existing account lock.
//! Requires TASKR_WORK_TEST_DATABASE_URL; never skips when explicitly included without it.

use billing_core::erasure::open_account_erasure;
use billing_core::types::{AccountId, Micros as BillingMicros};
use billing_postgres::PgBillingStore;
use media_ports::{Micros, ObjectRef, StoreKind};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::store::{self, Store};
use crate::work::{Status, Work};
use super::domain::{Generation, GenerationStatus};
use super::error::SummaryError;

mod driver;

const LOGIN: &str = "taskr_summary_test_billing_runtime";
const PASSWORD: &str = "taskr-summary-test-runtime-password";
pub(super) const NOW: i64 = 1_792_886_400_000_000;
const LIMIT: usize = 16 * 1024 * 1024;
static READY: OnceCell<()> = OnceCell::const_new();

async fn pool() -> PgPool {
    let url = std::env::var("TASKR_WORK_TEST_DATABASE_URL").expect("TASKR_WORK_TEST_DATABASE_URL must name an owned disposable database");
    let owner = billing_postgres::connect_owner(&url).await.unwrap();
    READY.get_or_init(|| async {
        billing_postgres::migrate(&owner).await.unwrap();
        taskr_billing::migrate_host(&owner).await.unwrap();
        store::migrate(&owner).await.unwrap();
        billing_postgres::provision_runtime_login(&owner, LOGIN, PASSWORD).await.unwrap();
        store::grant_runtime(&owner, "billing_runtime").await.unwrap();
    }).await;
    let options = billing_postgres::options_for_login(&url, LOGIN, PASSWORD).unwrap();
    billing_postgres::connect_runtime(options, 5).await.unwrap()
}

pub(super) struct Fixture { pub store: Store, pub account: Uuid, pub work: Uuid }

pub(super) async fn fixture() -> Fixture {
    let store = Store::new(pool().await);
    let account = Uuid::new_v4();
    let work = Uuid::new_v4();
    let mut tx = store.begin().await.unwrap();
    tx.enroll(account, "owner@example.test", Micros(NOW)).await.unwrap();
    let mut source = Work::create(work, account, ObjectRef { store: StoreKind::Gcs, bucket: "test".into(), key: format!("{account}/{work}"), version: Some("v1".into()) }, "meeting".into(), None, None, Micros(NOW));
    source.status = Status::Completed;
    source.transcript = Some("The team decided to ship on Friday.".into());
    source.completed_at = Some(Micros(NOW));
    source.revision = 1;
    tx.insert_work(&source).await.unwrap();
    tx.commit().await.unwrap();
    Fixture { store, account, work }
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn replay_reuses_the_generation_and_two_replicas_claim_it_only_once() {
    let f = fixture().await;
    let request = Uuid::new_v4();
    let first = f.store.admit_summary(f.account, f.work, request, NOW).await.unwrap();
    let repeated = f.store.admit_summary(f.account, f.work, request, NOW + 1).await.unwrap();
    assert!(first.created && !repeated.created);
    assert_eq!(first.generation.id, repeated.generation.id);
    let id = first.generation.id;
    let (a, b) = tokio::join!(
        f.store.claim_summary(f.account, f.work, id, NOW + 2, 16 * 1024 * 1024, "deepseek"),
        f.store.claim_summary(f.account, f.work, id, NOW + 2, 16 * 1024 * 1024, "deepseek"),
    );
    assert_eq!(usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()), 1, "only one durable execution may dispatch");
    let mut tx = f.store.begin().await.unwrap();
    tx.lock_account(f.account).await.unwrap();
    let other = Uuid::new_v4();
    let mut source = tx.load_work(f.work).await.unwrap().unwrap();
    source.id = other;
    tx.insert_work(&source).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(f.store.admit_summary(f.account, other, request, NOW + 3).await, Err(SummaryError::Conflict)), "same request id cannot mean another operation");
    let stranger = fixture().await;
    assert!(matches!(f.store.read_summary_generation(stranger.account, f.work, id, NOW + 4).await, Err(SummaryError::NotFound)));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn persisted_staged_final_reconciles_on_restart_and_survives_transcript_retention() {
    let f = fixture().await;
    let admitted = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap();
    let (g, _) = f.store.claim_summary(f.account, f.work, admitted.generation.id, NOW + 1, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "Hello".into(), false, NOW + 2).await.unwrap();
    f.store.checkpoint_summary(&g, "Hello world".into(), true, NOW + 3).await.unwrap();
    // Simulates the process disappearing after the successful stage COMMIT but before final save.
    // A fresh store has no LLM at all; reconciliation must use only the durable staged final.
    let restarted = Store::new(f.store.pool().clone());
    let recovered = restarted.read_summary_generation(f.account, f.work, g.id, NOW + 4).await.unwrap();
    assert_eq!((recovered.status, recovered.text.as_str()), (GenerationStatus::Completed, "Hello world"));
    let saved = restarted.read_summary(f.account, f.work, NOW + 5).await.unwrap().summary.unwrap();
    assert_eq!((saved.generation_id, saved.text.as_str()), (g.id, "Hello world"));
    assert_eq!(restarted.cancel_summary(f.account, f.work, g.id, NOW + 6).await.unwrap(), recovered, "terminal cancellation cannot delete a saved answer");

    let mut tx = restarted.begin().await.unwrap();
    tx.lock_account(f.account).await.unwrap();
    let mut source = tx.load_work(f.work).await.unwrap().unwrap();
    let before = source.revision;
    source.transcript = None;
    source.revision += 1;
    tx.save_work(&source, before, &[]).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(restarted.read_summary(f.account, f.work, NOW + 7).await.unwrap().summary, Some(saved));
    assert!(matches!(restarted.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 8).await, Err(SummaryError::NotReady)), "retention preserves completion, not permission for new source-less work");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn cancellation_source_revision_and_expired_execution_fence_late_answers() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 1, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap();
    let cancelled = f.store.cancel_summary(f.account, f.work, id, NOW + 2).await.unwrap();
    assert_eq!(cancelled.status, GenerationStatus::Cancelled);
    assert!(!f.store.checkpoint_summary(&g, "late final".into(), true, NOW + 3).await.unwrap());
    assert!(f.store.read_summary(f.account, f.work, NOW + 4).await.unwrap().summary.is_none());

    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 5).await.unwrap().generation.id;
    let (active, _) = f.store.claim_summary(f.account, f.work, id, NOW + 6, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap();
    let mut tx = f.store.begin().await.unwrap();
    tx.lock_account(f.account).await.unwrap();
    let mut work = tx.load_work(f.work).await.unwrap().unwrap();
    let revision = work.revision;
    work.revision += 1;
    tx.save_work(&work, revision, &[]).await.unwrap();
    tx.commit().await.unwrap();
    assert!(!f.store.checkpoint_summary(&active, "stale".into(), true, NOW + 7).await.unwrap());
    assert_eq!(f.store.read_summary_generation(f.account, f.work, id, NOW + 8).await.unwrap().status, GenerationStatus::Interrupted);

    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 9).await.unwrap().generation.id;
    f.store.claim_summary(f.account, f.work, id, NOW + 10, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap();
    let recovered = f.store.read_summary_generation(f.account, f.work, id, NOW + 70_000_000).await.unwrap();
    assert_eq!(recovered.status, GenerationStatus::Interrupted);
    assert!(recovered.remote_outcome_unknown);
    assert!(f.store.claim_summary(f.account, f.work, id, NOW + 70_000_001, 16 * 1024 * 1024, "deepseek").await.unwrap().is_none(), "restart recovery never dispatches the interrupted identity again");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn account_cascade_erases_all_summary_content_and_late_provider_identity_cannot_resurrect_it() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 1, 16 * 1024 * 1024, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "private partial".into(), false, NOW + 2).await.unwrap();
    f.store.open_barrier(f.account, Micros(NOW + 3)).await.unwrap();
    open_account_erasure(&PgBillingStore::new(f.store.pool().clone()), AccountId(f.account), BillingMicros(NOW + 3)).await.unwrap();
    f.store.erase_data(f.account, Micros(NOW + 4), Micros(NOW + 900_000_000)).await.unwrap();
    assert!(matches!(f.store.read_summary_generation(f.account, f.work, id, NOW + 5).await, Err(SummaryError::Erased)));
    let content: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM taskr_work.summary_generations WHERE account_id=$1) + (SELECT count(*) FROM taskr_work.summaries WHERE account_id=$1)")
        .bind(f.account).fetch_one(f.store.pool()).await.unwrap();
    assert_eq!(content, 0);
    assert!(!f.store.record_summary_remote(&g, Some("late-request-id"), Some(llm_core::RemoteOutcome::Unknown), NOW + 6).await.unwrap());
    let resources = f.store.erasure_resources(f.account).await.unwrap();
    let external = resources.iter().find(|r| r.kind == "llm").expect("uncertain external processing remains explicit");
    assert_eq!(external.target["remote_ids"], serde_json::json!(["late-request-id"]));
    assert!(external.target.get("text").is_none() && external.target.get("transcript").is_none());
    assert!(matches!(f.store.checkpoint_summary(&g, "late final".into(), true, NOW + 7).await, Err(SummaryError::Erased)));
    assert!(!f.store.finish_erasure(f.account, Micros(NOW + 8)).await.unwrap(), "external uncertainty is never an auto-exception");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn the_first_claim_starts_a_full_execution_lease_and_never_dispatches_twice() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    // Claimed late in the admission window, e.g. by a replayed start after the admitting process died.
    let claim_at = NOW + 50_000_000;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, claim_at, LIMIT, "deepseek").await.unwrap().unwrap();
    assert!(g.lease_until_us >= claim_at + 60_000_000, "the execution lease covers the whole provider deadline from the claim");
    let column: i64 = sqlx::query_scalar("SELECT lease_until_us FROM taskr_work.summary_generations WHERE id = $1")
        .bind(id).fetch_one(f.store.pool()).await.unwrap();
    assert_eq!(column, g.lease_until_us, "recovery reads the refreshed lease, not the admission one");
    assert!(f.store.checkpoint_summary(&g, "Hello".into(), false, claim_at + 59_000_000).await.unwrap(),
        "a checkpoint inside the provider deadline is still current");
    assert!(f.store.claim_summary(f.account, f.work, id, claim_at + 1, LIMIT, "deepseek").await.unwrap().is_none(), "a running generation is never claimed again");

    let unclaimed = fixture().await;
    let id = unclaimed.store.admit_summary(unclaimed.account, unclaimed.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    assert!(unclaimed.store.claim_summary(unclaimed.account, unclaimed.work, id, NOW + 65_000_001, LIMIT, "deepseek").await.unwrap().is_none(),
        "an accepted generation that waited past its admission lease is never dispatched");
    assert_eq!(unclaimed.store.read_summary_generation(unclaimed.account, unclaimed.work, id, NOW + 65_000_002).await.unwrap().status, GenerationStatus::Interrupted);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn cancel_wins_over_a_noncanonical_staged_final_but_never_over_a_saved_summary() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "Hello".into(), false, NOW + 2).await.unwrap();
    assert!(f.store.checkpoint_summary(&g, "Hello world".into(), true, NOW + 3).await.unwrap());
    // Lead decision: a stage is not the canonical summary; cancel committed first wins.
    let cancelled = f.store.cancel_summary(f.account, f.work, id, NOW + 4).await.unwrap();
    assert_eq!((cancelled.status, cancelled.text.as_str(), cancelled.staged.as_deref()), (GenerationStatus::Cancelled, "Hello", None));
    assert!(!f.store.settle_summary(f.account, f.work, id, g.execution, NOW + 5).await.unwrap(), "a late settlement cannot resurrect a cancelled stage");
    let view = f.store.read_summary(f.account, f.work, NOW + 6).await.unwrap();
    assert_eq!((view.summary, view.generation.map(|g| g.status)), (None, Some(GenerationStatus::Cancelled)));

    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 7).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 8, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "Saved".into(), true, NOW + 9).await.unwrap();
    assert!(f.store.settle_summary(f.account, f.work, id, g.execution, NOW + 10).await.unwrap());
    assert_eq!(f.store.cancel_summary(f.account, f.work, id, NOW + 11).await.unwrap().status, GenerationStatus::Completed);
    assert_eq!(f.store.read_summary(f.account, f.work, NOW + 12).await.unwrap().summary.map(|s| s.text), Some("Saved".to_string()));
}

async fn stored(store: &Store, id: Uuid) -> Generation {
    let sqlx::types::Json(g): sqlx::types::Json<Generation> = sqlx::query_scalar("SELECT payload FROM taskr_work.summary_generations WHERE id = $1")
        .bind(id).fetch_one(store.pool()).await.unwrap();
    g
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn transcript_retention_clears_noncanonical_partials_and_keeps_reasons_provider_facts_and_the_saved_summary() {
    let f = fixture().await;
    let old = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, old, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "old private partial".into(), false, NOW + 2).await.unwrap();
    f.store.cancel_summary(f.account, f.work, old, NOW + 3).await.unwrap();

    let saved = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 4).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, saved, NOW + 5, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "Saved summary".into(), true, NOW + 6).await.unwrap();
    assert!(f.store.settle_summary(f.account, f.work, saved, g.execution, NOW + 7).await.unwrap());

    let active = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW + 8).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, active, NOW + 9, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.checkpoint_summary(&g, "running private partial".into(), false, NOW + 10).await.unwrap();
    f.store.checkpoint_summary(&g, "running private final".into(), true, NOW + 11).await.unwrap();

    // The retention purge's own save of a completed work without its transcript.
    let mut tx = f.store.begin().await.unwrap();
    tx.lock_account(f.account).await.unwrap();
    let mut source = tx.load_work(f.work).await.unwrap().unwrap();
    let before = source.revision;
    assert_eq!(source.purge_transcript(), crate::work::Applied::Changed);
    tx.save_work(&source, before, &[]).await.unwrap();
    tx.commit().await.unwrap();

    // At rest, before any read could reconcile anything.
    let old = stored(&f.store, old).await;
    assert_eq!((old.status, old.text.as_str(), old.staged.as_deref()), (GenerationStatus::Cancelled, "", None));
    assert_eq!(old.provider.as_deref(), Some("deepseek"), "minimal provider facts survive for erasure");
    let active = stored(&f.store, active).await;
    assert_eq!((active.status, active.text.as_str(), active.staged.as_deref()), (GenerationStatus::Interrupted, "", None));
    assert!(active.error.is_some() && active.remote_outcome_unknown && active.provider.is_some(), "the reason and dispatch facts stay explicit");
    let saved = stored(&f.store, saved).await;
    assert_eq!((saved.status, saved.text.as_str()), (GenerationStatus::Completed, "Saved summary"));
    assert_eq!(f.store.read_summary(f.account, f.work, NOW + 12).await.unwrap().summary.map(|s| s.text), Some("Saved summary".to_string()));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn distinct_provider_ids_accumulate_before_and_after_the_erasure_cascade() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    assert!(f.store.record_summary_remote(&g, Some("hdr-1"), None, NOW + 2).await.unwrap());
    assert!(f.store.record_summary_remote(&g, Some("hdr-1"), None, NOW + 3).await.unwrap(), "a repeated id is not a second identity");
    f.store.open_barrier(f.account, Micros(NOW + 4)).await.unwrap();
    open_account_erasure(&PgBillingStore::new(f.store.pool().clone()), AccountId(f.account), BillingMicros(NOW + 4)).await.unwrap();
    f.store.erase_data(f.account, Micros(NOW + 5), Micros(NOW + 900_000_000)).await.unwrap();
    // The body id arrives after the local cascade removed the generation.
    assert!(!f.store.record_summary_remote(&g, Some("body-1"), Some(llm_core::RemoteOutcome::Finished), NOW + 6).await.unwrap());
    let resources = f.store.erasure_resources(f.account).await.unwrap();
    let external: Vec<_> = resources.iter().filter(|r| r.kind == "llm").collect();
    assert_eq!(external.len(), 1, "one resource per dispatched generation");
    assert_eq!(external[0].target["remote_ids"], serde_json::json!(["hdr-1", "body-1"]), "the pre-cascade id is not overwritten");
    assert_eq!(external[0].target["provider"], "deepseek");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn identities_observed_but_not_recorded_are_carried_into_the_interruption_or_the_erasure_manifest() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = f.store.claim_summary(f.account, f.work, id, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    assert!(f.store.record_summary_remote(&g, Some("hdr-1"), None, NOW + 2).await.unwrap());
    // The driver saw `body-1` but its own record transaction never committed.
    let observed = ["hdr-1".to_string(), "body-1".to_string()];
    f.store.interrupt_summary(&g, "The provider connection ended without a successful terminal completion.".into(), &observed, NOW + 3).await.unwrap();
    let stored = stored(&f.store, id).await;
    assert_eq!(stored.status, GenerationStatus::Interrupted);
    assert_eq!(stored.remote_ids, ["hdr-1", "body-1"]);
    assert!(stored.error.unwrap().contains("terminal completion"));

    let erased = fixture().await;
    let id = erased.store.admit_summary(erased.account, erased.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    let (g, _) = erased.store.claim_summary(erased.account, erased.work, id, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    assert!(erased.store.record_summary_remote(&g, Some("hdr-2"), None, NOW + 2).await.unwrap());
    erased.store.open_barrier(erased.account, Micros(NOW + 3)).await.unwrap();
    erased.store.interrupt_summary(&g, "The completion deadline elapsed; the provider outcome may be unknown.".into(), &["body-2".to_string()], NOW + 4).await.unwrap();
    open_account_erasure(&PgBillingStore::new(erased.store.pool().clone()), AccountId(erased.account), BillingMicros(NOW + 4)).await.unwrap();
    erased.store.erase_data(erased.account, Micros(NOW + 5), Micros(NOW + 900_000_000)).await.unwrap();
    let resources = erased.store.erasure_resources(erased.account).await.unwrap();
    let external: Vec<_> = resources.iter().filter(|r| r.kind == "llm").collect();
    assert_eq!(external.len(), 1);
    assert_eq!(external[0].target["remote_ids"], serde_json::json!(["hdr-2", "body-2"]), "the recorded id seeds the manifest and the observed one joins it");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn migration_0003_converts_the_0002_scalar_provider_id_without_losing_it() {
    let f = fixture().await;
    let id = f.store.admit_summary(f.account, f.work, Uuid::new_v4(), NOW).await.unwrap().generation.id;
    f.store.claim_summary(f.account, f.work, id, NOW + 1, LIMIT, "deepseek").await.unwrap().unwrap();
    f.store.open_barrier(f.account, Micros(NOW + 2)).await.unwrap();
    let url = std::env::var("TASKR_WORK_TEST_DATABASE_URL").unwrap();
    let owner = billing_postgres::connect_owner(&url).await.unwrap();
    // Rows exactly as 0002-era code wrote them: one scalar identity, no list.
    sqlx::query("UPDATE taskr_work.summary_generations SET payload = (payload - 'remote_ids') || jsonb_build_object('remote_id', 'legacy-generation-id') WHERE id = $1")
        .bind(id).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO taskr_work.erasure_resources (account_id, kind, target) VALUES ($1, 'llm', jsonb_build_object('generation', $2::text, 'provider', 'deepseek', 'remote_id', 'legacy-resource-id'))")
        .bind(f.account).bind(id.to_string()).execute(&owner).await.unwrap();
    // Idempotent: rows that already carry a list are untouched by a rerun.
    sqlx::raw_sql(include_str!("../../migrations/0003_summary_remote_ids.sql")).execute(&owner).await.unwrap();
    let converted = stored(&f.store, id).await;
    assert_eq!(converted.remote_ids, ["legacy-generation-id"]);
    let raw: serde_json::Value = sqlx::query_scalar("SELECT payload FROM taskr_work.summary_generations WHERE id = $1").bind(id).fetch_one(&owner).await.unwrap();
    assert!(raw.get("remote_id").is_none(), "the scalar is replaced, not left beside the list");
    let resources = f.store.erasure_resources(f.account).await.unwrap();
    let legacy = resources.iter().find(|r| r.kind == "llm").unwrap();
    assert_eq!(legacy.target["remote_ids"], serde_json::json!(["legacy-resource-id"]));
    assert!(legacy.target.get("remote_id").is_none());
}
