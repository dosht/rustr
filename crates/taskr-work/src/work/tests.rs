use media_ports::{ObjectRef, Provider, RemoteIdentity, StoreKind};
use uuid::Uuid;

use super::*;

const SEC: i64 = 1_000_000;
const DAY: i64 = 86_400 * SEC;
const NOW: Micros = Micros(1_800_000_000 * SEC);

fn object() -> ObjectRef {
    ObjectRef { store: StoreKind::R2, bucket: "work".into(), key: "owner/work/clip.mp3".into(), version: Some("v1".into()) }
}

fn remote(provider: Provider, id: &str) -> RemoteIdentity {
    RemoteIdentity { provider, project: "p".into(), id: id.into() }
}

fn uploaded(requested: Option<Provider>) -> Work {
    let mut work = Work::create(Uuid::from_u128(1), Uuid::from_u128(9), object(), "clip.mp3".into(), Some("en".into()), requested, NOW);
    work.uploaded(NOW).unwrap();
    work
}

fn policy() -> Policy {
    Policy::default()
}

fn claim(work: &mut Work, provider: Provider, at: Micros) -> Uuid {
    let id = Uuid::new_v4();
    work.claim(provider, id, at, &policy()).unwrap();
    id
}

#[test]
fn a_work_moves_from_upload_through_one_accepted_attempt_to_a_completed_transcript() {
    let mut work = Work::create(Uuid::from_u128(1), Uuid::from_u128(9), object(), "clip.mp3".into(), None, None, NOW);
    assert_eq!(work.status, Status::AwaitingUpload);
    assert_eq!(work.claim(Provider::Soniox, Uuid::new_v4(), NOW, &policy()), Err(WorkError::NotReady), "nothing is submitted before the upload exists");
    work.uploaded(NOW).unwrap();
    assert_eq!(work.status, Status::Uploaded);

    let attempt = claim(&mut work, Provider::Soniox, NOW);
    assert_eq!(work.status, Status::Pending);
    work.accepted(attempt, remote(Provider::Soniox, "r1"), NOW).unwrap();
    assert_eq!(work.status, Status::Running);
    assert_eq!(work.complete(attempt, "hello".into(), NOW), Ok(Applied::Changed));
    assert_eq!((work.status, work.transcript.as_deref(), work.completed_at), (Status::Completed, Some("hello"), Some(NOW)));
}

#[test]
fn the_requested_provider_goes_first_then_soniox_then_deepgram_and_never_a_tried_one() {
    let mut work = uploaded(Some(Provider::Deepgram));
    let both = [Provider::Soniox, Provider::Deepgram];
    assert_eq!(work.candidates(&both), vec![Provider::Deepgram, Provider::Soniox]);
    assert_eq!(uploaded(None).candidates(&both), vec![Provider::Soniox, Provider::Deepgram]);
    // A requested provider that is not configured is skipped, not an error.
    assert_eq!(work.candidates(&[Provider::Soniox]), vec![Provider::Soniox]);

    let first = claim(&mut work, Provider::Deepgram, NOW);
    work.rejected(first, "bad_request".into(), NOW).unwrap();
    assert_eq!(work.candidates(&both), vec![Provider::Soniox], "a rejected provider is not tried again for this work");
}

#[test]
fn a_rejection_falls_back_to_the_other_provider_and_only_the_last_one_fails_the_work() {
    let both = [Provider::Soniox, Provider::Deepgram];
    let mut work = uploaded(None);
    let first = claim(&mut work, Provider::Soniox, NOW);
    assert_eq!(work.rejected(first, "overloaded".into(), NOW), Ok(Applied::Changed));
    assert_eq!(work.status, Status::Pending, "another provider remains: still pending, not failed");
    assert!(work.candidates(&both).contains(&Provider::Deepgram));

    let second = claim(&mut work, Provider::Deepgram, NOW);
    work.rejected(second, "unsupported_language".into(), NOW).unwrap();
    assert_eq!(work.status, Status::Failed);
    assert_eq!(work.error.as_deref(), Some("all_providers_rejected"));
    assert!(work.candidates(&both).is_empty());
    assert_eq!(work.claim(Provider::Soniox, Uuid::new_v4(), NOW, &policy()), Err(WorkError::NotReady), "a failed work claims nothing more");
}

#[test]
fn only_one_attempt_is_live_and_a_superseded_attempts_answers_change_nothing() {
    let mut work = uploaded(None);
    let first = claim(&mut work, Provider::Soniox, NOW);
    assert_eq!(work.claim(Provider::Deepgram, Uuid::new_v4(), NOW, &policy()), Err(WorkError::Busy), "the first attempt still holds its lease");

    // The lease runs out: the host reconciled and found no match, so the attempt is superseded.
    let later = Micros(NOW.0 + policy().lease_micros + SEC);
    assert!(work.expired(later).is_some());
    work.supersede(first, later).unwrap();
    let second = claim(&mut work, Provider::Deepgram, later);
    work.accepted(second, remote(Provider::Deepgram, "d1"), later).unwrap();

    let before = work.clone();
    assert_eq!(work.accepted(first, remote(Provider::Soniox, "late"), later), Err(WorkError::Stale));
    assert_eq!(work.rejected(first, "late".into(), later), Err(WorkError::Stale));
    assert_eq!(work.complete(first, "stale text".into(), later), Err(WorkError::Stale));
    assert_eq!(work, before, "a stale answer changes nothing");
    assert_eq!(work.complete(second, "fresh text".into(), later), Ok(Applied::Changed));
}

#[test]
fn an_unanswered_submission_is_uncertain_until_its_lease_ends_and_never_a_rejection() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.uncertain(attempt).unwrap();
    assert_eq!(work.status, Status::Pending);
    assert!(work.expired(NOW).is_none(), "inside the lease nothing is reclaimed");
    let expired = work.expired(Micros(NOW.0 + policy().lease_micros)).expect("the lease has ended");
    assert_eq!((expired.id, expired.phase), (attempt, AttemptPhase::Uncertain));
    // Uncertain does not spend the provider: fallback only opens once the attempt is superseded.
    assert!(work.candidates(&[Provider::Soniox, Provider::Deepgram]).contains(&Provider::Soniox));
    // A late acknowledgement of the same, still current attempt is adopted.
    work.accepted(attempt, remote(Provider::Soniox, "late-ack"), Micros(NOW.0 + 1)).unwrap();
    assert_eq!(work.status, Status::Running);
}

#[test]
fn completion_is_applied_once_a_late_failure_never_undoes_it_and_only_an_identical_replay_is_a_no_op() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(attempt, remote(Provider::Soniox, "r"), NOW).unwrap();
    work.complete(attempt, "text".into(), NOW).unwrap();
    assert_eq!(work.complete(attempt, "text".into(), Micros(NOW.0 + 5)), Ok(Applied::Unchanged), "the same result again");
    // The same attempt reporting a different result is a conflict, held and never applied.
    let applied = work.clone();
    assert_eq!(work.complete(attempt, "other text".into(), Micros(NOW.0 + 5)), Err(WorkError::PayloadConflict));
    assert_eq!(work, applied, "the first application stays exactly as it was");
    assert_eq!(work.fail(attempt, "late_error".into(), NOW), Ok(Applied::Unchanged));
    assert_eq!(work.status, Status::Completed);

    // After retention removed the transcript there is nothing left to compare against.
    work.purge_transcript();
    assert_eq!(work.complete(attempt, "text".into(), NOW), Ok(Applied::Unchanged));
    assert_eq!(work.transcript, None, "a replay never brings a purged transcript back");
}

#[test]
fn a_running_work_without_progress_is_stuck_after_the_policy_window() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(attempt, remote(Provider::Soniox, "r"), NOW).unwrap();
    assert!(work.stuck(Micros(NOW.0 + policy().stuck_after_micros - 1), &policy()).is_none());
    let stuck = work.stuck(Micros(NOW.0 + policy().stuck_after_micros), &policy()).unwrap();
    assert_eq!(stuck.id, attempt);
    // A stuck attempt is superseded to try again, keeping its remote identity for erasure.
    work.supersede(attempt, NOW).unwrap();
    assert_eq!(work.status, Status::Pending);
    assert!(work.resources().remotes.contains(&remote(Provider::Soniox, "r")));
}

#[test]
fn retention_removes_the_transcript_of_completed_work_at_thirty_one_days_and_keeps_everything_else() {
    let mut completed = uploaded(None);
    let attempt = claim(&mut completed, Provider::Soniox, NOW);
    completed.accepted(attempt, remote(Provider::Soniox, "r"), NOW).unwrap();
    completed.complete(attempt, "text".into(), NOW).unwrap();

    let at = |days: i64| Micros(NOW.0 + days * DAY);
    assert!(!completed.retention_due(at(30), 30), "exactly thirty days: kept");
    assert!(completed.retention_due(at(31), 30), "thirty-one days: due");
    assert!(!completed.retention_due(at(31), 365), "a longer setting keeps it");

    // Only completed work with a transcript is ever due.
    assert!(!uploaded(None).retention_due(at(400), 30));
    let mut failed = uploaded(None);
    let a = claim(&mut failed, Provider::Soniox, NOW);
    failed.rejected(a, "x".into(), NOW).unwrap();
    let b = claim(&mut failed, Provider::Deepgram, NOW);
    failed.rejected(b, "x".into(), NOW).unwrap();
    assert!(!failed.retention_due(at(400), 30));

    assert_eq!(completed.purge_transcript(), Applied::Changed);
    assert_eq!(completed.transcript, None);
    assert_eq!(completed.status, Status::Completed, "the work row stays, only the transcript goes");
    assert!(!completed.retention_due(at(400), 30));
    assert_eq!(completed.purge_transcript(), Applied::Unchanged);
}

#[test]
fn the_erasure_manifest_lists_the_object_and_every_remote_identity_ever_bound() {
    let mut work = uploaded(None);
    let first = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(first, remote(Provider::Soniox, "s1"), NOW).unwrap();
    work.supersede(first, NOW).unwrap();
    let second = claim(&mut work, Provider::Deepgram, NOW);
    work.accepted(second, remote(Provider::Deepgram, "d1"), NOW).unwrap();
    let third_uncertain = {
        // A claim whose answer was lost has no identity, but the manifest says so instead of hiding it.
        let mut other = uploaded(None);
        let attempt = claim(&mut other, Provider::Soniox, NOW);
        other.uncertain(attempt).unwrap();
        other.resources()
    };
    let resources = work.resources();
    assert_eq!(resources.object, object());
    assert_eq!(resources.remotes, vec![remote(Provider::Soniox, "s1"), remote(Provider::Deepgram, "d1")]);
    assert_eq!(resources.unidentified_attempts, 0);
    assert_eq!(third_uncertain.unidentified_attempts, 1, "a lost answer is an unresolved resource, not an absent one");
}

#[test]
fn a_pending_work_with_no_configured_provider_left_fails_instead_of_waiting_forever() {
    let mut work = uploaded(None);
    let first = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(first, remote(Provider::Soniox, "s1"), NOW).unwrap();
    work.supersede(first, NOW).unwrap();
    // Only Soniox is configured and it was given up: nothing can ever claim this work again.
    assert_eq!(work.exhausted(&[Provider::Soniox, Provider::Deepgram]), Applied::Unchanged, "Deepgram is still available");
    assert_eq!(work.exhausted(&[Provider::Soniox]), Applied::Changed);
    assert_eq!((work.status, work.error.as_deref()), (Status::Failed, Some("providers_exhausted")));
    assert_eq!(work.exhausted(&[Provider::Soniox]), Applied::Unchanged, "a failed work is not failed twice");
    assert!(work.resources().remotes.contains(&remote(Provider::Soniox, "s1")));
}

#[test]
fn an_observed_identity_binds_once_and_a_different_one_is_refused_without_changing_anything() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.uncertain(attempt).unwrap();

    assert_eq!(work.observed(attempt, remote(Provider::Soniox, "a"), NOW), Ok(Applied::Changed), "the first authenticated identity is adopted");
    assert_eq!((work.status, work.attempts[0].remote.clone()), (Status::Running, Some(remote(Provider::Soniox, "a"))));

    let bound = work.clone();
    assert_eq!(work.observed(attempt, remote(Provider::Soniox, "a"), NOW), Ok(Applied::Unchanged), "the same identity again is a repeat");
    assert_eq!(work.observed(attempt, remote(Provider::Soniox, "b"), NOW), Err(WorkError::IdentityMismatch), "another identity never replaces the bound one");
    assert_eq!(work, bound);
    assert_eq!(work.observed(Uuid::new_v4(), remote(Provider::Soniox, "a"), NOW), Err(WorkError::Stale), "an unknown attempt is stale");
}

#[test]
fn a_late_identity_for_a_given_up_attempt_is_recorded_for_erasure_without_reviving_it() {
    let mut work = uploaded(Some(Provider::Deepgram));
    let first = claim(&mut work, Provider::Deepgram, NOW);
    work.uncertain(first).unwrap();
    work.supersede(first, NOW).unwrap();
    let second = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(second, remote(Provider::Soniox, "s1"), NOW).unwrap();

    let late = remote(Provider::Deepgram, "late");
    assert_eq!(work.observed(first, late.clone(), NOW), Ok(Applied::Changed), "the identity is learned");
    assert_eq!((work.attempts[0].phase, work.attempts[1].phase, work.status), (AttemptPhase::Superseded, AttemptPhase::Accepted, Status::Running), "nothing is revived");
    assert!(work.resources().remotes.contains(&late));
    assert_eq!(work.resources().unidentified_attempts, 0);

    let bound = work.clone();
    assert_eq!(work.observed(first, late, NOW), Ok(Applied::Unchanged));
    assert_eq!(work.observed(first, remote(Provider::Deepgram, "other"), NOW), Err(WorkError::IdentityMismatch), "a bound identity is never swapped, finished attempt or not");
    assert_eq!(work.observed(Uuid::new_v4(), remote(Provider::Deepgram, "x"), NOW), Err(WorkError::Stale), "an attempt this work never made");
    assert_eq!(work, bound);
}

#[test]
fn the_identity_is_compared_before_a_finished_attempt_is_called_a_duplicate() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(attempt, remote(Provider::Soniox, "a"), NOW).unwrap();
    work.complete(attempt, "text".into(), NOW).unwrap();
    // The attempt is Done, so it is no longer live; a different identity still conflicts.
    assert_eq!(work.observed(attempt, remote(Provider::Soniox, "b"), NOW), Err(WorkError::IdentityMismatch));
    assert_eq!(work.observed(attempt, remote(Provider::Soniox, "a"), NOW), Ok(Applied::Unchanged));
}

#[test]
fn the_first_result_digest_binds_and_a_different_payload_for_the_same_attempt_is_a_conflict() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(attempt, remote(Provider::Soniox, "a"), NOW).unwrap();
    work.complete(attempt, "text".into(), NOW).unwrap();

    assert_eq!(work.record_digest(attempt, "d1"), Ok(Applied::Changed));
    let bound = work.clone();
    assert_eq!(work.record_digest(attempt, "d1"), Ok(Applied::Unchanged), "the same delivery again");
    assert_eq!(work.record_digest(attempt, "d2"), Err(WorkError::PayloadConflict), "the same identity with another payload is held, never applied");
    assert_eq!(work.record_digest(Uuid::new_v4(), "d1"), Err(WorkError::Stale));
    assert_eq!(work, bound, "a conflict changes nothing");
}

#[test]
fn a_bound_digest_is_checked_on_its_own_before_any_lifecycle_decision() {
    let mut work = uploaded(None);
    let attempt = claim(&mut work, Provider::Soniox, NOW);
    work.accepted(attempt, remote(Provider::Soniox, "a"), NOW).unwrap();
    assert_eq!(work.check_digest(attempt, "f1"), Ok(()), "nothing is bound yet");
    work.fail(attempt, "bad audio".into(), NOW).unwrap();
    work.record_digest(attempt, "f1").unwrap();

    // The attempt is Failed, so `fail` and `complete` would answer Stale: the digest says more.
    assert_eq!(work.check_digest(attempt, "f1"), Ok(()), "the same delivery");
    assert_eq!(work.check_digest(attempt, "f2"), Err(WorkError::PayloadConflict), "a different failure");
    assert_eq!(work.fail(attempt, "other".into(), NOW), Err(WorkError::Stale));
    assert_eq!(work.check_digest(Uuid::new_v4(), "f1"), Err(WorkError::Stale));
}
