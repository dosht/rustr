use super::*;
use crate::jobs::domain::JobStatus;
use crate::languages::Language;
use crate::presentation::fixtures;

const NOW: Micros = Micros(1_800_000_000_000_000);

fn id() -> DeliveryId {
    DeliveryId(Uuid::from_u128(7))
}

fn completed(callback_url: Option<&str>) -> JobEvent {
    JobEvent::Completed {
        job: JobId(3),
        account: fixtures::account(),
        provider: None,
        language: None,
        error: None,
        callback_url: callback_url.map(str::to_string),
        at: Micros(NOW.0 - 5),
    }
}

fn status(code: u16) -> SendOutcome {
    SendOutcome::Status { code, excerpt: String::new() }
}

// D6, API-015: only a completion with a callback URL delivers; a failed job, a
// completion without a URL, or with an empty one, delivers nothing.
#[test]
fn d6_only_completed_jobs_with_a_callback_url_deliver() {
    let d = delivery_for(id(), 41, &completed(Some("https://hooks.example.test/t")), NOW).expect("a delivery");
    assert_eq!((d.id, d.account, d.job, d.event_id, d.url.as_str()), (id(), fixtures::account(), JobId(3), 41, "https://hooks.example.test/t"));
    assert_eq!((d.state, d.attempts, d.next_attempt_at, d.completed_at, d.created_at), (DeliveryState::Pending, 0, NOW, Micros(NOW.0 - 5), NOW));
    assert_eq!((d.delivered_at, d.payload.clone()), (None, None));
    assert_eq!(delivery_for(id(), 41, &completed(None), NOW), None);
    assert_eq!(delivery_for(id(), 41, &completed(Some("")), NOW), None);
    let failed = JobEvent::Failed { job: JobId(3), account: fixtures::account(), language: None, error: None, at: NOW };
    assert_eq!(delivery_for(id(), 42, &failed, NOW), None);
}

// D5, API-016: 2xx delivers; network, timeout, 408, 429 and 5xx retry; other 4xx,
// redirects, informational answers and refused destinations are final.
#[test]
fn d5_retryable_and_final_outcomes() {
    for code in [200, 201, 204, 299] {
        assert_eq!(classify(&status(code)), Verdict::Delivered, "{code}");
    }
    for outcome in [status(408), status(429), status(500), status(502), status(599), SendOutcome::Timeout, SendOutcome::Network { error: "reset".into() }] {
        assert_eq!(classify(&outcome), Verdict::Retry, "{outcome:?}");
    }
    for outcome in [status(400), status(401), status(404), status(410), status(422), status(301), status(302), status(100), SendOutcome::Refused { reason: "private".into() }] {
        assert_eq!(classify(&outcome), Verdict::Final, "{outcome:?}");
    }
}

// D5: retries at +1 m, 5 m, 30 m, 2 h, 12 h after the previous attempt; the sixth
// failure abandons; a delivered or final attempt schedules nothing.
#[test]
fn d5_schedule_has_six_attempts_then_abandons() {
    let expected = [60, 300, 1_800, 7_200, 43_200];
    for (n, seconds) in (1..=5).zip(expected) {
        let s = settle(n, &status(503), NOW);
        assert_eq!(s, Settlement { state: DeliveryState::Pending, next_attempt_at: Some(Micros(NOW.0 + seconds * MICROS_PER_SECOND)), delivered_at: None }, "after attempt {n}");
    }
    assert_eq!(settle(6, &SendOutcome::Timeout, NOW), Settlement { state: DeliveryState::Abandoned, next_attempt_at: None, delivered_at: None });
    assert_eq!(settle(3, &status(200), NOW), Settlement { state: DeliveryState::Delivered, next_attempt_at: None, delivered_at: Some(NOW) });
    assert_eq!(settle(1, &status(404), NOW), Settlement { state: DeliveryState::Failed, next_attempt_at: None, delivered_at: None });
    assert_eq!(MAX_ATTEMPTS, 6);
}

// D7: wait while the P4 summary outcome is undecided, rechecking, at most 15 minutes
// after completion; any decided outcome sends at once.
#[test]
fn d7_wait_for_the_summary_at_most_fifteen_minutes() {
    let done = NOW;
    assert_eq!(summary_gate(None, done, done), SummaryGate::Wait { recheck_at: Micros(done.0 + SUMMARY_RECHECK) });
    let near = Micros(done.0 + SUMMARY_WAIT - 10 * MICROS_PER_SECOND);
    assert_eq!(summary_gate(None, done, near), SummaryGate::Wait { recheck_at: Micros(done.0 + SUMMARY_WAIT) }, "never past the bound");
    assert_eq!(summary_gate(None, done, Micros(done.0 + SUMMARY_WAIT)), SummaryGate::Send);
    let saved = SummaryOutcome::Saved { generation: crate::ai::GenerationId(Uuid::from_u128(1)) };
    assert_eq!(summary_gate(Some(&saved), done, done), SummaryGate::Send);
    assert_eq!(summary_gate(Some(&SummaryOutcome::Failed { reason: "x".into() }), done, done), SummaryGate::Send);
    assert_eq!(summary_gate(Some(&SummaryOutcome::Absent { reason: "x".into() }), done, done), SummaryGate::Send);
}

// API-015, D10: the source body; corrected_text kept, summary only when present,
// audio_url null in the stored payload and signed in at send.
#[test]
fn api_015_payload_matches_the_source_shape() {
    let mut job = fixtures::job(3, JobStatus::Completed, "call.mp3");
    job.duration_minutes = Some(4);
    job.language = Language::from_name("German");
    job.fix_grammer = Some(true);
    job.error = Some(json!({"message": "partial"}));
    let mut first = fixtures::line(1, 3, "00:00:00", "00:00:05", "Hallo", Some("SPEAKER_00"));
    first.corrected_text = Some("Hallo!".into());
    let second = fixtures::line(2, 3, "00:00:05", "00:00:09", "Welt", None);
    let body = payload(&job, &[first, second], Some("Short summary"));
    assert_eq!(
        body,
        json!({
            "job": {"id": 3, "status": "completed", "duration": 4, "language": "German", "audio_url": null, "fix_grammer": true},
            "result": [
                {"start_time": "00:00:00", "end_time": "00:00:05", "text": "Hallo", "corrected_text": "Hallo!", "speaker": "SPEAKER_00"},
                {"start_time": "00:00:05", "end_time": "00:00:09", "text": "Welt", "corrected_text": null, "speaker": null},
            ],
            "errors": {"message": "partial"},
            "summary": {"text": "Short summary"},
        })
    );
    let without = payload(&job, &[], None);
    assert!(without.get("summary").is_none() && without["result"] == json!([]));
    assert_eq!(with_audio_url(&body, Some("https://signed.example.test/a"))["job"]["audio_url"], "https://signed.example.test/a");
    assert_eq!(body["job"]["audio_url"], Value::Null, "the stored payload never holds the signed URL");
}

// D5: the signed bytes and the header format.
#[test]
fn d5_signature_covers_timestamp_and_body() {
    assert_eq!(signed_message(1_700_000_000, b"{}"), b"1700000000.{}".to_vec());
    assert_eq!(signature(1_700_000_000, "ab12"), "t=1700000000,v1=ab12");
}
