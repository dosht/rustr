use std::collections::BTreeMap;
use std::sync::Mutex;

use billing_core::ledger::Package;
use billing_core::model::NewUsage;
use billing_core::types::BoxFuture;
use media_ports::{Submission, VideoMetadataSource};

use super::*;
use crate::export::ExportLabels;
use crate::ids::{LineId, MICROS_PER_DAY};
use crate::jobs::domain::DeferredCharge;
use crate::jobs::ports::ArchiveWrite;
use crate::jobs::testing::{self, CountingLimiter, FakeObjects, FakeTranscriber, MemStore, NoSleep, TestClock};
use crate::presentation::fixtures;
use crate::privacy::erasure::{ErasureRecord, ErasureState};
use crate::transcript::TranslationStatus;

const NOW: Micros = Micros(1_800_000_000_000_000);

struct Metadata {
    answer: Mutex<Result<VideoMetadata, media_ports::CaptionError>>,
}

impl VideoMetadataSource for Metadata {
    fn fetch<'a>(&'a self, _video_id: &'a str) -> BoxFuture<'a, Result<VideoMetadata, media_ports::CaptionError>> {
        let v = self.answer.lock().unwrap().clone();
        Box::pin(async move { v })
    }
}

struct World {
    store: MemStore,
    soniox: Arc<FakeTranscriber>,
    deepgram: Arc<FakeTranscriber>,
    objects: Arc<FakeObjects>,
    metadata: Arc<Metadata>,
    clock: Arc<TestClock>,
}

impl World {
    fn new() -> World {
        let w = World {
            store: MemStore::default(),
            soniox: Arc::new(FakeTranscriber::new(media_ports::Provider::Soniox)),
            deepgram: Arc::new(FakeTranscriber::new(media_ports::Provider::Deepgram)),
            objects: Arc::new(FakeObjects::default()),
            metadata: Arc::new(Metadata { answer: Mutex::new(Err(media_ports::CaptionError::VideoUnavailable)) }),
            clock: Arc::new(TestClock(Mutex::new(NOW))),
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
        self.jobs_with(vec![self.objects.clone()], |_| {})
    }

    fn jobs_with(&self, objects: Vec<Arc<FakeObjects>>, configure: impl FnOnce(&mut JobsConfig)) -> Jobs {
        let mut config = JobsConfig {
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
            freemium_lists: FreemiumLists::default(),
        };
        configure(&mut config);
        Jobs::new(
            JobsServices {
                store: Arc::new(self.store.clone()),
                clock: self.clock.clone(),
                objects: objects.into_iter().map(|o| o as Arc<dyn ObjectStore>).collect(),
                transcribers: vec![self.soniox.clone(), self.deepgram.clone()],
                stager: None,
                sleeper: Arc::new(NoSleep),
                rate_limiter: Arc::new(CountingLimiter { hits: Mutex::new(BTreeMap::new()) }),
                paid_captions: None,
                video_metadata: Some(self.metadata.clone()),
            },
            config,
        )
    }

    fn draft(&self, jobs: &Jobs, minutes: Option<i32>, language: &str) -> Job {
        let draft = NewDraft {
            name: "talk.mp3".into(),
            size_bytes: Some(10),
            language: Language::from_name(language),
            speakers: false,
            duration_minutes: minutes,
        };
        run(jobs.create_draft(fixtures::account(), draft)).unwrap()
    }

    fn usage(&self) -> i64 {
        self.store.with(|d| d.usage.iter().map(|u| u.minutes).sum())
    }

    fn job(&self, id: JobId) -> Job {
        self.store.with(|d| d.jobs[&id.0].clone())
    }

    fn attempt_of(&self, id: JobId) -> Attempt {
        let job = self.job(id);
        self.store.with(|d| d.attempts[&job.current_attempt.unwrap()].clone())
    }
}

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

fn start(jobs: &Jobs, ids: Vec<JobId>) -> Result<StartAnswer, JobsError> {
    run(jobs.start(fixtures::account(), StartRequest { job_ids: ids, locale: None, preferences: Preferences::default() }))
}

// JOB-004/014, BIL-025, TRX-015, D11: one start charges, claims and submits; the
// accepted job runs with the accepting provider and its remote id.
#[test]
fn start_charges_and_hands_over() {
    let w = World::new();
    let jobs = w.jobs();
    let job = w.draft(&jobs, Some(12), "English");
    w.soniox.accept("tx-1");
    let answer = start(&jobs, vec![job.id]).unwrap();
    assert_eq!(answer.results[0].status, JobStatus::Running);
    assert_eq!(answer.meta.new_balance, 88);
    assert_eq!(w.usage(), 12);
    let stored = w.job(job.id);
    assert_eq!((stored.provider, stored.soniox_transcription_id.as_deref()), (Some(Provider::Soniox), Some("tx-1")));
    let request = w.soniox.submitted.lock().unwrap()[0].clone();
    assert_eq!((request.model.as_str(), request.client_reference.as_str(), request.language.as_deref()), ("stt-async-v5", "job_1", Some("en")));
    let attempt = w.attempt_of(job.id);
    assert_eq!(attempt.state, AttemptState::Accepted);
    assert!(request.callback.url.expose().contains(&format!("attempt={}", attempt.id)));
}

// JOB-006, BIL-039: a negative balance refuses the whole batch; nothing moves.
#[test]
fn negative_balance_refuses_all_drafts() {
    let w = World::new();
    w.store.with(|d| {
        d.usage.push(NewUsage { account: fixtures::account(), package_id: 1, subject: Some(99), minutes: 101, used_at: NOW });
    });
    let jobs = w.jobs();
    let a = w.draft(&jobs, Some(3), "English");
    let b = w.draft(&jobs, Some(4), "English");
    assert!(matches!(start(&jobs, vec![a.id, b.id]), Err(JobsError::InsufficientBalance(_))));
    assert_eq!((w.job(a.id).status, w.job(b.id).status), (JobStatus::Draft, JobStatus::Draft));
    assert!(w.soniox.submitted.lock().unwrap().is_empty());
}

// TRX-014, JOB-015: a rate limit fails over to the next compatible provider.
#[test]
fn rate_limit_fails_over() {
    let w = World::new();
    let jobs = w.jobs();
    let job = w.draft(&jobs, Some(5), "English");
    w.soniox.answer(Ok(Submission::Rejected { status: Some(429), code: "Too Many Requests".into() }));
    w.deepgram.accept("dg-1");
    let answer = start(&jobs, vec![job.id]).unwrap();
    assert_eq!(answer.results[0].status, JobStatus::Running);
    let stored = w.job(job.id);
    assert_eq!((stored.provider, stored.retry_count), (Some(Provider::Deepgram), 1));
}

// Seam 6: a lost answer is uncertain, never a failover that could double-submit.
#[test]
fn uncertain_submission_does_not_fail_over() {
    let w = World::new();
    let jobs = w.jobs();
    let job = w.draft(&jobs, Some(5), "English");
    w.soniox.answer(Ok(Submission::Uncertain { status: None, operation: "submit".into() }));
    let answer = start(&jobs, vec![job.id]).unwrap();
    assert_eq!(answer.results[0].status, JobStatus::NotStarted);
    assert_eq!(w.attempt_of(job.id).state, AttemptState::Uncertain);
    assert!(w.deepgram.submitted.lock().unwrap().is_empty());
}

fn accepted_soniox_job(w: &World, jobs: &Jobs, minutes: Option<i32>) -> Job {
    let job = w.draft(jobs, minutes, "English");
    w.soniox.accept("tx-1");
    start(jobs, vec![job.id]).unwrap();
    w.job(job.id)
}

fn webhook(duration_ms: Option<f64>) -> ProviderObservation {
    testing::soniox("tx-1", ObservationOrigin::Webhook, "completed", duration_ms, None)
}

// D6, BIL-037, JOB-028/030: the webhook duration lowers stored minutes without a refund;
// lines land and the job completes once, whatever the redeliveries.
#[test]
fn callback_completes_once() {
    let w = World::new();
    let jobs = w.jobs();
    let job = accepted_soniox_job(&w, &jobs, Some(12));
    *w.soniox.completed.lock().unwrap() = Some(testing::soniox(
        "tx-1",
        ObservationOrigin::Webhook,
        "completed",
        None,
        Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
    ));
    let attempt = w.attempt_of(job.id);
    let obs = webhook(Some(300_000.0));
    let ack = run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap();
    let IntakeAck::Stored { inbox } = ack else { panic!("{ack:?}") };
    run(jobs.ingest(inbox)).unwrap();
    let done = w.job(job.id);
    assert_eq!((done.status, done.duration_minutes), (JobStatus::Completed, Some(5)));
    assert_eq!(w.usage(), 12);
    assert_eq!(w.store.with(|d| d.lines.len()), 1);
    // The same delivery again: acknowledged, nothing applied twice.
    assert!(matches!(run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap(), IntakeAck::Duplicate { .. }));
    run(jobs.ingest(inbox)).unwrap();
    assert_eq!(w.store.with(|d| d.lines.len()), 1);
    // F6: a different payload under the same attempt and remote id is held, never applied,
    // and kept once however often it is redelivered.
    let other = webhook(Some(60_000.0));
    for _ in 0..2 {
        assert!(matches!(run(jobs.intake(&attempt, other.clone(), ingest::fingerprint(&other))).unwrap(), IntakeAck::Conflict { .. }));
    }
    let held: Vec<_> = w.store.with(|d| d.inbox.values().filter(|e| e.state == InboxState::Conflict).cloned().collect());
    assert_eq!(held.len(), 1);
    assert_eq!((held[0].entry.fingerprint.clone(), held[0].entry.observation.clone()), (ingest::fingerprint(&other), other));
    run(jobs.ingest_pass()).unwrap();
    assert_eq!((w.job(job.id).duration_minutes, w.store.with(|d| d.lines.len())), (Some(5), 1), "the held body is never applied");
}

// BIL-034/028: an unknown duration is charged exactly once from the provider's length.
#[test]
fn deferred_charge_from_the_result() {
    let w = World::new();
    let jobs = w.jobs();
    let job = accepted_soniox_job(&w, &jobs, None);
    assert_eq!(w.usage(), 0);
    *w.soniox.completed.lock().unwrap() = Some(testing::soniox(
        "tx-1",
        ObservationOrigin::Webhook,
        "completed",
        None,
        Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
    ));
    let attempt = w.attempt_of(job.id);
    let obs = webhook(Some(150_000.0));
    let IntakeAck::Stored { inbox } = run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap() else { panic!() };
    run(jobs.ingest(inbox)).unwrap();
    let done = w.job(job.id);
    assert_eq!((done.duration_minutes, done.deferred_charge), (Some(3), DeferredCharge::Charged));
    assert_eq!(w.usage(), 3);
}

// JOB-D01 (C1): a result after deletion writes no lines and asks for remote cleanup.
#[test]
fn late_result_after_delete_writes_nothing() {
    let w = World::new();
    let jobs = w.jobs();
    let job = accepted_soniox_job(&w, &jobs, Some(2));
    run(jobs.delete_job(fixtures::account(), job.id)).unwrap();
    *w.soniox.completed.lock().unwrap() = Some(testing::soniox(
        "tx-1",
        ObservationOrigin::Webhook,
        "completed",
        None,
        Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
    ));
    let attempt = w.attempt_of(job.id);
    let obs = webhook(Some(60_000.0));
    let IntakeAck::Stored { inbox } = run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap() else { panic!() };
    run(jobs.ingest(inbox)).unwrap();
    assert_eq!(w.job(job.id).status, JobStatus::Deleted);
    assert_eq!(w.store.with(|d| d.lines.len()), 0);
    assert_eq!(w.attempt_of(job.id).remote_cleanup, RemoteCleanup::Pending);
    assert_eq!(w.store.with(|d| d.inbox[&inbox.0].state), InboxState::Ignored);
}

// D7: last write wins on the owner's line; another account's line is not found.
#[test]
fn edits_are_owner_scoped_last_write_wins() {
    let w = World::new();
    let jobs = w.jobs();
    w.store.with(|d| {
        d.jobs.insert(1, fixtures::job(1, JobStatus::Completed, "a.mp3"));
        d.lines.push(fixtures::line(1, 1, "00:00:00", "00:00:01", "old", None));
    });
    let edit = |text: &str| LineEdit::Text { line: LineId(1), text: text.into() };
    assert_eq!(run(jobs.edit(fixtures::account(), edit("first"))).unwrap(), 1);
    assert_eq!(run(jobs.edit(fixtures::account(), edit(""))).unwrap(), 1);
    assert_eq!(w.store.with(|d| d.lines[0].line.clone()), "");
    let stranger = crate::ids::AccountId(uuid::Uuid::from_u128(5));
    w.store.with(|d| {
        d.accounts.insert(stranger);
    });
    assert_eq!(run(jobs.edit(stranger, edit("x"))), Err(JobsError::NotFound));
}

// C-11: every authenticated entry point refuses an account with an erasure barrier.
#[test]
fn erasure_barrier_closes_entry_points() {
    let w = World::new();
    let jobs = w.jobs();
    w.store.with(|d| {
        let id = crate::ids::ErasureId::new();
        d.erasures.insert(
            id,
            ErasureRecord {
                id,
                account: fixtures::account(),
                requested_at: NOW,
                state: ErasureState::Requested,
                local_erased_at: None,
                identity_deleted_at: None,
                closed_at: None,
            },
        );
    });
    let draft = NewDraft { name: "a.mp3".into(), size_bytes: None, language: None, speakers: false, duration_minutes: Some(1) };
    assert_eq!(run(jobs.create_draft(fixtures::account(), draft)), Err(JobsError::ErasureInProgress));
    assert_eq!(run(jobs.settings(fixtures::account())), Err(JobsError::ErasureInProgress));
    assert_eq!(run(jobs.drafts(fixtures::account())), Err(JobsError::ErasureInProgress));
}

// JOB-003 (`getAllDraftJobs`): every own draft, oldest first, outside the history window and page size.
#[test]
fn drafts_restore_every_own_draft_oldest_first() {
    let w = World::new();
    let jobs = w.jobs();
    let stranger = crate::ids::AccountId(uuid::Uuid::from_u128(5));
    w.store.with(|d| {
        for id in 1..=25 {
            let mut j = fixtures::job(id, JobStatus::Draft, "d.mp3");
            j.created_at = Micros(0);
            d.jobs.insert(id, j);
        }
        d.jobs.insert(26, fixtures::job(26, JobStatus::Completed, "c.mp3"));
        let mut theirs = fixtures::job(27, JobStatus::Draft, "t.mp3");
        theirs.account = stranger;
        d.jobs.insert(27, theirs);
    });
    let ids: Vec<i64> = run(jobs.drafts(fixtures::account())).unwrap().into_iter().map(|j| j.id.0).collect();
    assert_eq!(ids, (1..=25).collect::<Vec<_>>());
}

fn labels() -> ExportLabels {
    ExportLabels::default()
}

// EXP-001/013, FRE-022: completed jobs only; the transcript page keeps translation lines.
#[test]
fn export_use_case() {
    let w = World::new();
    let jobs = w.jobs();
    let mut job = fixtures::job(1, JobStatus::Completed, "talk.mp3");
    job.translate_to = Language::from_name("German").into_iter().collect();
    let mut original = fixtures::line(1, 1, "00:00:00", "00:00:02", "Hello", None);
    original.translation_status = Some(TranslationStatus::Original);
    let mut translated = fixtures::line(2, 1, "00:00:00", "00:00:02", "Hallo", None);
    translated.translation_status = Some(TranslationStatus::Translation);
    w.store.with(|d| {
        d.jobs.insert(1, job);
        d.lines.extend([original, translated]);
        d.jobs.insert(2, fixtures::job(2, JobStatus::Running, "b.mp3"));
    });
    let request = |surface| ExportRequest { kind: ExportKind::AllFormats, surface, labels: labels() };
    let (bundle, event) = run(jobs.export(fixtures::account(), JobId(1), request(ExportSurface::TranscriptPage))).unwrap();
    assert_eq!(bundle.entries[0].content, crate::export::EntryContent::Text { text: "Hallo".into() });
    assert_eq!((event.download_format.as_str(), event.word_count), ("all", 1));
    let (bundle, _) = run(jobs.export(fixtures::account(), JobId(1), request(ExportSurface::JobList))).unwrap();
    assert_eq!(bundle.entries[0].content, crate::export::EntryContent::Text { text: "Hello Hallo".into() });
    assert!(matches!(run(jobs.export(fixtures::account(), JobId(2), request(ExportSurface::JobList))), Err(JobsError::WrongState(_))));
    assert!(matches!(run(jobs.export(fixtures::account(), JobId(9), request(ExportSurface::JobList))), Err(JobsError::NotFound)));
}

// RTL-004: twenty metadata lookups per user per minute; unknown videos are 404.
#[test]
fn youtube_metadata_limits_and_errors() {
    let w = World::new();
    let jobs = w.jobs();
    for _ in 0..20 {
        assert_eq!(run(jobs.youtube_metadata(fixtures::account(), "dQw4w9WgXcQ")).err(), Some(JobsError::VideoNotFound));
    }
    assert!(matches!(run(jobs.youtube_metadata(fixtures::account(), "dQw4w9WgXcQ")), Err(JobsError::RateLimited(_))));
    let fresh = World::new();
    let jobs = fresh.jobs();
    assert_eq!(run(jobs.youtube_metadata(fixtures::account(), "")).err(), Some(JobsError::InvalidRequest("videoId is required".into())));
    assert_eq!(
        run(jobs.youtube_metadata(fixtures::account(), "https://example.com/watch")).err(),
        Some(JobsError::InvalidRequest("Invalid YouTube URL or video ID".into()))
    );
    *fresh.metadata.answer.lock().unwrap() = Err(media_ports::CaptionError::Network);
    assert_eq!(run(jobs.youtube_metadata(fixtures::account(), "dQw4w9WgXcQ")).err(), Some(JobsError::Failed("Failed to fetch metadata".into())));
}

// UPL-024: the YouTube draft takes the title, ceil-free rounded minutes and the declared language.
#[test]
fn youtube_add_creates_the_draft() {
    let w = World::new();
    let jobs = w.jobs();
    *w.metadata.answer.lock().unwrap() = Ok(VideoMetadata {
        video_id: "dQw4w9WgXcQ".into(),
        title: "Talk".into(),
        duration_seconds: Some(89.0),
        language: Some("tr-TR".into()),
        raw: serde_json::Value::Null,
    });
    let (job, metadata) = run(jobs.youtube_add(fixtures::account(), "https://youtu.be/dQw4w9WgXcQ", None)).unwrap();
    assert_eq!(metadata.title, "Talk");
    assert_eq!((job.name.as_deref(), job.duration_minutes, job.language.map(|l| l.name())), (Some("Talk"), Some(1), Some("Turkish")));
    assert_eq!((job.status, job.size_bytes, job.speakers, job.fix_grammer), (JobStatus::Draft, Some(0), false, Some(true)));
}

// UPL-010/011 as built, erasure inventory: an upload URL belongs to the caller it was signed
// for (its folder), even for another account's or a missing job id; the job's owner never
// inherits it and the foreign draft is not bound to it.
#[test]
fn issued_uploads_belong_to_the_signing_caller() {
    let w = World::new();
    let jobs = w.jobs();
    let ada_job = w.draft(&jobs, Some(1), "English");
    let bob = AccountId(Uuid::from_u128(77));
    w.store.with(|d| {
        d.accounts.insert(bob);
    });
    run(jobs.init_upload(bob, ada_job.id, "x.mp3")).unwrap();
    run(jobs.init_upload(bob, JobId(999), "y.mp3")).unwrap();
    let uploads = w.store.with(|d| d.uploads.clone());
    assert_eq!(uploads.iter().map(|u| u.account).collect::<Vec<_>>(), vec![bob, bob]);
    assert!(uploads.iter().all(|u| u.object.key.starts_with(&format!("userId={bob}/"))));
    assert_eq!(w.job(ada_job.id).source, JobSource::Upload { storage: StorageProvider::Gcs, object: None });
}

// JOB-044 smoke finding: with two adapters of one kind (uploads and archive buckets), the raw
// payload is archived through the adapter bound to the archive bucket, never the first of its kind.
#[test]
fn the_payload_archive_uses_the_archive_buckets_adapter() {
    let w = World::new();
    let uploads = Arc::new(FakeObjects { kind: Some(StoreKind::R2), bucket_name: Some("uploads"), ..FakeObjects::default() });
    let archive = Arc::new(FakeObjects { kind: Some(StoreKind::R2), bucket_name: Some("archive"), ..FakeObjects::default() });
    let jobs = w.jobs_with(vec![uploads.clone(), archive.clone()], |c| {
        c.upload_storage = StorageProvider::Cloudflare;
        c.upload_bucket = "uploads".into();
        c.archive_store = StoreKind::R2;
        c.archive_bucket = "archive".into();
    });
    let job = accepted_soniox_job(&w, &jobs, Some(2));
    *w.soniox.completed.lock().unwrap() = Some(testing::soniox(
        "tx-1",
        ObservationOrigin::Webhook,
        "completed",
        None,
        Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
    ));
    let attempt = w.attempt_of(job.id);
    let obs = webhook(Some(60_000.0));
    let IntakeAck::Stored { inbox } = run(jobs.intake(&attempt, obs.clone(), ingest::fingerprint(&obs))).unwrap() else { panic!() };
    run(jobs.ingest(inbox)).unwrap();
    let recorded = w.store.with(|d| d.archived.clone());
    assert_eq!(recorded.len(), 1);
    assert_eq!((recorded[0].object.bucket.as_str(), recorded[0].write), ("archive", ArchiveWrite::Written));
    let key = recorded[0].object.key.clone();
    assert!(key.starts_with(&format!("{}/{}/", fixtures::account(), job.id.0)) && key.ends_with("/soniox_transcript.json"));
    assert!(archive.present.lock().unwrap().contains(&key), "archived in the archive bucket");
    assert!(uploads.present.lock().unwrap().is_empty(), "nothing written to the upload bucket");
}

fn uncertain_soniox_attempt(w: &World, jobs: &Jobs) -> (Job, Attempt) {
    let job = w.draft(jobs, Some(5), "English");
    w.soniox.answer(Ok(Submission::Uncertain { status: None, operation: "submit".into() }));
    start(jobs, vec![job.id]).unwrap();
    (w.job(job.id), w.attempt_of(job.id))
}

/// A Soniox listing record that agrees with everything `attempt` sent.
fn listed(attempt: &Attempt, id: &str) -> media_ports::RemoteRecord {
    media_ports::RemoteRecord {
        remote: media_ports::RemoteIdentity { provider: media_ports::Provider::Soniox, project: attempt.project.clone(), id: id.into() },
        status: "queued".into(),
        model: attempt.model.clone(),
        client_reference: Some(attempt.request.client_reference.clone()),
        audio_url: Some(SensitiveUrl::new(format!("https://storage.test/{}/{}?sig=1", attempt.request.audio.bucket, attempt.request.audio.key))),
        callback_url: Some(SensitiveUrl::new(format!("https://transgate.test/api/webhooks/soniox?attempt={}", attempt.id))),
        created_at: crate::time::iso8601(attempt.claimed_at),
    }
}

// C5/F5: the listing adopts an attempt only through one record that agrees with everything it
// sent. A loose match (right reference and attempt, other audio) or two exact matches leave it
// unresolved past the stuck window: never superseded, never a second paid submission.
#[test]
fn soniox_reconciliation_adopts_only_one_exact_record() {
    let w = World::new();
    let jobs = w.jobs();
    let (job, attempt) = uncertain_soniox_attempt(&w, &jobs);
    let mut loose = listed(&attempt, "tx-loose");
    loose.audio_url = Some(SensitiveUrl::new("https://storage.test/bucket/other.mp3"));
    *w.soniox.records.lock().unwrap() = vec![loose];
    w.clock.advance_minutes(31);
    let report = run(jobs.reconcile_pass()).unwrap();
    assert_eq!((report.recovered, report.submitted, w.attempt_of(job.id).state), (0, 0, AttemptState::Uncertain));
    *w.soniox.records.lock().unwrap() = vec![listed(&attempt, "tx-a"), listed(&attempt, "tx-b")];
    let report = run(jobs.reconcile_pass()).unwrap();
    assert_eq!((report.recovered, report.submitted, w.attempt_of(job.id).state), (0, 0, AttemptState::Uncertain));
    assert_eq!(w.soniox.submitted.lock().unwrap().len(), 1, "no second paid submission");
    *w.soniox.records.lock().unwrap() = vec![listed(&attempt, "tx-2")];
    assert_eq!(run(jobs.reconcile_pass()).unwrap().recovered, 1);
    let adopted = w.job(job.id);
    assert_eq!((adopted.status, adopted.soniox_transcription_id.as_deref()), (JobStatus::Running, Some("tx-2")));
    assert_eq!(w.attempt_of(job.id).id, attempt.id);
}

// C5/F5: a listing that cannot be read is not a miss: the attempt stays unresolved after the
// stuck window and the pass reports the error.
#[test]
fn a_failed_soniox_listing_is_not_a_miss() {
    let w = World::new();
    let jobs = w.jobs();
    let (job, attempt) = uncertain_soniox_attempt(&w, &jobs);
    *w.soniox.listing_error.lock().unwrap() = Some(media_ports::TransportError::Unavailable { status: Some(503), operation: "list".into() });
    w.clock.advance_minutes(31);
    let report = run(jobs.reconcile_pass()).unwrap();
    assert_eq!((report.errors, report.submitted), (1, 0));
    assert_eq!((w.attempt_of(job.id).id, w.attempt_of(job.id).state), (attempt.id, AttemptState::Uncertain));
    assert_eq!(w.soniox.submitted.lock().unwrap().len(), 1);
}

/// The first claim's lease expired with its POST still in flight, and a second claim
/// was accepted as `tx-2`. Returns the job and the first attempt.
fn second_claim_after_lease_expiry(w: &World, jobs: &Jobs) -> (Job, Attempt) {
    let (job, first) = uncertain_soniox_attempt(w, jobs);
    w.store.with(|d| d.attempts.get_mut(&first.id).unwrap().state = AttemptState::Claimed);
    w.clock.advance_minutes(CLAIM_LEASE_MINUTES + 1);
    w.soniox.accept("tx-2");
    run(jobs.dispatch(job.id, Preferences::default())).unwrap();
    assert_ne!(w.attempt_of(job.id).id, first.id);
    (job, first)
}

// C5/F3a: lease expiry, a second claim, then the first POST answers Accepted: its identity is
// learned on the superseded attempt and that copy is deleted; the job keeps the second one.
#[test]
fn a_late_acceptance_after_a_second_claim_is_learned_and_deleted() {
    let w = World::new();
    let jobs = w.jobs();
    let (job, first) = second_claim_after_lease_expiry(&w, &jobs);
    let answer = run(jobs.acknowledge(&first, SubmitOutcome::Accepted { remote_id: "tx-1".into() })).unwrap();
    assert!(matches!(answer, AttemptResult::Busy));
    let late = w.store.with(|d| d.attempts[&first.id].clone());
    assert_eq!((late.state, late.remote_id.as_deref(), late.remote_cleanup), (AttemptState::Superseded, Some("tx-1"), RemoteCleanup::Pending));
    let live = w.job(job.id);
    assert_eq!((live.status, live.soniox_transcription_id.as_deref()), (JobStatus::Running, Some("tx-2")));
    run(jobs.delete_remote_copy(first.id)).unwrap();
    assert_eq!(w.soniox.deleted.lock().unwrap().iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec!["tx-1".to_string()]);
}

// C5/F3b: a callback for the superseded attempt is ignored, but the identity it names is
// learned, so remote cleanup deletes that copy instead of finding no remote id.
#[test]
fn a_callback_for_a_superseded_attempt_names_the_copy_to_delete() {
    let w = World::new();
    let jobs = w.jobs();
    let (job, first) = second_claim_after_lease_expiry(&w, &jobs);
    *w.soniox.completed.lock().unwrap() = Some(testing::soniox(
        "tx-1",
        ObservationOrigin::Webhook,
        "completed",
        None,
        Some(vec![testing::token("Hello there everyone.", 0.0, 2000.0, None)]),
    ));
    let obs = webhook(Some(60_000.0));
    let IntakeAck::Stored { inbox } = run(jobs.intake(&first, obs.clone(), ingest::fingerprint(&obs))).unwrap() else { panic!() };
    run(jobs.ingest(inbox)).unwrap();
    let late = w.store.with(|d| d.attempts[&first.id].clone());
    assert_eq!((late.remote_id.as_deref(), late.remote_cleanup), (Some("tx-1"), RemoteCleanup::Pending));
    assert_eq!(w.store.with(|d| d.inbox[&inbox.0].state), InboxState::Ignored);
    assert_eq!((w.job(job.id).status, w.store.with(|d| d.lines.len())), (JobStatus::Running, 0));
    run(jobs.delete_remote_copy(first.id)).unwrap();
    assert_eq!(w.soniox.deleted.lock().unwrap().iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec!["tx-1".to_string()]);
}

// C10/F3c: an acceptance answered after the erasure closed and the cascade removed the attempt
// becomes an erasure item of its own and reopens the erasure, once however often it repeats.
#[test]
fn a_late_acceptance_after_the_erasure_closed_reopens_it() {
    let w = World::new();
    let jobs = w.jobs();
    let (_job, attempt) = uncertain_soniox_attempt(&w, &jobs);
    let erasure = crate::ids::ErasureId::new();
    w.store.with(|d| {
        d.attempts.clear();
        d.erasures.insert(
            erasure,
            ErasureRecord {
                id: erasure,
                account: fixtures::account(),
                requested_at: NOW,
                state: ErasureState::FullyErased,
                local_erased_at: Some(NOW),
                identity_deleted_at: Some(NOW),
                closed_at: Some(NOW),
            },
        );
    });
    for _ in 0..2 {
        let answer = run(jobs.acknowledge(&attempt, SubmitOutcome::Accepted { remote_id: "tx-1".into() })).unwrap();
        assert!(matches!(answer, AttemptResult::Busy));
    }
    let (record, items) = w.store.with(|d| (d.erasures[&erasure].clone(), d.items.clone()));
    assert_eq!((record.state, record.closed_at), (ErasureState::LocalErased, None));
    let remote = media_ports::RemoteIdentity { provider: media_ports::Provider::Soniox, project: attempt.project.clone(), id: "tx-1".into() };
    assert_eq!(items.iter().map(|i| i.target.clone()).collect::<Vec<_>>(), vec![ItemTarget::ProviderCopy { provider: attempt.provider, remote }]);
    assert_eq!(items[0].state, ItemState::Pending);
}

// Decision (no-listing fallback): an unanswered Deepgram submission is superseded after the
// 30-minute stuck window and dispatched again, knowingly risking duplicate remote work. It never
// charges twice, and the first submission's late identity is still learned for cleanup.
#[test]
fn a_deepgram_submission_without_answer_is_redispatched_once_without_a_second_charge() {
    let w = World::new();
    let jobs = w.jobs();
    let job = w.draft(&jobs, Some(5), "English");
    w.soniox.answer(Ok(Submission::Rejected { status: Some(429), code: "Too Many Requests".into() }));
    w.deepgram.answer(Ok(Submission::Uncertain { status: None, operation: "submit".into() }));
    start(&jobs, vec![job.id]).unwrap();
    let first = w.attempt_of(job.id);
    assert_eq!((first.provider, first.state), (Provider::Deepgram, AttemptState::Uncertain));
    let charged = w.usage();
    w.clock.advance_minutes(29);
    assert_eq!(run(jobs.reconcile_pass()).unwrap().submitted, 0, "inside the stuck window nothing moves");
    w.clock.advance_minutes(2);
    w.soniox.answer(Ok(Submission::Rejected { status: Some(429), code: "Too Many Requests".into() }));
    w.deepgram.accept("dg-2");
    assert_eq!(run(jobs.reconcile_pass()).unwrap().submitted, 1);
    let second = w.attempt_of(job.id);
    assert_ne!(second.id, first.id);
    assert_eq!((w.job(job.id).status, second.remote_id.as_deref()), (JobStatus::Running, Some("dg-2")));
    assert_eq!(w.store.with(|d| d.attempts[&first.id].state), AttemptState::Superseded);
    assert_eq!(w.usage(), charged, "no second local charge");
    assert!(matches!(run(jobs.acknowledge(&first, SubmitOutcome::Accepted { remote_id: "dg-1".into() })).unwrap(), AttemptResult::Busy));
    let late = w.store.with(|d| d.attempts[&first.id].clone());
    assert_eq!((late.remote_id.as_deref(), late.remote_cleanup), (Some("dg-1"), RemoteCleanup::Pending));
    assert_eq!((w.job(job.id).provider, w.attempt_of(job.id).id), (Some(Provider::Deepgram), second.id), "the job keeps the second");
}
