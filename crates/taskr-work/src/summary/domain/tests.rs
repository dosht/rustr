use super::*;

fn generation() -> Generation {
    Generation::new(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), 4, 100, 65_000_100)
}

#[test]
fn cancellation_fences_late_checkpoints_and_a_final_and_preserves_observed_partial_text() {
    let mut g = generation();
    assert!(g.claim(101));
    assert!(g.checkpoint("partial".into(), 102).unwrap());
    assert!(g.cancel(103));
    let cancelled = g.clone();
    assert!(!g.checkpoint("late provider answer".into(), 104).unwrap());
    assert!(!g.stage("late final".into(), 105).unwrap());
    assert!(!g.complete(106));
    assert_eq!(g, cancelled);
    assert_eq!(g.snapshot().status, GenerationStatus::Cancelled);
    assert!(g.snapshot().remote_outcome_unknown, "local abort is not remote deletion/completion proof");
    assert_eq!(g.snapshot().text, "partial");
}

#[test]
fn final_does_not_claim_completion_until_canonical_result_commit_and_cannot_be_cancelled_afterwards() {
    let mut g = generation();
    assert!(g.claim(101));
    assert!(g.checkpoint("provisional answer ".into(), 102).unwrap());
    assert!(g.stage("authoritative final".into(), 103).unwrap());
    assert_eq!(g.snapshot().status, GenerationStatus::Running, "staging is not committed completion");
    assert_eq!(g.snapshot().text, "provisional answer ");
    assert!(g.complete(104));
    let completed = g.clone();
    assert_eq!(g.snapshot().text, "authoritative final");
    assert!(!g.snapshot().remote_outcome_unknown);
    assert!(!g.cancel(105));
    assert!(!g.interrupt("late error".into(), 106));
    assert_eq!(g, completed);
}

#[test]
fn cancelling_before_dispatch_never_claims_remote_work_and_recovery_does_not_redispatch() {
    let mut g = generation();
    assert!(g.cancel(101));
    assert!(!g.snapshot().remote_outcome_unknown);
    assert!(!g.claim(102));
    let mut interrupted = generation();
    assert!(interrupted.claim(101));
    assert!(interrupted.interrupt("worker lost".into(), 102));
    assert!(!interrupted.claim(103), "recovery cannot replay a possibly accepted request");
    assert!(interrupted.snapshot().remote_outcome_unknown);
}

#[test]
fn output_limit_refuses_without_truncating_or_changing_the_last_checkpoint() {
    let mut g = generation();
    assert!(g.claim(101));
    assert!(g.checkpoint("kept".into(), 102).unwrap());
    let before = g.clone();
    assert_eq!(g.checkpoint("x".repeat(MAX_OUTPUT_BYTES + 1), 103), Err(OutputLimit));
    assert_eq!(g, before);
}

#[test]
fn provider_ids_stay_distinct_first_seen_and_bounded_without_dropping_known_ones() {
    let mut known = vec!["hdr".to_string()];
    assert_eq!(merge_remote_ids(&mut known, &["hdr".to_string(), "body".to_string(), String::new()]), Ok(()));
    assert_eq!(known, ["hdr", "body"], "a repeat or empty id is not a new identity");
    let long = "x".repeat(MAX_REMOTE_ID_BYTES + 1);
    assert_eq!(merge_remote_ids(&mut known, &[long, "next".to_string()]), Err(RemoteIdLimit::TooLong));
    assert_eq!(known, ["hdr", "body", "next"], "an oversized id is refused whole, never truncated, and later ids still fit");
    let more: Vec<String> = (0..MAX_REMOTE_IDS).map(|i| format!("id-{i}")).collect();
    assert_eq!(merge_remote_ids(&mut known, &more), Err(RemoteIdLimit::TooMany));
    assert_eq!(known.len(), MAX_REMOTE_IDS);
    assert_eq!(&known[..3], ["hdr", "body", "next"], "known identities are always retained");
}

#[test]
fn the_execution_lease_starts_at_the_first_claim_however_late_it_was_admitted() {
    let mut g = generation();
    assert!(g.claim(60_000_000));
    assert_eq!(g.lease_until_us, 60_000_000 + PROVIDER_DEADLINE_US + SETTLE_MARGIN_US);
    let mut late = generation();
    assert!(!late.claim(65_000_100), "an accepted generation past its admission lease is never dispatched");
}

#[test]
fn retention_clears_noncanonical_text_and_keeps_the_reason_provider_facts_and_completed_summaries() {
    let mut running = generation();
    assert!(running.claim(101));
    running.provider = Some("deepseek".into());
    running.remote_ids = vec!["hdr".into()];
    running.checkpoint("partial".into(), 102).unwrap();
    running.stage("final".into(), 103).unwrap();
    let sequence = running.sequence;
    assert!(running.purge_source_content());
    assert_eq!((running.status, running.text.as_str(), running.staged.as_deref()), (GenerationStatus::Interrupted, "", None));
    assert!(running.error.is_some() && running.remote_outcome_unknown);
    assert_eq!((running.provider.as_deref(), running.remote_ids.as_slice()), (Some("deepseek"), ["hdr".to_string()].as_slice()));
    assert!(running.sequence > sequence, "attachments see the replacement");
    assert!(!running.purge_source_content(), "idempotent");

    let mut cancelled = generation();
    cancelled.claim(101);
    cancelled.checkpoint("partial".into(), 102).unwrap();
    cancelled.cancel(103);
    assert!(cancelled.purge_source_content());
    assert_eq!((cancelled.status, cancelled.text.as_str()), (GenerationStatus::Cancelled, ""));

    let mut completed = generation();
    completed.claim(101);
    completed.stage("saved".into(), 102).unwrap();
    completed.complete(103);
    let canonical = completed.clone();
    assert!(!completed.purge_source_content());
    assert_eq!(completed, canonical, "the canonical summary survives transcript retention");
}
