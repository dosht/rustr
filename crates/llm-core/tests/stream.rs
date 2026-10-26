use llm_core::*;

fn identity() -> StreamIdentity {
    StreamIdentity { generation: "g-1".into(), content_revision: 7, consent_epoch: 2, execution: "worker-1".into() }
}
fn frame(seq: u64, event: TextEvent) -> ApplicationFrame { ApplicationFrame { identity: identity(), seq, event } }
fn checkpoint(seq: u64, text: &str) -> ApplicationFrame {
    frame(seq, TextEvent::Snapshot { text: text.into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional })
}

#[test]
fn reconnect_never_appends_a_raw_suffix_to_a_sanitized_prefix() {
    let mut state = StreamState::default();
    state.start(identity());
    state.update(frame(1, TextEvent::Delta("Hello ".into()))).unwrap();
    state.resume();
    state.update(checkpoint(1, "Hello")).unwrap();
    assert_eq!(state.update(frame(2, TextEvent::Delta("world".into()))).unwrap(), StreamUpdate::ResnapshotRequired);
    assert_eq!(state.view().text, "Hello");
    state.update(checkpoint(2, "Hello world")).unwrap();
    state.update(frame(3, TextEvent::Final { text: "Hello world".into(), persistence: Persistence::Saved })).unwrap();
    assert_eq!(state.view().text, "Hello world");
    assert_eq!(state.view().status, StreamStatus::Completed);
}

#[test]
fn cumulative_redaction_and_html_replacements_are_not_prefixes() {
    let mut state = StreamState::default();
    state.start(identity());
    state.update(checkpoint(1, "Email alice@")).unwrap();
    state.update(checkpoint(2, "Email [EMAIL_REDACTED]")).unwrap();
    state.update(checkpoint(3, "Email [EMAIL_REDACTED] Hello")).unwrap();
    state.update(checkpoint(4, "Email [EMAIL_REDACTED] Hello world")).unwrap();
    assert_eq!(state.view().text, "Email [EMAIL_REDACTED] Hello world");
    assert_eq!(state.view().mode, DeliveryMode::SnapshotReplacement);
}

#[test]
fn delta_gap_requests_snapshot_but_cumulative_watermark_can_skip() {
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Delta("a".into()))).unwrap();
    assert_eq!(state.update(frame(3, TextEvent::Delta("c".into()))).unwrap(), StreamUpdate::ResnapshotRequired);
    assert_eq!(state.view().text, "a");
    state.update(checkpoint(4, "abcd")).unwrap();
    assert_eq!(state.view().text, "abcd");
    assert_eq!(state.update(checkpoint(3, "abc")).unwrap(), StreamUpdate::Ignored);
}

#[test]
fn resume_can_replace_an_uncheckpointed_suffix_with_durable_state() {
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Delta("one ".into()))).unwrap();
    state.update(frame(2, TextEvent::Delta("lost".into()))).unwrap();
    state.resume();
    state.update(checkpoint(1, "one")).unwrap();
    assert_eq!(state.view().text, "one");
    assert_eq!(state.view().last_seq, Some(1));
}

#[test]
fn old_epoch_worker_and_generation_cannot_change_or_revoke_new_view() {
    let mut state = StreamState::default(); state.start(identity());
    let mut old = checkpoint(1, "private"); old.identity.consent_epoch = 1;
    assert_eq!(state.update(old).unwrap(), StreamUpdate::Ignored);
    let mut old = frame(2, TextEvent::Revoked); old.identity.execution = "old".into();
    assert_eq!(state.update(old).unwrap(), StreamUpdate::Ignored);
    state.update(frame(1, TextEvent::Delta("current".into()))).unwrap();
    state.update(frame(2, TextEvent::Revoked)).unwrap();
    assert_eq!(state.view().text, "");
    assert_eq!(state.view().status, StreamStatus::Revoked);
    assert_eq!(state.update(frame(3, TextEvent::Delta("late".into()))).unwrap(), StreamUpdate::Ignored);
}

#[test]
fn unsaved_final_is_not_completed_and_can_be_reconciled_without_append() {
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Final { text: "answer".into(), persistence: Persistence::Unsaved })).unwrap();
    assert_ne!(state.view().status, StreamStatus::Completed);
    assert_eq!(state.view().persistence, Persistence::Unsaved);
    state.update(frame(2, TextEvent::Final { text: "answer".into(), persistence: Persistence::Saved })).unwrap();
    assert_eq!(state.view().status, StreamStatus::Completed);
    assert_eq!(state.view().text, "answer");
}

#[test]
fn output_bounds_refuse_without_partially_applying_text() {
    let mut state = StreamState::new(4); state.start(identity());
    state.update(frame(1, TextEvent::Delta("abcd".into()))).unwrap();
    assert_eq!(state.update(frame(2, TextEvent::Delta("e".into()))), Err(StreamError::OutputLimit));
    assert_eq!(state.view().text, "abcd");
}

#[test]
fn queue_coalesces_snapshot_discards_raw_and_bounds_slow_attachment() {
    let mut queue = AttachmentQueue::new(identity(), 1024, 32);
    queue.push(frame(1, TextEvent::Delta("a".repeat(512)))).unwrap();
    assert_eq!(queue.push(frame(2, TextEvent::Delta("b".repeat(512)))), Err(QueueError::SlowConsumer));
    let mut queue = AttachmentQueue::new(identity(), 1024, 32);
    queue.push(frame(1, TextEvent::Delta("raw".into()))).unwrap();
    queue.push(checkpoint(1, "clean")).unwrap();
    assert_eq!(queue.push(frame(2, TextEvent::Delta("suffix".into()))).unwrap(), QueueUpdate::SnapshotRequired);
    queue.push(checkpoint(3, "clean replacement")).unwrap();
    assert_eq!(queue.pop().unwrap(), checkpoint(3, "clean replacement"));
    assert!(queue.pop().is_none());
}

#[test]
fn interrupted_stream_cannot_accept_late_raw_deltas_without_explicit_resume() {
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Delta("partial".into()))).unwrap();
    state.update(frame(2, TextEvent::Interrupted(PublicError { code: "failed".into(), message: "Unavailable".into() }))).unwrap();
    assert_eq!(state.update(frame(3, TextEvent::Delta("late".into()))).unwrap(), StreamUpdate::Ignored);
    assert_eq!(state.view().text, "partial");
    state.resume();
    state.update(checkpoint(3, "authoritative")).unwrap();
    assert_eq!(state.view().text, "authoritative");
}

#[test]
fn cancellation_keeps_an_already_completed_saved_answer() {
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Final { text: "saved".into(), persistence: Persistence::Saved })).unwrap();
    state.cancel();
    assert_eq!(state.view().status, StreamStatus::Completed);
    assert_eq!(state.view().text, "saved");
}

fn control(seq: u64, code: &str) -> ApplicationFrame {
    frame(seq, TextEvent::Interrupted(PublicError { code: code.into(), message: "The sign-in service is not reachable.".into() }))
}

#[test]
fn resumed_attachment_applies_control_interruption_without_text_change() {
    // First frame after resume is a host control frame at durable + 1.
    let mut state = StreamState::default(); state.start(identity());
    state.update(frame(1, TextEvent::Delta("Hello ".into()))).unwrap();
    state.resume();
    assert_eq!(state.update(control(1, "identity_unavailable")).unwrap(), StreamUpdate::Applied { discard_deltas: false });
    assert_eq!(state.view().status, StreamStatus::Interrupted);
    assert_eq!(state.view().error.map(|e| e.code.as_str()), Some("identity_unavailable"));
    assert_eq!(state.view().text, "Hello ");
    assert_eq!(state.view().durable_seq, None, "a control frame is not a durable checkpoint");

    // Raw deltas moved last_seq past the host's durable sequence; the control frame still applies.
    let mut state = StreamState::default(); state.start(identity());
    for (seq, text) in [(1, "a"), (2, "b"), (3, "c")] { state.update(frame(seq, TextEvent::Delta(text.into()))).unwrap(); }
    state.resume();
    assert_eq!(state.update(control(2, "unavailable")).unwrap(), StreamUpdate::Applied { discard_deltas: false });
    assert_eq!(state.view().error.map(|e| e.code.as_str()), Some("unavailable"));
    assert_eq!(state.view().text, "abc");
}

#[test]
fn resumed_control_interruption_keeps_the_durable_cursor_and_rejects_stale_controls() {
    let mut state = StreamState::default(); state.start(identity());
    state.resume();
    state.update(checkpoint(5, "saved")).unwrap();
    state.resume();
    assert_eq!(state.update(control(3, "unavailable")).unwrap(), StreamUpdate::Ignored, "older than the durable checkpoint");
    assert_eq!(state.update(control(6, "unavailable")).unwrap(), StreamUpdate::Applied { discard_deltas: false });
    assert_eq!(state.view().durable_seq, Some(5));
    assert_eq!(state.view().text, "saved");
    // The next attachment can still recover from the durable checkpoint.
    state.resume();
    state.update(checkpoint(6, "saved more")).unwrap();
    assert_eq!(state.view().text, "saved more");
    assert_eq!(state.view().status, StreamStatus::Streaming);
}
