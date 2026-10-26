use media_ports::ObservationOrigin;

use super::*;
use crate::jobs::domain::JobStatus;
use crate::jobs::testing::{deepgram, soniox, token};
use crate::presentation::fixtures;

// D6/BIL-035: a positive webhook duration wins over token ends; zero falls back.
#[test]
fn soniox_minutes_keep_the_observation_value() {
    let tokens = vec![token("a", 0.0, 400_000.0, None)];
    let webhook = soniox("t", ObservationOrigin::Webhook, "completed", Some(61_000.0), Some(tokens.clone()));
    assert_eq!(provider_minutes(&webhook), Some(2));
    let zero = soniox("t", ObservationOrigin::Webhook, "completed", Some(0.0), Some(tokens.clone()));
    assert_eq!(provider_minutes(&zero), Some(7));
    let polled = soniox("t", ObservationOrigin::RecoveryPoll, "completed", None, Some(tokens));
    assert_eq!(provider_minutes(&polled), Some(7));
    let nothing = soniox("t", ObservationOrigin::Webhook, "completed", None, None);
    assert_eq!(provider_minutes(&nothing), None);
}

// TRX-034: Deepgram seconds rounded up; missing is nothing.
#[test]
fn deepgram_minutes_round_up() {
    assert_eq!(provider_minutes(&deepgram("r", Some(60.01), &[])), Some(2));
    assert_eq!(provider_minutes(&deepgram("r", None, &[])), None);
}

// BIL-034/035/037
#[test]
fn decisions() {
    let mut job = fixtures::job(1, JobStatus::Running, "a.mp3");
    job.duration_minutes = None;
    assert_eq!(decide(&job, Some(5)), DurationDecision::SetAndCharge { minutes: 5 });
    assert_eq!(decide(&job, None), DurationDecision::NoDuration);
    job.duration_minutes = Some(10);
    assert_eq!(decide(&job, Some(10)), DurationDecision::Keep);
    assert_eq!(decide(&job, Some(12)), DurationDecision::Keep);
    assert_eq!(decide(&job, Some(7)), DurationDecision::Lower { minutes: 7 });
}

// The deferred charge resolves once: charged, refused or no duration.
#[test]
fn deferred_state_after_decision() {
    let mut job = fixtures::job(1, JobStatus::Running, "a.mp3");
    job.deferred_charge = DeferredCharge::Pending;
    let set = DurationDecision::SetAndCharge { minutes: 3 };
    assert_eq!(deferred_after(&job, set, Some(true)), DeferredCharge::Charged);
    assert_eq!(deferred_after(&job, set, Some(false)), DeferredCharge::Refused);
    assert_eq!(deferred_after(&job, DurationDecision::NoDuration, None), DeferredCharge::NoDuration);
    job.deferred_charge = DeferredCharge::None;
    assert_eq!(deferred_after(&job, DurationDecision::NoDuration, None), DeferredCharge::None);
}
