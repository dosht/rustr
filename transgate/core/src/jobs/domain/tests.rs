use media_ports::{ObjectRef, SecretString, StoreKind};

use super::*;
use crate::presentation::fixtures;

fn attempt(job: &Job, provider: Provider, now: Micros) -> Attempt {
    Attempt {
        id: AttemptId::new(),
        job: job.id,
        account: job.account,
        number: job.attempts_made + 1,
        provider,
        project: "project".into(),
        model: "m".into(),
        state: AttemptState::Claimed,
        claimed_at: now,
        lease_until: Micros(now.0 + CLAIM_LEASE_MINUTES * MICROS_PER_MINUTE),
        request: AttemptRequest {
            language_code: None,
            translation_target: None,
            diarize: false,
            audio: ObjectRef { store: StoreKind::Gcs, bucket: "b".into(), key: "k".into(), version: None },
            client_reference: client_reference(job.id),
        },
        callback_secret: SecretString::new("s"),
        remote_id: None,
        accepted_at: None,
        last_error: None,
        remote_cleanup: RemoteCleanup::NotRequested,
    }
}

const NOW: Micros = Micros(1_800_000_000_000_000);

// JOB-001: twelve values, all round-trip by their stored text.
#[test]
fn twelve_statuses_round_trip() {
    assert_eq!(JobStatus::ALL.len(), 12);
    for s in JobStatus::ALL {
        assert_eq!(JobStatus::parse(s.as_str()), Some(s));
    }
    for legacy in ["uploaded", "processing", "ready", "speaker_labeling"] {
        assert!(JobStatus::parse(legacy).is_some());
    }
}

// JOB-036/PRV-012: the list allow-list excludes drafts, deleted, canceled and legacy states.
#[test]
fn listed_statuses_are_the_source_allow_list() {
    let listed: Vec<&str> = JobStatus::ALL.iter().filter(|s| s.is_listed()).map(|s| s.as_str()).collect();
    assert_eq!(listed, ["not_started", "pending", "running", "completed", "failed", "speaker_labeling"]);
}

// JOB-004: only drafts start; NULL duration defers the charge (BIL-028).
#[test]
fn start_moves_only_drafts() {
    let mut job = fixtures::job(1, JobStatus::Draft, "a.mp3");
    job.duration_minutes = None;
    job.start(NOW).unwrap();
    assert_eq!(job.status, JobStatus::NotStarted);
    assert_eq!(job.deferred_charge, DeferredCharge::Pending);
    assert!(job.start(NOW).is_err());
    for s in [JobStatus::Ready, JobStatus::Uploaded, JobStatus::Completed] {
        let mut j = fixtures::job(2, s, "a.mp3");
        assert!(j.start(NOW).is_err(), "{s}");
    }
}

// JOB-005: options change only on drafts.
#[test]
fn draft_options_are_draft_only() {
    let mut job = fixtures::job(1, JobStatus::Draft, "a.mp3");
    job.set_draft_options(Some(None), Some(Language::from_name("German").into_iter().collect()), Some(false), NOW).unwrap();
    assert_eq!(job.language, None);
    assert!(job.is_translation());
    assert!(!job.speakers);
    let mut running = fixtures::job(2, JobStatus::Running, "a.mp3");
    assert!(running.set_draft_options(None, None, Some(true), NOW).is_err());
}

// JOB-D02 correction: a live claim blocks a second claim; an expired one does not.
#[test]
fn a_live_claim_fences_the_job() {
    let mut job = fixtures::job(1, JobStatus::NotStarted, "a.mp3");
    let first = attempt(&job, Provider::Soniox, NOW);
    job.claim(&first, None, NOW).unwrap();
    assert_eq!(job.provider, Some(Provider::Soniox));
    let second = attempt(&job, Provider::Deepgram, NOW);
    assert!(matches!(job.claim(&second, Some(&first), NOW), Err(JobError::AttemptInFlight { .. })));
    let later = Micros(NOW.0 + (CLAIM_LEASE_MINUTES + 1) * MICROS_PER_MINUTE);
    job.claim(&second, Some(&first), later).unwrap();
    assert_eq!(job.current_attempt, Some(second.id));
}

// JOB-014: acceptance sets running, started_at, the accepting provider and the Soniox id.
#[test]
fn acceptance_records_the_accepting_provider() {
    let mut job = fixtures::job(1, JobStatus::Pending, "a.mp3");
    let a = attempt(&job, Provider::Soniox, NOW);
    job.claim(&a, None, NOW).unwrap();
    job.accept(&a, "tx-1", NOW).unwrap();
    assert_eq!(job.status, JobStatus::Running);
    assert_eq!(job.started_at, Some(NOW));
    assert_eq!(job.soniox_transcription_id.as_deref(), Some("tx-1"));
}

// Seam 6: a stale attempt cannot accept, complete or fail the job.
#[test]
fn stale_attempts_are_fenced() {
    let mut job = fixtures::job(1, JobStatus::NotStarted, "a.mp3");
    let old = attempt(&job, Provider::Deepgram, NOW);
    let new = attempt(&job, Provider::Soniox, NOW);
    job.current_attempt = Some(new.id);
    assert!(matches!(job.accept(&old, "r", NOW), Err(JobError::StaleAttempt { .. })));
    job.status = JobStatus::Running;
    assert!(job.complete(old.id, NOW).is_err());
    assert!(job.fail_result(old.id, json!({}), NOW).is_err());
}

// JOB-015: rate limit from pending, not_started or running; retry count and error.
#[test]
fn rate_limit_moves_to_pending() {
    let mut job = fixtures::job(1, JobStatus::NotStarted, "a.mp3");
    let a = attempt(&job, Provider::Deepgram, NOW);
    job.claim(&a, None, NOW).unwrap();
    job.rate_limited(&a, None, Some(429), Micros(NOW.0 + 1), NOW).unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert_eq!(job.retry_count, 1);
    assert_eq!(job.error.as_ref().unwrap()["message"], "Rate limit exceeded, will retry");
}

// JOB-020/021: stuck after 30 minutes; reset counts a retry and marks stuck recovery.
#[test]
fn stuck_deepgram_resets_to_pending() {
    let mut job = fixtures::job(1, JobStatus::Running, "a.mp3");
    job.started_at = Some(Micros(NOW.0 - 30 * MICROS_PER_MINUTE));
    assert!(!is_stuck(&job, NOW));
    let later = Micros(NOW.0 + 1);
    assert!(is_stuck(&job, later));
    job.reset_stuck_deepgram(later).unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert_eq!(job.retry_count, 1);
    assert_eq!(job.error.as_ref().unwrap()["stuck_recovery"], true);
}

// JOB-024: the recovery claim is a compare-and-swap; release gives it back.
#[test]
fn recovery_claim_is_compare_and_swap() {
    let mut job = fixtures::job(1, JobStatus::Running, "a.mp3");
    job.claim_recovery(0, NOW).unwrap();
    assert!(job.claim_recovery(0, NOW).is_err());
    job.release_recovery(NOW);
    assert_eq!(job.recovery_attempts, 0);
}

// JOB-040: cancel only from not_started, pending or running.
#[test]
fn cancel_requires_an_active_status() {
    for s in [JobStatus::NotStarted, JobStatus::Pending, JobStatus::Running] {
        let mut j = fixtures::job(1, s, "a.mp3");
        j.cancel(NOW).unwrap();
        assert_eq!(j.status, JobStatus::Canceled);
    }
    for s in [JobStatus::Draft, JobStatus::Completed, JobStatus::Failed, JobStatus::Deleted] {
        let mut j = fixtures::job(1, s, "a.mp3");
        assert!(j.cancel(NOW).is_err(), "{s}");
    }
}

// JOB-036/T23: soft delete from any status; repeating is a no-op.
#[test]
fn soft_delete_from_any_status() {
    for s in JobStatus::ALL {
        let mut j = fixtures::job(1, s, "a.mp3");
        assert_eq!(j.soft_delete(NOW), s != JobStatus::Deleted);
        assert_eq!(j.status, JobStatus::Deleted);
    }
}

// JOB-035: YouTube drafts complete without passing not_started.
#[test]
fn youtube_completes_from_draft() {
    let mut j = fixtures::job(1, JobStatus::Draft, "video");
    j.source = JobSource::YouTube { video_id: Some("x".into()), youtube_url: None, transcript_source: None };
    j.complete_youtube("transcriptapi", NOW).unwrap();
    assert_eq!(j.status, JobStatus::Completed);
    assert_eq!(j.deferred_charge, DeferredCharge::None);
    let mut upload = fixtures::job(2, JobStatus::Draft, "a.mp3");
    assert!(upload.complete_youtube("transcriptapi", NOW).is_err());
}

// TRX-003: the qta rule reads the last extension in lower case.
#[test]
fn extension_comes_from_the_job_name() {
    let mut j = fixtures::job(1, JobStatus::Draft, "Recording.QTA");
    assert_eq!(j.file_extension().as_deref(), Some("qta"));
    j.name = Some("noext".into());
    assert_eq!(j.file_extension(), None);
}

// TRX-011..013, LNG-010: capability tables.
#[test]
fn provider_capabilities() {
    let armenian = Language::from_name("Armenian");
    assert!(Provider::Soniox.can_handle(None));
    assert!(!Provider::Deepgram.can_handle(None));
    assert!(!Provider::Soniox.can_handle(armenian));
    assert!(Provider::Deepgram.can_handle(armenian));
    assert!(Provider::Soniox.supports_translation());
    assert!(!Provider::Deepgram.supports_translation());
    assert_eq!(Provider::Runpod.transport(), None);
    assert_eq!(Provider::SonioxV5.transport(), Some(media_ports::Provider::Soniox));
}
