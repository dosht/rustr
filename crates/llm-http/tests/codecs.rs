use llm_core::*;
use llm_http::*;

fn frame(seq: u64, event: TextEvent) -> ApplicationFrame {
    ApplicationFrame { identity: StreamIdentity { generation: "g".into(), content_revision: 7, consent_epoch: 3, execution: "w".into() }, seq, event }
}
fn snapshot(seq: u64, text: &str) -> ApplicationFrame {
    frame(seq, TextEvent::Snapshot { text: text.into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional })
}
fn feed(decoder: &mut ApplicationDecoder, input: &[u8]) -> Vec<ApplicationFrame> {
    let mut offset = 0; let mut frames = Vec::new();
    while offset < input.len() { let (used, frame) = decoder.push(&input[offset..]).unwrap(); assert!(used > 0); offset += used; if let Some(frame) = frame { frames.push(frame); } }
    frames
}

#[test]
fn one_byte_utf8_crlf_and_multiline_sse_preserve_content() {
    let input = ": keepalive\r\nid: 4\r\nevent: text\r\ndata: {\"text\":\r\ndata: \"ş😀\"}\r\n\r\n";
    let mut decoder = SseDecoder::new(512); let mut got = Vec::new();
    for byte in input.as_bytes() { let (used, event) = decoder.push(&[*byte]).unwrap(); assert_eq!(used, 1); if let Some(event) = event { got.push(event); } }
    decoder.finish().unwrap();
    assert_eq!(got, vec![SseEvent { event: "text".into(), id: Some("4".into()), data: "{\"text\":\n\"ş😀\"}".into() }]);
}

#[test]
fn unterminated_records_invalid_utf8_and_comment_flood_are_not_success() {
    let mut decoder = SseDecoder::new(64);
    decoder.push(b"data: {\"partial\":true}\n").unwrap();
    assert_eq!(decoder.finish(), Err(CodecError::Incomplete));
    let mut decoder = SseDecoder::new(64);
    assert_eq!(decoder.push(b"data: \xff\n\n"), Err(CodecError::Utf8));
    let mut decoder = SseDecoder::new(8);
    assert_eq!(decoder.push(b":12345678"), Err(CodecError::Limit));
}

#[test]
fn actual_codec_reconnect_replaces_whitespace_and_cross_chunk_redaction() {
    let mut state = StreamState::default(); state.start(frame(0, TextEvent::Revoked).identity);
    state.update(frame(1, TextEvent::Delta("Hello ".into()))).unwrap();
    state.resume();
    let mut decoder = ApplicationDecoder::new(Limits::default());
    let saved = encode_frame(&snapshot(1, "Hello"), Limits::default()).unwrap();
    for byte in saved.as_bytes() { for frame in feed(&mut decoder, &[*byte]) { state.update(frame).unwrap(); } }
    let suffix = encode_frame(&frame(2, TextEvent::Delta("world alice@".into())), Limits::default()).unwrap();
    let decoded = feed(&mut decoder, suffix.as_bytes()).pop().unwrap();
    assert_eq!(state.update(decoded).unwrap(), StreamUpdate::ResnapshotRequired);
    for frame in feed(&mut decoder, encode_frame(&snapshot(2, "Hello world alice@"), Limits::default()).unwrap().as_bytes()) { state.update(frame).unwrap(); }
    for frame in feed(&mut decoder, encode_frame(&snapshot(3, "Hello world [EMAIL_REDACTED]"), Limits::default()).unwrap().as_bytes()) { state.update(frame).unwrap(); }
    let final_frame = frame(4, TextEvent::Final { text: "Hello world [EMAIL_REDACTED] bold".into(), persistence: Persistence::Saved });
    for frame in feed(&mut decoder, encode_frame(&final_frame, Limits::default()).unwrap().as_bytes()) { state.update(frame).unwrap(); }
    decoder.finish().unwrap();
    assert_eq!(state.view().text, "Hello world [EMAIL_REDACTED] bold");
    assert_eq!(state.view().status, StreamStatus::Completed);
    assert_eq!(state.view().durable_seq, Some(4));
}

#[test]
fn application_envelope_rejects_identity_spoofing_pending_question_and_false_saved_state() {
    let good = encode_application_json(&snapshot(2, "public"), Limits::default()).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&good).unwrap();
    json["question"] = "private pending user text".into();
    assert_eq!(decode_application_json(serde_json::to_string(&json).unwrap().as_bytes(), Limits::default()), Err(CodecError::Json));
    let wire = format!("event: text\nid: another:2\ndata: {good}\n\n");
    assert_eq!(ApplicationDecoder::new(Limits::default()).push(wire.as_bytes()), Err(CodecError::Envelope));
    let invalid = frame(3, TextEvent::Snapshot { text: "unsaved".into(), status: StreamStatus::Completed, persistence: Persistence::Unsaved });
    assert_eq!(encode_frame(&invalid, Limits::default()), Err(CodecError::Envelope));
}

#[test]
fn escaped_large_snapshot_is_bounded_by_decoded_text_not_provider_record_size() {
    let limits = Limits { output_bytes: 1024 * 1024, ..Limits::default() };
    let text = "\u{0001}".repeat(300_000);
    let wire = encode_frame(&snapshot(1, &text), limits).unwrap();
    let mut decoder = ApplicationDecoder::new(limits);
    let decoded = feed(&mut decoder, wire.as_bytes()).pop().unwrap();
    assert_eq!(decoded, snapshot(1, &text));
    assert_eq!(encode_frame(&snapshot(2, &"a".repeat(limits.output_bytes + 1)), limits), Err(CodecError::Limit));
}

#[test]
fn nesting_attack_is_rejected_before_deserializing_application_tree() {
    let bytes = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert_eq!(decode_application_json(bytes.as_bytes(), Limits::default()), Err(CodecError::Depth));
}

#[test]
fn every_encodable_frame_decodes_under_the_same_limits() {
    // Small output limit, maximum escaped control-character error and maximum quoted identities.
    let limits = Limits { output_bytes: 1024, ..Limits::default() };
    let identity = StreamIdentity { generation: "\"".repeat(256), content_revision: u64::MAX, consent_epoch: u64::MAX, execution: "\\".repeat(256) };
    let frames = [
        ApplicationFrame { identity: identity.clone(), seq: u64::MAX, event: TextEvent::Interrupted(PublicError { code: "\u{0001}".repeat(256), message: "\u{0001}".repeat(4096) }) },
        ApplicationFrame { identity: identity.clone(), seq: u64::MAX, event: TextEvent::Snapshot { text: "\u{0001}".repeat(1024), status: StreamStatus::Streaming, persistence: Persistence::Provisional } },
        ApplicationFrame { identity: identity.clone(), seq: u64::MAX, event: TextEvent::Final { text: "\u{0001}".repeat(1024), persistence: Persistence::Saved } },
    ];
    for expected in frames {
        let json = encode_application_json(&expected, limits).unwrap();
        assert_eq!(decode_application_json(json.as_bytes(), limits), Ok(expected.clone()));
        let wire = encode_frame(&expected, limits).unwrap();
        let mut decoder = ApplicationDecoder::new(limits);
        assert_eq!(feed(&mut decoder, wire.as_bytes()), vec![expected]);
        decoder.finish().unwrap();
    }
    // The ceiling stays bounded: one byte of text over the output limit is still refused.
    let over = ApplicationFrame { identity, seq: 1, event: TextEvent::Delta("a".repeat(1025)) };
    assert_eq!(encode_frame(&over, limits), Err(CodecError::Limit));
}
