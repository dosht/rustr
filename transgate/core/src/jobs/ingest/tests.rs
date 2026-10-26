use media_ports::ObservationOrigin;

use super::*;
use crate::jobs::testing::{deepgram, soniox, token};
use crate::presentation::fixtures;

fn running(duration: Option<i32>) -> Job {
    let mut j = fixtures::job(1, JobStatus::Running, "a.mp3");
    j.duration_minutes = duration;
    j
}

fn speech() -> Vec<media_ports::SonioxToken> {
    vec![token("Hello there everyone.", 0.0, 2000.0, None)]
}

// JOB-027, JOB-D01 (C1): completed, deleted, canceled and failed jobs ignore results.
#[test]
fn late_results_are_ignored() {
    let obs = soniox("t", ObservationOrigin::Webhook, "completed", Some(1.0), Some(speech()));
    for s in [JobStatus::Completed, JobStatus::Deleted, JobStatus::Canceled, JobStatus::Failed, JobStatus::Draft] {
        let mut j = running(Some(1));
        j.status = s;
        assert!(matches!(plan(&j, &obs), IngestPlan::Ignore { .. }), "{s}");
    }
    let mut pending = running(Some(1));
    pending.status = JobStatus::Pending;
    assert!(matches!(plan(&pending, &obs), IngestPlan::Complete { .. }));
}

// JOB-028: a Soniox error status fails with the provider's message before any duration step.
#[test]
fn soniox_error_fails_first() {
    let mut obs = soniox("t", ObservationOrigin::Webhook, "error", Some(60_000.0), None);
    if let media_ports::ProviderPayload::Soniox { reported, .. } = &mut obs.payload {
        reported.error = Some(serde_json::json!({ "message": "bad audio", "code": "invalid_audio" }));
    }
    match plan(&running(None), &obs) {
        IngestPlan::ProviderFailed { failure } => {
            assert_eq!(failure["message"], "bad audio");
            assert_eq!(failure["code"], "invalid_audio");
            assert_eq!(failure["details"]["transcriptionId"], "t");
        }
        other => panic!("{other:?}"),
    }
}

// JOB-031/TRX-040: no speech still runs the duration rule first (charged when unknown).
#[test]
fn no_speech_keeps_the_duration_step() {
    let obs = soniox("t", ObservationOrigin::Webhook, "completed", Some(120_000.0), Some(vec![]));
    match plan(&running(None), &obs) {
        IngestPlan::NoSegments { duration, failure, archive_name } => {
            assert_eq!(duration, DurationDecision::SetAndCharge { minutes: 2 });
            assert_eq!(failure["code"], "no_speech_detected");
            assert_eq!(archive_name, SONIOX_ARCHIVE);
        }
        other => panic!("{other:?}"),
    }
    let dg = deepgram("r", Some(30.0), &[]);
    assert!(matches!(plan(&running(Some(1)), &dg), IngestPlan::NoSegments { duration: DurationDecision::Keep, .. }));
}

// TRX-024, JOB-030: a normal result plans lines and the duration decision.
#[test]
fn completion_plan() {
    let obs = soniox("t", ObservationOrigin::Webhook, "completed", Some(300_000.0), Some(speech()));
    match plan(&running(Some(10)), &obs) {
        IngestPlan::Complete { duration, lines, .. } => {
            assert_eq!(duration, DurationDecision::Lower { minutes: 5 });
            assert_eq!(lines.len(), 1);
        }
        other => panic!("{other:?}"),
    }
}

// JOB-044, F2: one key per archive write; two writes of one job never share an object.
#[test]
fn archive_layout() {
    let j = running(None);
    let (first, second) = (uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2));
    assert_eq!(archive_key(&j, first, DEEPGRAM_ARCHIVE), format!("{}/1/{first}/deepgram_transcript.json", j.account));
    assert_ne!(archive_key(&j, first, DEEPGRAM_ARCHIVE), archive_key(&j, second, DEEPGRAM_ARCHIVE));
}

// C-6: identical deliveries share a fingerprint; a changed payload does not.
#[test]
fn fingerprints_compare_deliveries() {
    let a = soniox("t", ObservationOrigin::Webhook, "completed", Some(1.0), Some(speech()));
    let b = soniox("t", ObservationOrigin::Webhook, "completed", Some(2.0), Some(speech()));
    assert_eq!(fingerprint(&a), fingerprint(&a.clone()));
    assert_ne!(fingerprint(&a), fingerprint(&b));
}
