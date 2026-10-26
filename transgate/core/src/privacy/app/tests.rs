use std::collections::BTreeSet;
use std::sync::Mutex;

use billing_core::erasure::{TerminationEvidence, TerminationTarget};
use billing_core::ports::GatewayError;
use media_ports::{ObjectRef, SecretString, StoreKind};

use super::*;
use crate::ids::{AttemptId, JobId, MICROS_PER_DAY};
use crate::jobs::domain::{Attempt, AttemptRequest, AttemptState, JobSource, JobStatus, RemoteCleanup, StorageProvider};
use crate::jobs::ports::JobQueries;
use crate::jobs::testing::{
    FakeIdentity, FakeInventory, FakeObjects, FakeTerminator, FakeTranscriber, MemBillingStore, MemStore, TestClock,
    TestLocks,
};
use crate::presentation::fixtures;
use crate::privacy::settings::UserSettings;

const NOW: Micros = Micros(1_800_000_000_000_000);

struct World {
    store: MemStore,
    identity: Arc<FakeIdentity>,
    objects: Arc<FakeObjects>,
    soniox: Arc<FakeTranscriber>,
    deepgram: Arc<FakeTranscriber>,
    terminator: Arc<FakeTerminator>,
    locks: Arc<TestLocks>,
    /// The legacy Supabase `jobs` bucket and its prefix inventory (D2/F9).
    legacy: Arc<FakeObjects>,
    inventory: Arc<FakeInventory>,
    clock: Arc<TestClock>,
}

impl World {
    fn new() -> World {
        let legacy = Arc::new(FakeObjects { kind: Some(StoreKind::Supabase), bucket_name: Some("jobs"), ..FakeObjects::default() });
        World {
            store: MemStore::default(),
            identity: Arc::new(FakeIdentity { outcome: Mutex::new(Ok(IdentityOutcome::Deleted)), calls: Mutex::new(0) }),
            objects: Arc::new(FakeObjects::default()),
            soniox: Arc::new(FakeTranscriber::new(media_ports::Provider::Soniox)),
            deepgram: Arc::new(FakeTranscriber::new(media_ports::Provider::Deepgram)),
            terminator: Arc::new(FakeTerminator { evidence: Mutex::new(Ok(TerminationEvidence::CancelSucceeded)) }),
            locks: Arc::new(TestLocks { held: Arc::new(Mutex::new(BTreeSet::new())) }),
            inventory: Arc::new(FakeInventory::over(legacy.clone())),
            legacy,
            clock: Arc::new(TestClock(Mutex::new(NOW))),
        }
    }

    fn privacy(&self, exceptions: ExceptionPolicy) -> Privacy {
        self.privacy_with(exceptions, vec![self.inventory.clone()])
    }

    fn privacy_with(&self, exceptions: ExceptionPolicy, inventories: Vec<Arc<dyn ObjectInventory>>) -> Privacy {
        Privacy::new(PrivacyServices {
            store: Arc::new(self.store.clone()),
            clock: self.clock.clone(),
            locks: self.locks.clone(),
            objects: vec![self.objects.clone(), self.legacy.clone()],
            inventories,
            transcribers: vec![self.soniox.clone(), self.deepgram.clone()],
            identity: self.identity.clone(),
            billing: Arc::new(MemBillingStore { data: self.store.data.clone() }),
            terminators: vec![self.terminator.clone()],
            exceptions,
        })
    }

    /// A billed account with one completed upload job, its line and an accepted attempt.
    fn seed(&self, provider: Provider) {
        let account = fixtures::account();
        let mut job = fixtures::job(1, JobStatus::Completed, "a.mp3");
        job.created_at = Micros(NOW.0 - 40 * MICROS_PER_DAY);
        let object = ObjectRef { store: StoreKind::Gcs, bucket: "bucket".into(), key: "u/1/a.mp3".into(), version: None };
        job.source = JobSource::Upload { storage: StorageProvider::Gcs, object: Some(object.clone()) };
        let attempt = Attempt {
            id: AttemptId::new(),
            job: job.id,
            account,
            number: 1,
            provider,
            project: "project".into(),
            model: "m".into(),
            state: AttemptState::Accepted,
            claimed_at: NOW,
            lease_until: NOW,
            request: AttemptRequest {
                language_code: None,
                translation_target: None,
                diarize: false,
                audio: object,
                client_reference: "job_1".into(),
            },
            callback_secret: SecretString::new("s"),
            remote_id: Some("r1".into()),
            accepted_at: Some(NOW),
            last_error: None,
            remote_cleanup: RemoteCleanup::NotRequested,
        };
        self.store.with(|d| {
            d.accounts.insert(account);
            d.jobs.insert(1, job);
            d.next_job = 1;
            d.lines.push(fixtures::line(1, 1, "00:00:00", "00:00:02", "hello", None));
            d.next_line = 1;
            d.attempts.insert(attempt.id, attempt);
        });
    }
}

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

fn target() -> TerminationTarget {
    TerminationTarget { gateway: "iyzico".into(), environment: "sandbox".into(), subscription_ref: "sub-1".into(), customer_ref: Some("c-1".into()) }
}

// PRV-001..010 intended, D9, C-11: barrier, billing first, local cascade, identity, then remote items.
#[test]
fn erasure_runs_to_fully_erased() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let p = w.privacy(ExceptionPolicy::default());
    let view = run(p.request_erasure(fixtures::account())).unwrap();
    // The first remote step is phase 0: the (empty) legacy folder, ahead of the identity (D2).
    assert_eq!(view, ErasureView::LocalDataErased { pending: 3, exceptions: vec![] });
    w.store.with(|d| {
        assert!(d.jobs.is_empty() && d.lines.is_empty() && d.attempts.is_empty());
        assert_eq!(d.billing_erased, vec![fixtures::account()]);
    });
    assert_eq!(*w.inventory.owners.lock().unwrap(), vec![fixtures::account().0.to_string()]);
    assert_eq!(*w.identity.calls.lock().unwrap(), 0);
    assert_eq!(run(p.erasure_pass()).unwrap(), 1);
    assert_eq!(*w.identity.calls.lock().unwrap(), 1);
    assert_eq!(run(p.erasure_pass()).unwrap(), 1);
    assert_eq!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::FullyErased);
    assert_eq!(*w.objects.deleted.lock().unwrap(), vec!["u/1/a.mp3".to_string()]);
    assert_eq!(w.soniox.deleted.lock().unwrap().len(), 1);
    // D9: the barrier outlives completion.
    assert!(run(w.store.barrier(fixtures::account())).unwrap());
    // A repeated request changes nothing.
    assert_eq!(run(p.request_erasure(fixtures::account())).unwrap(), ErasureView::FullyErased);
}

// D10: nothing local is erased while a verified subscription may still renew.
#[test]
fn erasure_waits_for_subscription_stop() {
    let w = World::new();
    w.seed(Provider::Soniox);
    w.store.with(|d| d.bound_subscriptions.push((fixtures::account(), target())));
    *w.terminator.evidence.lock().unwrap() = Err(GatewayError::Uncertain("timeout".into()));
    let p = w.privacy(ExceptionPolicy::default());
    let view = run(p.request_erasure(fixtures::account())).unwrap();
    assert_eq!(view, ErasureView::Requested { waiting_for: vec!["subscription sub-1 at iyzico: not yet confirmed stopped".into()] });
    w.store.with(|d| assert_eq!(d.jobs.len(), 1));
    assert_eq!(*w.identity.calls.lock().unwrap(), 0);
    *w.terminator.evidence.lock().unwrap() = Ok(TerminationEvidence::CancelSucceeded);
    run(p.erasure_pass()).unwrap();
    w.store.with(|d| assert!(d.jobs.is_empty()));
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }));
}

// D3, C-11: a provider copy without a delete API never counts as erased; only an approval closes it.
#[test]
fn unsupported_provider_copy_needs_an_approved_exception() {
    let w = World::new();
    w.seed(Provider::Deepgram);
    *w.deepgram.delete_outcome.lock().unwrap() = media_ports::DeleteOutcome::Unsupported { reason: "no delete api".into() };
    let p = w.privacy(ExceptionPolicy::default());
    run(p.request_erasure(fixtures::account())).unwrap();
    run(p.erasure_pass()).unwrap();
    run(p.erasure_pass()).unwrap();
    match run(p.erasure_status(fixtures::account())).unwrap() {
        ErasureView::LocalDataErased { pending, exceptions } => {
            assert_eq!(pending, 1);
            assert_eq!(exceptions, vec![format!("deepgram copy r1: {NO_PROVIDER_DELETE_API}")]);
        }
        other => panic!("{other:?}"),
    }
    let (id, item) = w.store.with(|d| {
        let id = *d.erasures.keys().next().unwrap();
        let item = d.items.iter().find(|i| matches!(i.target, ItemTarget::ProviderCopy { .. })).unwrap().id;
        (id, item)
    });
    run(p.approve_exception(id, item, &approval("ops@transgate", "Deepgram keeps no copy after mip_opt_out"))).unwrap();
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::ErasedWithExceptions { .. }));
}

// A deployment policy may pre-approve the reason; the result is still "with exceptions".
#[test]
fn policy_approved_exception() {
    let w = World::new();
    w.seed(Provider::Deepgram);
    *w.deepgram.delete_outcome.lock().unwrap() = media_ports::DeleteOutcome::Unsupported { reason: "no delete api".into() };
    let p = w.privacy(ExceptionPolicy { approved_unsupported: vec![NO_PROVIDER_DELETE_API.into()] });
    run(p.request_erasure(fixtures::account())).unwrap();
    run(p.erasure_pass()).unwrap();
    assert_eq!(run(p.erasure_pass()).unwrap(), 1);
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::ErasedWithExceptions { .. }));
}

fn legacy_inventory_state(w: &World) -> ItemState {
    w.store.with(|d| d.items.iter().find(|i| matches!(i.target, ItemTarget::LegacyAccountInventory { .. })).unwrap().state.clone())
}

// D2/F9: the legacy folder counts as erased only on an empty authenticated listing. A failed
// listing is never an empty one; listed objects are deleted through the bucket's adapter (another
// owner's are never touched), and the identity waits until the listing comes back empty.
#[test]
fn the_legacy_folder_closes_only_on_an_empty_listing() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let mine = format!("{}/7/soniox_transcript.json", fixtures::account().0);
    let other = format!("{}/7/soniox_transcript.json", uuid::Uuid::from_u128(9));
    w.legacy.present.lock().unwrap().extend([mine.clone(), other.clone()]);
    *w.inventory.error.lock().unwrap() = Some(media_ports::TransportError::Unavailable { status: Some(503), operation: "list".into() });
    let p = w.privacy(ExceptionPolicy::default());
    run(p.request_erasure(fixtures::account())).unwrap();
    assert!(matches!(legacy_inventory_state(&w), ItemState::Retrying { .. }));
    assert!(w.legacy.deleted.lock().unwrap().is_empty());
    *w.inventory.error.lock().unwrap() = None;
    w.clock.advance_minutes(24 * 60);
    run(p.erasure_pass()).unwrap();
    assert_eq!(*w.legacy.deleted.lock().unwrap(), vec![mine]);
    assert!(matches!(legacy_inventory_state(&w), ItemState::Retrying { .. }), "a listing that found objects never closes");
    assert_eq!(*w.identity.calls.lock().unwrap(), 0);
    w.clock.advance_minutes(24 * 60);
    run(p.erasure_pass()).unwrap();
    assert!(matches!(legacy_inventory_state(&w), ItemState::Absent { .. }));
    assert_eq!(*w.identity.calls.lock().unwrap(), 0, "the identity is the next step");
    run(p.erasure_pass()).unwrap();
    assert_eq!(*w.identity.calls.lock().unwrap(), 1);
    assert!(w.legacy.present.lock().unwrap().contains(&other));
}

// D2/F9: without a configured legacy inventory the folder stays an explicit unresolved item; the
// erasure never closes and the identity is never deleted ahead of it.
#[test]
fn an_unconfigured_legacy_inventory_keeps_the_erasure_open() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let p = w.privacy_with(ExceptionPolicy::default(), vec![]);
    run(p.request_erasure(fixtures::account())).unwrap();
    for _ in 0..3 {
        w.clock.advance_minutes(24 * 60);
        run(p.erasure_pass()).unwrap();
    }
    match legacy_inventory_state(&w) {
        ItemState::Unresolved { reason, .. } => assert_eq!(reason, "legacy storage inventory is not configured"),
        other => panic!("{other:?}"),
    }
    assert_eq!(*w.identity.calls.lock().unwrap(), 0);
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }));
}

fn approval(operator: &str, rationale: &str) -> OperatorApproval {
    OperatorApproval { operator: operator.into(), rationale: rationale.into() }
}

// D15: an upload URL's write cannot be observed to end, so after the URL expired the re-check
// stays unresolved. A policy pre-approving that very reason never closes it. An approval without
// a verified operator or without a rationale is refused and changes nothing; done work is not
// approvable. Only an operator's explicit approval with a rationale closes it, as "with
// exceptions", keeping the approver, rationale and the item's target.
#[test]
fn an_unobservable_upload_closes_only_by_operator_approval() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let object = ObjectRef { store: StoreKind::Gcs, bucket: "bucket".into(), key: "u/1/b.mp3".into(), version: None };
    w.store.with(|d| {
        d.uploads.push(crate::jobs::ports::IssuedUpload { account: fixtures::account(), job: JobId(1), object, expires_at: Micros(NOW.0 - 1) })
    });
    let policy = ExceptionPolicy { approved_unsupported: vec![erasure::UPLOAD_OUTCOME_UNKNOWN.into()] };
    let p = w.privacy(policy);
    run(p.request_erasure(fixtures::account())).unwrap();
    run(p.erasure_pass()).unwrap();
    run(p.erasure_pass()).unwrap();
    let (id, item) = w.store.with(|d| {
        let item = d.items.iter().find(|i| matches!(i.target, ItemTarget::UploadResweep { .. })).unwrap();
        assert!(matches!(&item.state, ItemState::Unresolved { reason, .. } if reason == erasure::UPLOAD_OUTCOME_UNKNOWN), "{:?}", item.state);
        (*d.erasures.keys().next().unwrap(), item.id)
    });
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }));
    let identity = w.store.with(|d| d.items.iter().find(|i| matches!(i.target, ItemTarget::Identity)).unwrap().id);
    let rationale = "upload URL expired; a late PUT cannot be ruled out";
    assert!(matches!(run(p.approve_exception(id, identity, &approval("ops@transgate", rationale))), Err(JobsError::WrongState(_))), "done work is not approvable");
    assert!(matches!(run(p.approve_exception(id, item, &approval("  ", rationale))), Err(JobsError::InvalidRequest(_))), "no verified operator");
    assert!(matches!(run(p.approve_exception(id, item, &approval("ops@transgate", " "))), Err(JobsError::InvalidRequest(_))), "no rationale");
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }), "refusals change nothing");
    run(p.approve_exception(id, item, &approval("ops@transgate", rationale))).unwrap();
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::ErasedWithExceptions { .. }));
    let (record, approved) = w.store.with(|d| (d.erasures[&id].clone(), d.items.iter().find(|i| i.id == item).unwrap().clone()));
    assert_eq!(record.state, ErasureState::ErasedWithExceptions);
    assert!(matches!(approved.target, ItemTarget::UploadResweep { .. }));
    assert!(matches!(&approved.state, ItemState::ApprovedException { reason, approved_by, .. } if reason == rationale && approved_by == "ops@transgate"));
}

// D15: unresolved legacy inventory is not an unobservable write; no operator approval closes it.
#[test]
fn an_unresolved_legacy_inventory_cannot_be_approved_away() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let p = w.privacy_with(ExceptionPolicy::default(), vec![]);
    run(p.request_erasure(fixtures::account())).unwrap();
    let (id, item) = w.store.with(|d| {
        let item = d.items.iter().find(|i| matches!(i.target, ItemTarget::LegacyAccountInventory { .. })).unwrap();
        assert!(matches!(item.state, ItemState::Unresolved { .. }));
        (*d.erasures.keys().next().unwrap(), item.id)
    });
    let refused = run(p.approve_exception(id, item, &approval("ops@transgate", "accept the risk")));
    assert!(matches!(refused, Err(JobsError::WrongState(_))));
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }));
}

// D3/D9: a possible LLM dispatch is captured by account erasure before the cascade and stays
// unresolved: never fully erased, not closable by a deployment exception policy naming its
// reason, only by an operator's explicit approval with a rationale, as "with exceptions". A
// definitively undispatched request is absent.
#[test]
fn a_possible_llm_dispatch_keeps_erasure_open_until_an_operator_approves() {
    use crate::ai::ports::{DispatchDisposition, LlmDispatch};
    let w = World::new();
    w.seed(Provider::Soniox);
    let dispatch = |n: u128, disposition, ids: Vec<&str>| LlmDispatch {
        account: fixtures::account(),
        generation: crate::ai::GenerationId(uuid::Uuid::from_u128(n)),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        disposition,
        remote_ids: ids.into_iter().map(String::from).collect(),
        unkept_id_observed: false,
        created_at: NOW,
    };
    w.store.with(|d| {
        d.ai.dispatches.push(dispatch(1, DispatchDisposition::Possible, vec!["req-1", "cmpl-1"]));
        d.ai.dispatches.push(dispatch(2, DispatchDisposition::NotDispatched, vec![]));
    });
    let policy = ExceptionPolicy { approved_unsupported: vec![erasure::LLM_RETENTION_UNVERIFIED.into()] };
    let p = w.privacy(policy);
    run(p.request_erasure(fixtures::account())).unwrap();
    for _ in 0..3 {
        run(p.erasure_pass()).unwrap();
    }
    let (id, possible, absent, left) = w.store.with(|d| {
        let llm: Vec<_> = d.items.iter().filter(|i| matches!(i.target, ItemTarget::LlmDispatch { .. })).cloned().collect();
        (*d.erasures.keys().next().unwrap(), llm[0].clone(), llm[1].clone(), d.ai.dispatches.len())
    });
    assert_eq!(left, 0, "the cascade removed the intents after the manifest captured them");
    assert_eq!(
        possible.target,
        ItemTarget::LlmDispatch { generation: uuid::Uuid::from_u128(1), provider: "deepseek".into(), remote_ids: vec!["req-1".into(), "cmpl-1".into()], unkept_id_observed: false }
    );
    assert!(matches!(possible.state, ItemState::Unresolved { .. }), "{:?}", possible.state);
    assert!(matches!(absent.state, ItemState::Absent { .. }));
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::LocalDataErased { .. }), "never fully erased by policy");
    assert!(matches!(run(p.approve_exception(id, possible.id, &approval("ops@transgate", " "))), Err(JobsError::InvalidRequest(_))));
    run(p.approve_exception(id, possible.id, &approval("ops@transgate", "DeepSeek retention accepted for this account"))).unwrap();
    assert!(matches!(run(p.erasure_status(fixtures::account())).unwrap(), ErasureView::ErasedWithExceptions { .. }));
}

// PRV-013/016/022: a missing settings row is the column default (true) and deletes; an
// explicit opt-out keeps its lines and is counted.
#[test]
fn retention_respects_the_preference() {
    let w = World::new();
    w.seed(Provider::Soniox);
    let other = crate::ids::AccountId(uuid::Uuid::from_u128(9));
    w.store.with(|d| {
        let mut job = fixtures::job(2, JobStatus::Completed, "b.mp3");
        job.account = other;
        job.created_at = Micros(NOW.0 - 40 * MICROS_PER_DAY);
        d.jobs.insert(2, job);
        let mut line = fixtures::line(2, 2, "00:00:00", "00:00:01", "kept", None);
        line.account = other;
        d.lines.push(line);
        d.accounts.insert(other);
        // The seeded account has no settings row: the default (true) applies.
        let mut s = UserSettings::defaults(other);
        s.auto_delete_transcripts = false;
        d.settings.insert(other, s);
    });
    let p = w.privacy(ExceptionPolicy::default());
    let outcome = run(p.retention_run(RetentionDays::SCHEDULED, "cron")).unwrap();
    assert_eq!(outcome, RetentionOutcome::Success { rows_deleted: 1, users_opted_out: 1 });
    w.store.with(|d| {
        assert_eq!(d.lines.iter().map(|l| l.job).collect::<Vec<_>>(), vec![JobId(2)]);
        assert_eq!(d.runs.len(), 1);
    });
}

// PRV-020: a concurrent run is recorded as skipped and deletes nothing.
#[test]
fn retention_skips_while_locked() {
    let w = World::new();
    w.seed(Provider::Soniox);
    w.locks.held.lock().unwrap().insert(TRANSCRIPT_LOCK.to_string());
    let p = w.privacy(ExceptionPolicy::default());
    assert_eq!(run(p.retention_run(RetentionDays::SCHEDULED, "cron")).unwrap(), RetentionOutcome::Skipped);
    w.store.with(|d| {
        assert_eq!(d.lines.len(), 1);
        assert_eq!(d.runs.len(), 1);
    });
}

fn history_run(schedule: Schedule, days_old: i64, finished: bool) -> crate::jobs::testing::ScheduleRun {
    crate::jobs::testing::ScheduleRun { schedule: schedule.name().into(), started_at: Micros(NOW.0 - days_old * MICROS_PER_DAY), finished }
}

// PRV-025/031, CRN-007: finished scheduler runs started at or before now - 91 days go
// (start_time <= cutoff); unfinished claims and newer runs stay; the audit row has no
// opt-out count.
#[test]
fn history_retention_removes_only_old_finished_runs() {
    let w = World::new();
    w.store.with(|d| {
        d.schedule_runs = vec![
            history_run(Schedule::Jobs, 91, true),
            history_run(Schedule::TranscriptRetention, 120, true),
            history_run(Schedule::Jobs, 120, false),
            history_run(Schedule::RemoteCleanup, 90, true),
        ];
    });
    let p = w.privacy(ExceptionPolicy::default());
    let outcome = run(p.history_retention_run(retention::HISTORY_RETENTION_DAYS, "scheduler")).unwrap();
    assert_eq!(outcome, HistoryOutcome::Success { rows_deleted: 2 });
    assert_eq!(outcome.status().as_str(), "success");
    w.store.with(|d| {
        let kept: Vec<(bool, i64)> = d.schedule_runs.iter().map(|r| (r.finished, (NOW.0 - r.started_at.0) / MICROS_PER_DAY)).collect();
        assert_eq!(kept, vec![(false, 120), (true, 90)]);
        let audit = &d.runs[0];
        assert_eq!((audit.job_name.as_str(), audit.table_name.as_str(), audit.retention_days), ("cleanup-cron-job-history", "media.schedule_slots", 90));
        assert_eq!((audit.status, audit.rows_deleted, audit.users_opted_out_count), (retention::RunStatus::Success, 2, None));
    });
}

// PRV-019/020 for the history cleanup: a held lock writes skipped; more than 100000 rows
// aborts with nothing deleted.
#[test]
fn history_retention_skips_and_aborts_like_the_source() {
    let w = World::new();
    w.store.with(|d| d.schedule_runs = (0..100_001).map(|_| history_run(Schedule::Jobs, 100, true)).collect());
    w.locks.held.lock().unwrap().insert(HISTORY_LOCK.to_string());
    let p = w.privacy(ExceptionPolicy::default());
    assert_eq!(run(p.history_retention_run(retention::HISTORY_RETENTION_DAYS, "scheduler")).unwrap(), HistoryOutcome::Skipped);
    w.locks.held.lock().unwrap().clear();
    assert_eq!(
        run(p.history_retention_run(retention::HISTORY_RETENTION_DAYS, "scheduler")).unwrap(),
        HistoryOutcome::Aborted { would_delete: 100_001 }
    );
    w.store.with(|d| {
        assert_eq!(d.schedule_runs.len(), 100_001, "nothing deleted");
        let statuses: Vec<_> = d.runs.iter().map(|r| r.status).collect();
        assert_eq!(statuses, vec![retention::RunStatus::Skipped, retention::RunStatus::Aborted]);
        assert_eq!(d.runs[1].error_message.as_deref(), Some("SAFETY THRESHOLD EXCEEDED: Would delete 100001 rows (limit: 100000)"));
    });
}

// An interrupted run never stays `running` and is never reported as a success: the next run
// of the same job, under its lock, records it as a failure that deleted nothing. Another
// job's rows are untouched.
#[test]
fn the_next_run_records_an_interrupted_run_as_failed() {
    let w = World::new();
    let stale_history = RetentionRun::history_started(retention::HISTORY_RETENTION_DAYS, Micros(NOW.0 - MICROS_PER_DAY), "scheduler");
    let stale_transcripts = RetentionRun::started(RetentionDays::SCHEDULED, Micros(NOW.0 - MICROS_PER_DAY), "scheduler");
    w.store.with(|d| d.runs = vec![stale_history.clone(), stale_transcripts.clone()]);
    let p = w.privacy(ExceptionPolicy::default());
    run(p.history_retention_run(retention::HISTORY_RETENTION_DAYS, "scheduler")).unwrap();
    w.store.with(|d| {
        let history = d.runs.iter().find(|r| r.id == stale_history.id).unwrap();
        assert_eq!((history.status, history.rows_deleted), (retention::RunStatus::Failure, 0));
        assert_eq!(history.error_message.as_deref(), Some(retention::INTERRUPTED_MESSAGE));
        let transcripts = d.runs.iter().find(|r| r.id == stale_transcripts.id).unwrap();
        assert_eq!(transcripts.status, retention::RunStatus::Running, "only the same job's rows");
    });
    run(p.retention_run(RetentionDays::SCHEDULED, "scheduler")).unwrap();
    w.store.with(|d| {
        let transcripts = d.runs.iter().find(|r| r.id == stale_transcripts.id).unwrap();
        assert_eq!(transcripts.status, retention::RunStatus::Failure);
        assert!(d.runs.iter().all(|r| r.status != retention::RunStatus::Running));
    });
}
