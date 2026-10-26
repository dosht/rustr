use crate::ai::{prompts, sanitize};

#[test]
fn source_input_and_output_are_distinct_paths_not_token_filters() {
    let raw = " <script>alert(1)</script><b>SELECT</b> javascript: onclick= x@y.com  ";
    assert_eq!(sanitize::input(raw), "[FILTERED] [FILTERED] [FILTERED] [EMAIL_REDACTED]");
    assert_eq!(sanitize::output(raw), "alert(1)SELECT onclick= [EMAIL_REDACTED]");
    assert_eq!(sanitize::input("selection UPDATE café"), "selection [FILTERED] café");
    assert_eq!(sanitize::output("DROP table"), "DROP table");
}

#[test]
fn cumulative_sanitation_handles_html_email_and_spacing_across_provider_boundaries() {
    assert_eq!(sanitize::output("Hello "), "Hello");
    assert_eq!(sanitize::output("Hello world"), "Hello world");
    let chunks = ["<b>Contact a", "da@exa", "mple.com</", "b> now"];
    let mut raw = String::new();
    for chunk in chunks { raw.push_str(chunk); }
    assert_eq!(sanitize::output(&raw), "Contact [EMAIL_REDACTED] now");
    assert_eq!(sanitize::input("4111-1111-1111-1111 123-45-6789 800-555-1212 abcdefghijklmnopqrstuvwxyz123456"), "[CREDIT_CARD_REDACTED] [SSN_REDACTED] [PHONE_REDACTED] [API_KEY_REDACTED]");
    // Source JS word boundaries are ASCII, including next to non-ASCII letters.
    assert_eq!(sanitize::input("éSELECTé"), "é[FILTERED]é");
}

#[test]
fn transcript_context_and_question_security_use_source_utf16_boundaries() {
    assert!(sanitize::validate_chat_context(&"a".repeat(49)).is_err());
    assert!(sanitize::validate_chat_context(&"😀".repeat(25)).is_ok());
    assert!(sanitize::validate_chat_context(&"a".repeat(512_001)).is_err());
    assert!(sanitize::validate_input_security(&"😀".repeat(256_000)).is_ok());
    assert!(sanitize::validate_input_security(&"😀".repeat(256_001)).is_err());
}

#[test]
fn chat_language_instructions_keep_source_exact_code_lookup_and_raw_unknown_fallback() {
    let fr = prompts::chat_system_message("Context", Some("fr"));
    assert!(fr.contains("Please respond in French."));
    assert!(fr.ends_with("Transcript context:\nContext"));
    assert!(!prompts::chat_system_message("Context", Some("en")).contains("Please respond in"));
    assert!(prompts::chat_system_message("Context", Some("fr-CA")).contains("Please respond in fr-CA."));
}

#[test]
fn canonical_analysis_is_raw_while_summary_and_chat_drop_blank_lines_and_sanitize() {
    use crate::presentation::fixtures::line;
    let lines = [
        line(1, 1, "00:00:00", "00:00:01", "  SELECT <b>plan</b>  ", Some(" Ada ")),
        line(2, 1, "00:00:01", "00:00:02", " \u{feff} ", None),
        line(3, 1, "00:00:02", "00:00:03", " next ", Some("")),
    ];
    let analysis = prompts::analysis_messages(&lines);
    assert!(analysis.user.contains("Transcript:\n Ada :   SELECT <b>plan</b>  \nSpeaker:  \u{feff} \nSpeaker:  next \n\nRemember:"));
    assert_eq!(sanitize::transcript_text(&lines, true), " Ada : SELECT <b>plan</b> next");
    assert_eq!(sanitize::transcript_text(&lines, false), "SELECT <b>plan</b> next");
    let summary = prompts::summary_messages(&lines, false);
    assert!(summary.user.contains("Transcript:\nAda : [FILTERED] plan next\n\nSummary:"));
    let detailed = prompts::summary_messages(&lines, true);
    assert!(detailed.user.starts_with("Please create a detailed summary"));
}

#[test]
fn source_js_whitespace_and_ascii_case_folding_are_not_unicode_generalizations() {
    assert_eq!(sanitize::output("\u{feff} a\u{a0}b\u{feff}"), "a b");
    assert_eq!(sanitize::output("\u{0085}hello\u{0085}"), "\u{0085}hello\u{0085}");
    assert_eq!(sanitize::input("ſELECT javascript: JAVASCRIPT:"), "ſELECT [FILTERED] [FILTERED]");
    assert_eq!(sanitize::input("<ScRiPt>x<em>y</em></sCrIpT>ok"), "ok");
    assert_eq!(sanitize::input("<script>unclosed"), "unclosed");
}

#[test]
fn raw_route_rejections_precede_sanitation_and_keep_source_substring_semantics() {
    use sanitize::QuestionError;
    assert_eq!(sanitize::validate_raw_question(" \u{feff} "), Err(QuestionError::Required));
    assert_eq!(sanitize::validate_raw_question(&"😀".repeat(50_001)), Err(QuestionError::RawTooLong));
    assert_eq!(sanitize::validate_raw_question(&"😀".repeat(50_000)), Ok(()));
    for question in ["<scripture", "JAVASCRIPT:x", "data:text/html", "vbscript:x", "preselect text", "select\u{feff}text"] {
        assert_eq!(sanitize::validate_raw_question(question), Err(QuestionError::Unsafe), "{question}");
    }
    // Source trims first and does not require a SQL word boundary.
    assert_eq!(sanitize::validate_raw_question("select "), Ok(()));
    assert_eq!(sanitize::validate_raw_question("SELECT?"), Ok(()));
    assert_eq!(sanitize::validate_raw_question("ſelect text"), Ok(()));
}

#[test]
fn optional_question_validation_is_separate_from_unconditional_route_cap() {
    use sanitize::QuestionError;
    let question = "a".repeat(10_001);
    assert_eq!(sanitize::validate_raw_question(&question), Ok(()));
    assert_eq!(sanitize::validate_question(&question, "context"), Err(QuestionError::TooLong));
    assert_eq!(sanitize::validate_question(&"😀".repeat(5_000), "context"), Ok(()));
    assert_eq!(sanitize::validate_question(" ", ""), Err(QuestionError::Empty));
    assert_eq!(sanitize::validate_question("Valid?", " \u{feff} "), Err(QuestionError::NoTranscript));
}

#[test]
fn analysis_substitutes_transcript_literally_instead_of_expanding_js_replacement_directives() {
    use crate::presentation::fixtures::line;
    let transcript = r#"$$ $& $` $' {{transcript}}"#;
    let messages = prompts::analysis_messages(&[line(1, 1, "0", "1", transcript, Some("Ada"))]);
    let submitted = messages.user.split_once("\nTranscript:\n").unwrap().1
        .rsplit_once("\n\nRemember:").unwrap().0;
    assert_eq!(submitted, format!("Ada: {transcript}"));
}

#[test]
fn analysis_estimate_uses_actual_raw_transcript_utf16_rounding_not_bytes_prompt_or_chat_ceiling() {
    use crate::presentation::fixtures::line;
    let lines = [
        line(1, 1, "0", "1", "😀", Some("😀")),
        line(2, 1, "1", "2", "x", None),
    ];
    // Actual canonical text is \"😀: 😀\\nSpeaker: x\": 17 UTF-16 units,
    // versus 21 UTF-8 bytes; JS Math.round(17/4)=4 (not chat's ceil=5).
    assert_eq!(prompts::analysis_input_estimated_tokens(&lines), 4);
    assert_eq!(prompts::analysis_input_estimated_tokens(&[line(1, 1, "0", "1", "😀", Some("A"))]), 1);
    assert_eq!(prompts::analysis_input_estimated_tokens(&[line(1, 1, "0", "1", "😀x", Some("A"))]), 2);
    assert_eq!(prompts::analysis_input_estimated_tokens(&[]), 0);
    // Analysis includes even a blank row with its fallback speaker prefix.
    assert_eq!(prompts::analysis_input_estimated_tokens(&[line(1, 1, "0", "1", "", Some(""))]), 2);
}
