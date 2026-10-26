//! Resource safety across the erasure barrier, driven through the real use cases
//! with scripted schedules: archive writes whose outcome arrives after erasure
//! ran or never arrives (F2), late upload writes (F1), and a capability signed
//! while the barrier commits (F7). No schedule is closed by the clock.

use std::collections::{BTreeMap, BTreeSet};
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use billing_core::erasure::{ErasureStatus, TerminationEvidence};
use billing_core::ledger::Package;
use billing_core::types::BoxFuture;
use media_ports::{
    AccessMethod, DeleteOutcome, ObjectInventory, ObjectMetadata, ObjectRef, ObjectStore, ObservationOrigin, PutObject,
    SensitiveUrl, StoreKind, TemporaryAccess, TransportError,
};

use super::super::{IntakeAck, Jobs, JobsConfig, JobsError, JobsServices, NewDraft, StartRequest};
use crate::ids::{ErasureId, InboxId, JobId, MICROS_PER_DAY, Micros};
use crate::jobs::domain::{Job, StorageProvider};
use crate::jobs::ingest;
use crate::jobs::ports::{ArchiveWrite, ArchivedPayload, IdentityOutcome};
use crate::jobs::routing::{ModelConfig, Preferences, RoutingConfig};
use crate::jobs::testing::{
    self, CountingLimiter, FakeIdentity, FakeTerminator, FakeTranscriber, MemBillingStore, MemStore, NoSleep, TestClock, TestLocks,
};
use crate::languages::Language;
use crate::presentation::fixtures;
use crate::privacy::app::{OperatorApproval, Privacy, PrivacyServices};
use crate::privacy::erasure::{
    ARCHIVE_OUTCOME_UNKNOWN, ErasureRecord, ErasureState, ErasureView, ExceptionPolicy, ItemState, ItemTarget,
    LEGACY_JOBS_BUCKET, UPLOAD_OUTCOME_UNKNOWN,
};

const NOW: Micros = Micros(1_800_000_000_000_000);

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

/// Both futures to completion on one task, polled in turn: a scripted interleaving.
async fn both<A, B>(a: impl Future<Output = A>, b: impl Future<Output = B>) -> (A, B) {
    let (mut a, mut b) = (pin!(a), pin!(b));
    let (mut ra, mut rb) = (None, None);
    std::future::poll_fn(|cx| {
        if ra.is_none() && let Poll::Ready(v) = a.as_mut().poll(cx) {
            ra = Some(v);
        }
        if rb.is_none() && let Poll::Ready(v) = b.as_mut().poll(cx) {
            rb = Some(v);
        }
        match (ra.take(), rb.take()) {
            (Some(x), Some(y)) => Poll::Ready((x, y)),
            (x, y) => {
                (ra, rb) = (x, y);
                Poll::Pending
            }
        }
    })
    .await
}

/// A one-shot latch: `wait()` stays pending until `open()`.
#[derive(Default)]
struct Gate(Mutex<(bool, Option<Waker>)>);

impl Gate {
    fn open(&self) {
        let mut s = self.0.lock().unwrap();
        s.0 = true;
        if let Some(w) = s.1.take() {
            w.wake();
        }
    }
    async fn wait(&self) {
        std::future::poll_fn(|cx| {
            let mut s = self.0.lock().unwrap();
            if s.0 {
                Poll::Ready(())
            } else {
                s.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

/// How the scripted store treats a PUT.
enum PutMode {
    /// Written and answered at once.
    Immediate,
    /// The answer is lost (`Unavailable`) and the object only becomes visible
    /// on `land()`: the store commits a write the client never heard about.
    LostAnswer,
    /// The request hangs until the gate opens, then writes and answers.
    Hang(Arc<Gate>),
}

type Hook = Box<dyn FnMut(&ObjectRef) + Send>;

struct Objects {
    present: Mutex<BTreeSet<String>>,
    landing: Mutex<Vec<String>>,
    put_mode: PutMode,
    put_started: Gate,
    /// When set, a HEAD answers what it saw at its start only once this opens.
    head_gate: Mutex<Option<Arc<Gate>>>,
    head_started: Gate,
    on_put: Mutex<Option<Hook>>,
    on_sign: Mutex<Option<Hook>>,
}

impl Objects {
    fn new(put_mode: PutMode) -> Objects {
        Objects {
            present: Mutex::default(),
            landing: Mutex::default(),
            put_mode,
            put_started: Gate::default(),
            head_gate: Mutex::default(),
            head_started: Gate::default(),
            on_put: Mutex::default(),
            on_sign: Mutex::default(),
        }
    }
    fn visible(&self, key: &str) -> bool {
        self.present.lock().unwrap().contains(key)
    }
    /// The client's own upload, or a late write landing.
    fn write(&self, key: &str) {
        self.present.lock().unwrap().insert(key.into());
    }
    fn land(&self) {
        let landed: Vec<String> = self.landing.lock().unwrap().drain(..).collect();
        self.present.lock().unwrap().extend(landed);
    }
}

impl ObjectStore for Objects {
    fn kind(&self) -> StoreKind {
        StoreKind::Gcs
    }
    fn bucket(&self) -> &str {
        "bucket"
    }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: media_ports::Micros, ttl: u32)
        -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        if let Some(hook) = self.on_sign.lock().unwrap().as_mut() {
            hook(object);
        }
        let access = TemporaryAccess {
            object: object.clone(),
            method,
            url: SensitiveUrl::new(format!("https://storage.test/{}", object.key)),
            expires_at: media_ports::Micros(now.0 + i64::from(ttl) * 1_000_000),
            headers: vec![],
        };
        Box::pin(async move { Ok(access) })
    }
    fn head<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        let meta = self.visible(&object.key).then(|| ObjectMetadata { object: object.clone(), size: 1, content_type: None, etag: None });
        let gate = self.head_gate.lock().unwrap().clone();
        if gate.is_some() {
            self.head_started.open();
        }
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.wait().await;
            }
            Ok(meta)
        })
    }
    fn put(&self, request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        if let Some(hook) = self.on_put.lock().unwrap().as_mut() {
            hook(&request.object);
        }
        self.put_started.open();
        let key = request.object.key.clone();
        let meta = ObjectMetadata { object: request.object, size: request.content_length, content_type: Some(request.content_type), etag: None };
        Box::pin(async move {
            match &self.put_mode {
                PutMode::Immediate => self.write(&key),
                PutMode::LostAnswer => {
                    self.landing.lock().unwrap().push(key);
                    return Err(TransportError::Unavailable { status: None, operation: "put".into() });
                }
                PutMode::Hang(gate) => {
                    gate.wait().await;
                    self.write(&key);
                }
            }
            Ok(meta)
        })
    }
    fn delete<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        let was = self.present.lock().unwrap().remove(&object.key);
        Box::pin(async move { Ok(if was { DeleteOutcome::Deleted } else { DeleteOutcome::Absent }) })
    }
}

/// The legacy bucket holds nothing for this account (authenticated, complete).
struct EmptyLegacyBucket;

impl ObjectInventory for EmptyLegacyBucket {
    fn kind(&self) -> StoreKind {
        StoreKind::Supabase
    }
    fn bucket(&self) -> &str {
        LEGACY_JOBS_BUCKET
    }
    fn inventory<'a>(&'a self, _owner: &'a str) -> BoxFuture<'a, Result<Vec<ObjectRef>, TransportError>> {
        Box::pin(async { Ok(vec![]) })
    }
}

struct World {
    store: MemStore,
    clock: Arc<TestClock>,
    objects: Arc<Objects>,
    soniox: Arc<FakeTranscriber>,
}

impl World {
    fn new(put_mode: PutMode) -> World {
        let w = World {
            store: MemStore::default(),
            clock: Arc::new(TestClock(Mutex::new(NOW))),
            objects: Arc::new(Objects::new(put_mode)),
            soniox: Arc::new(FakeTranscriber::new(media_ports::Provider::Soniox)),
        };
        w.store.with(|d| {
            d.accounts.insert(fixtures::account());
            d.packages.push(Package {
                id: 1,
                account: fixtures::account(),
                payment_id: None,
                name: Some("Pro".into()),
                minutes_included: 100,
                start_at: Micros(NOW.0 - 10 * MICROS_PER_DAY),
                end_at: Micros(NOW.0 + 30 * MICROS_PER_DAY),
            });
        });
        w
    }

    fn jobs(&self) -> Jobs {
        let config = JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig {
                soniox: "stt-async-v5".into(),
                soniox_v5: "stt-async-v5".into(),
                deepgram: "whisper-large".into(),
                deepgram_account_overrides: vec![],
            },
            upload_storage: StorageProvider::Gcs,
            upload_bucket: "bucket".into(),
            archive_store: StoreKind::Gcs,
            archive_bucket: "bucket".into(),
            staging_store: StoreKind::Gcs,
            staging_bucket: "bucket".into(),
            callback_base_url: "https://transgate.test/".into(),
            soniox_auto_delete: false,
            ssrf_protection: true,
            freemium_lists: Default::default(),
        };
        Jobs::new(
            JobsServices {
                store: Arc::new(self.store.clone()),
                clock: self.clock.clone(),
                objects: vec![self.objects.clone()],
                transcribers: vec![self.soniox.clone()],
                stager: None,
                sleeper: Arc::new(NoSleep),
                rate_limiter: Arc::new(CountingLimiter { hits: Mutex::new(BTreeMap::new()) }),
                paid_captions: None,
                video_metadata: None,
            },
            config,
        )
    }

    fn privacy(&self) -> Privacy {
        Privacy::new(PrivacyServices {
            store: Arc::new(self.store.clone()),
            clock: self.clock.clone(),
            locks: Arc::new(TestLocks { held: Arc::new(Mutex::new(BTreeSet::new())) }),
            objects: vec![self.objects.clone()],
            inventories: vec![Arc::new(EmptyLegacyBucket)],
            transcribers: vec![self.soniox.clone()],
            identity: Arc::new(FakeIdentity { outcome: Mutex::new(Ok(IdentityOutcome::Deleted)), calls: Mutex::new(0) }),
            billing: Arc::new(MemBillingStore { data: self.store.data.clone() }),
            terminators: vec![Arc::new(FakeTerminator { evidence: Mutex::new(Ok(TerminationEvidence::CancelSucceeded)) })],
            exceptions: ExceptionPolicy::default(),
        })
    }

    fn draft(&self, jobs: &Jobs) -> Job {
        let draft = NewDraft {
            name: "talk.mp3".into(),
            size_bytes: Some(10),
            language: Language::from_name("English"),
            speakers: false,
            duration_minutes: Some(2),
        };
        run(jobs.create_draft(fixtures::account(), draft)).unwrap()
    }

    /// A started Soniox job with its completion webhook stored; `ingest` of the
    /// inbox entry commits the result, then archives the raw payload.
    fn deliver(&self, jobs: &Jobs) -> (JobId, InboxId) {
        let job = self.draft(jobs);
        self.soniox.accept("tx-1");
        let start = StartRequest { job_ids: vec![job.id], locale: None, preferences: Preferences::default() };
        run(jobs.start(fixtures::account(), start)).unwrap();
        *self.soniox.completed.lock().unwrap() = Some(testing::soniox(
            "tx-1",
            ObservationOrigin::Webhook,
            "completed",
            None,
            Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
        ));
        let stored = self.store.with(|d| d.jobs[&job.id.0].clone());
        let attempt = self.store.with(|d| d.attempts[&stored.current_attempt.unwrap()].clone());
        let obs = testing::soniox("tx-1", ObservationOrigin::Webhook, "completed", Some(60_000.0), None);
        let IntakeAck::Stored { inbox } = run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap() else {
            panic!("not stored")
        };
        (job.id, inbox)
    }

    fn complete(&self, jobs: &Jobs) -> JobId {
        let (job, inbox) = self.deliver(jobs);
        run(jobs.ingest(inbox)).unwrap();
        job
    }

    fn status(&self, privacy: &Privacy) -> ErasureView {
        run(privacy.erasure_status(fixtures::account())).unwrap()
    }

    fn erase(&self, privacy: &Privacy) -> ErasureView {
        run(erase(privacy))
    }

    fn archive_write(&self, key: &str) -> Option<ArchiveWrite> {
        self.store.with(|d| d.archived.iter().find(|a| a.object.key == key).map(|a| a.write))
    }

    /// The key of the one recorded archive intent.
    fn archived_key(&self) -> String {
        self.store.with(|d| match d.archived.as_slice() {
            [one] => one.object.key.clone(),
            other => panic!("expected one archive intent, found {}", other.len()),
        })
    }

    /// The barrier, committed from outside: an erasure request racing the caller.
    fn commit_barrier(store: &MemStore) {
        store.with(|d| {
            let account = fixtures::account();
            d.billing_erasures.insert(
                account,
                ErasureStatus { started_at: NOW, billing_erased_at: None, pending: vec![], confirmed: vec![], unbound_plan_payments: vec![] },
            );
            let id = ErasureId::new();
            d.erasures.insert(
                id,
                ErasureRecord {
                    id,
                    account,
                    requested_at: NOW,
                    state: ErasureState::Requested,
                    local_erased_at: None,
                    identity_deleted_at: None,
                    closed_at: None,
                },
            );
        });
    }
}

/// JOB-044's source layout: the key every archive of this job would share.
fn shared_key(job: JobId) -> String {
    format!("{}/{}/soniox_transcript.json", fixtures::account(), job.0)
}

/// The owner's request, then the passes that take the saga through the
/// identity phase and the object phase (one phase per step).
async fn erase(privacy: &Privacy) -> ErasureView {
    privacy.request_erasure(fixtures::account()).await.unwrap();
    privacy.erasure_pass().await.unwrap();
    privacy.erasure_pass().await.unwrap();
    privacy.erasure_status(fixtures::account()).await.unwrap()
}

/// Local data erased, remote work open, and `reason` among the named problems.
fn open_with(view: &ErasureView, reason: &str) -> bool {
    matches!(view, ErasureView::LocalDataErased { exceptions, .. } if exceptions.iter().any(|e| e.contains(reason)))
}

// F2: erasure runs while our archive PUT is in flight: its miss proves nothing,
// so the saga stays open. The PUT then completes; its durable acknowledgement,
// arriving after the cascade removed the intent, reopens cleanup of the object.
#[test]
fn an_archive_put_completing_after_the_cascade_is_acknowledged_and_deleted() {
    let hang = Arc::new(Gate::default());
    let w = World::new(PutMode::Hang(hang.clone()));
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let (_, inbox) = w.deliver(&jobs);
    let erase_during_put = async {
        w.objects.put_started.wait().await;
        let key = w.archived_key();
        let view = erase(&privacy).await;
        let swept = !w.objects.visible(&key);
        hang.open();
        (key, view, swept)
    };
    let (ingested, (key, view, swept)) = run(both(jobs.ingest(inbox), erase_during_put));
    ingested.unwrap();
    assert!(swept, "erasure swept before the write landed");
    assert!(open_with(&view, ARCHIVE_OUTCOME_UNKNOWN), "{view:?}");
    assert!(w.objects.visible(&key), "the PUT completed after erasure's sweep");

    run(privacy.erasure_pass()).unwrap();
    assert!(!w.objects.visible(&key));
    assert_eq!(w.status(&privacy), ErasureView::FullyErased);
}

// D15: a known late completion must reopen an exception-labeled closure. The
// retained object identity, erasure barrier and identity-deletion evidence must
// survive, and completing cleanup must not erase the operator's audit record.
#[test]
fn a_late_archive_completion_reopens_an_exception_labeled_closure_without_losing_identity() {
    let hang = Arc::new(Gate::default());
    let w = World::new(PutMode::Hang(hang.clone()));
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let (job, inbox) = w.deliver(&jobs);
    let approve_during_put = async {
        w.objects.put_started.wait().await;
        let intent = w.store.with(|d| match d.archived.as_slice() {
            [one] => one.clone(),
            _ => panic!("expected the single durable archive intent"),
        });
        let view = erase(&privacy).await;
        assert!(open_with(&view, ARCHIVE_OUTCOME_UNKNOWN), "{view:?}");
        let (erasure, flight) = w.store.with(|d| {
            let record = d.erasures.values().find(|r| r.account == fixtures::account()).unwrap();
            let flight = d.items.iter().find(|i| i.erasure == record.id
                && matches!(&i.target, ItemTarget::ArchiveInFlight { object } if object == &intent.object)).unwrap();
            assert!(matches!(flight.state, ItemState::Unresolved { .. }));
            (record.id, flight.id)
        });
        let approval = OperatorApproval {
            operator: "privacy-operator".into(),
            rationale: "Explicitly accept the unobservable archive PUT outcome; retain its cleanup identity.".into(),
        };
        privacy.approve_exception(erasure, flight, &approval).await.unwrap();
        let closed_view = privacy.erasure_status(fixtures::account()).await.unwrap();
        assert!(matches!(closed_view, ErasureView::ErasedWithExceptions { .. }), "{closed_view:?}");
        let (closed, approved, identity) = w.store.with(|d| {
            let closed = d.erasures[&erasure].clone();
            let approved = d.items.iter().find(|i| i.id == flight).unwrap().clone();
            let identity = d.items.iter().find(|i| i.erasure == erasure && i.target == ItemTarget::Identity).unwrap().clone();
            assert_eq!(closed.state, ErasureState::ErasedWithExceptions);
            assert!(closed.closed_at.is_some() && closed.identity_deleted_at.is_some());
            assert_eq!(approved.state, ItemState::ApprovedException {
                reason: approval.rationale.clone(), approved_by: approval.operator.clone(), at: NOW,
            });
            assert!(identity.state.is_done());
            (closed, approved, identity)
        });
        assert!(!w.objects.visible(&intent.object.key), "the PUT is still in flight");
        hang.open();
        (intent, closed, approved, identity)
    };
    let (ingested, (intent, closed, approved, identity)) = run(both(jobs.ingest(inbox), approve_during_put));
    ingested.unwrap(); // the successful PUT drives the real Written acknowledgement
    assert!(w.objects.visible(&intent.object.key), "the late PUT must land before cleanup");
    assert_eq!(intent.write, ArchiveWrite::InFlight);
    assert_eq!(intent.job, job);
    w.store.with(|d| {
        let reopened = &d.erasures[&closed.id];
        assert_eq!(reopened.state, ErasureState::LocalErased);
        assert_eq!(reopened.closed_at, None);
        assert_eq!((reopened.id, reopened.account, reopened.requested_at, reopened.local_erased_at, reopened.identity_deleted_at),
            (closed.id, closed.account, closed.requested_at, closed.local_erased_at, closed.identity_deleted_at));
        assert_eq!(d.items.iter().find(|i| i.id == approved.id), Some(&approved), "retain the exact approved object identity and audit");
        assert_eq!(d.items.iter().find(|i| i.id == identity.id), Some(&identity), "do not repeat or replace identity deletion");
        assert!(d.items.iter().any(|i| i.erasure == closed.id && matches!(i.state, ItemState::Pending)
            && matches!(&i.target, ItemTarget::StorageObject { object } if object == &intent.object)),
            "the acknowledged object needs a fresh delete with its persisted reference");
    });
    assert!(matches!(w.status(&privacy), ErasureView::LocalDataErased { .. }));
    assert!(matches!(run(jobs.streaming_url(fixtures::account(), job, "talk.mp3")), Err(JobsError::ErasureInProgress)));

    run(privacy.erasure_pass()).unwrap();
    assert!(!w.objects.visible(&intent.object.key));
    assert!(matches!(w.status(&privacy), ErasureView::ErasedWithExceptions { .. }), "the explicit exception audit remains");
    w.store.with(|d| {
        assert_eq!(d.erasures[&closed.id].state, ErasureState::ErasedWithExceptions);
        assert_eq!(d.erasures[&closed.id].identity_deleted_at, closed.identity_deleted_at);
        assert_eq!(d.items.iter().find(|i| i.id == approved.id), Some(&approved));
        assert_eq!(d.items.iter().find(|i| i.id == identity.id), Some(&identity));
        assert!(d.items.iter().filter(|i| i.erasure == closed.id).all(|i| i.state.is_done()));
    });
}

// F2: a sweep loads the in-flight item and misses; before it saves, the PUT
// completes and its acknowledgement commits. The older miss must not overwrite
// the acknowledgement, or erasure would wait forever on a write already known.
#[test]
fn a_sweep_racing_the_acknowledgement_keeps_the_acknowledged_state() {
    let hang = Arc::new(Gate::default());
    let head = Arc::new(Gate::default());
    let w = World::new(PutMode::Hang(hang.clone()));
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let (_, inbox) = w.deliver(&jobs);
    let ingested = Gate::default();
    let ingest = async {
        let result = jobs.ingest(inbox).await;
        ingested.open();
        result
    };
    let erase = async {
        w.objects.put_started.wait().await;
        let key = w.archived_key();
        privacy.request_erasure(fixtures::account()).await.unwrap();
        privacy.erasure_pass().await.unwrap();
        *w.objects.head_gate.lock().unwrap() = Some(head.clone());
        // The object phase: the sweep misses, then waits before saving.
        privacy.erasure_pass().await.unwrap();
        // The delete the acknowledgement inserted.
        privacy.erasure_pass().await.unwrap();
        (key, privacy.erasure_status(fixtures::account()).await.unwrap())
    };
    let conduct = async {
        w.objects.head_started.wait().await;
        hang.open();
        ingested.wait().await;
        head.open();
    };
    let (ingest_result, ((key, view), ())) = run(both(ingest, both(erase, conduct)));
    ingest_result.unwrap();
    assert!(!w.objects.visible(&key));
    assert_eq!(view, ErasureView::FullyErased);
}

// F2: the PUT's answer is lost and the write lands later. No elapsed time closes
// the saga; only finding (and deleting) the object resolves the write.
#[test]
fn an_unacknowledged_archive_write_stays_pending_until_its_object_is_found() {
    let w = World::new(PutMode::LostAnswer);
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let job = w.complete(&jobs);
    let key = w.archived_key();
    assert!(key.starts_with(&format!("{}/{}/", fixtures::account(), job.0)) && key != shared_key(job), "{key}");
    assert_eq!(w.archive_write(&key), Some(ArchiveWrite::InFlight), "an unknown outcome is not settled");
    // An older, settled archive of the same job at the source's shared key: its
    // presence is no evidence that this PUT landed.
    let older = ArchivedPayload {
        account: fixtures::account(),
        job,
        object: ObjectRef { store: StoreKind::Gcs, bucket: "bucket".into(), key: shared_key(job), version: None },
        write: ArchiveWrite::Written,
    };
    w.objects.write(&older.object.key);
    w.store.with(|d| d.archived.push(older));

    let view = w.erase(&privacy);
    assert!(open_with(&view, ARCHIVE_OUTCOME_UNKNOWN), "{view:?}");
    assert!(!w.objects.visible(&shared_key(job)), "the older archive is deleted on its own");
    w.clock.advance_minutes(30 * 24 * 60);
    run(privacy.erasure_pass()).unwrap();
    assert!(open_with(&w.status(&privacy), ARCHIVE_OUTCOME_UNKNOWN), "a month of misses proves nothing");

    w.objects.land();
    w.clock.advance_minutes(7 * 60);
    run(privacy.erasure_pass()).unwrap();
    assert!(!w.objects.visible(&key));
    assert_eq!(w.status(&privacy), ErasureView::FullyErased);
}

// F2: the archive object is durable in-flight intent when the PUT starts, the
// completed PUT is acknowledged, and the object stays in the manifest after its
// job row is gone.
#[test]
fn the_archive_intent_precedes_the_put_and_outlives_its_job() {
    let w = World::new(PutMode::Immediate);
    let store = w.store.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in_hook = seen.clone();
    *w.objects.on_put.lock().unwrap() = Some(Box::new(move |object: &ObjectRef| {
        let write = store.with(|d| d.archived.iter().find(|a| &a.object == object && a.account == fixtures::account()).map(|a| a.write));
        seen_in_hook.lock().unwrap().push(write);
    }));
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let job = w.complete(&jobs);
    let key = w.archived_key();
    assert_eq!(*seen.lock().unwrap(), vec![Some(ArchiveWrite::InFlight)], "recorded when the PUT started");
    assert_eq!(w.archive_write(&key), Some(ArchiveWrite::Written));

    w.store.with(|d| {
        d.jobs.remove(&job.0);
        d.lines.retain(|l| l.job != job);
    });
    w.erase(&privacy);
    assert!(!w.objects.visible(&key), "deleted although its job row is gone");
    assert_eq!(w.status(&privacy), ErasureView::FullyErased);
}

// F1: an expired URL for a removed draft still gets its object deleted, and a
// write landing later is swept again; with no upload acknowledgement, a miss
// never closes the saga (pending the lead's decision on upload bounds).
#[test]
fn an_expired_upload_of_a_removed_draft_is_deleted_and_swept_again() {
    let w = World::new(PutMode::Immediate);
    let (jobs, privacy) = (w.jobs(), w.privacy());
    let draft = w.draft(&jobs);
    let access = run(jobs.init_upload(fixtures::account(), draft.id, "talk.mp3")).unwrap();
    let key = access.object.key.clone();
    w.objects.write(&key);
    run(jobs.remove_draft(fixtures::account(), draft.id)).unwrap();
    w.clock.advance_minutes(16);

    let view = w.erase(&privacy);
    assert!(!w.objects.visible(&key), "deleted at once although the draft and its URL are gone");
    assert!(open_with(&view, UPLOAD_OUTCOME_UNKNOWN), "{view:?}");

    w.objects.write(&key);
    w.clock.advance_minutes(7 * 60);
    run(privacy.erasure_pass()).unwrap();
    assert!(!w.objects.visible(&key), "the late write is swept");
    assert!(open_with(&w.status(&privacy), UPLOAD_OUTCOME_UNKNOWN));
}

// F7: the barrier commits while the player URL is being signed; the URL is
// never returned. The same race on the upload URL refuses too.
#[test]
fn no_capability_is_returned_across_the_erasure_barrier() {
    let w = World::new(PutMode::Immediate);
    let jobs = w.jobs();
    let draft = w.draft(&jobs);
    run(jobs.init_upload(fixtures::account(), draft.id, "talk.mp3")).unwrap();
    let store = w.store.clone();
    *w.objects.on_sign.lock().unwrap() = Some(Box::new(move |_: &ObjectRef| World::commit_barrier(&store)));
    let answer = run(jobs.streaming_url(fixtures::account(), draft.id, "talk.mp3"));
    assert!(matches!(answer, Err(JobsError::ErasureInProgress)), "{answer:?}");

    let w = World::new(PutMode::Immediate);
    let jobs = w.jobs();
    let draft = w.draft(&jobs);
    let store = w.store.clone();
    *w.objects.on_sign.lock().unwrap() = Some(Box::new(move |_: &ObjectRef| World::commit_barrier(&store)));
    assert!(run(jobs.init_upload(fixtures::account(), draft.id, "talk.mp3")).is_err());
}
