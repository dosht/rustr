use super::*;

const CASES: &[(AnalysisType, &str)] = &[
    (AnalysisType::Meeting, include_str!("fixtures/meeting.txt")),
    (AnalysisType::Lecture, include_str!("fixtures/lecture.txt")),
    (AnalysisType::Sales, include_str!("fixtures/sales.txt")),
    (AnalysisType::Support, include_str!("fixtures/support.txt")),
    (AnalysisType::Interview, include_str!("fixtures/interview.txt")),
    (AnalysisType::Generic, include_str!("fixtures/generic.txt")),
];

#[test]
fn six_shapes_survive_every_character_boundary_with_typed_optional_content() {
    for &(kind, text) in CASES {
        let mut parser = AnalysisParser::new();
        for c in text.chars() {
            parser.push(c.encode_utf8(&mut [0; 4])).unwrap();
        }
        let result = parser.finish(true).unwrap();
        assert_eq!(result.detected.kind, kind);
        assert_eq!(result.analysis.kind(), kind);
        assert_eq!(result.summary, "A useful café summary with --- quoted data.");
        match result.analysis {
            Analysis::Meeting(m) => {
                let items = m.action_items.unwrap();
                assert_eq!(items[0].status, ActionStatus::Pending);
                assert_eq!(items[0].owner.as_deref(), Some("Ada"));
                assert_eq!(m.decisions.unwrap()[0].timestamp, Some(1.25));
            }
            Analysis::Lecture(l) => {
                assert_eq!(l.topics.unwrap()[0].duration, Some(12.5));
                assert_eq!(l.concepts.unwrap()[0].examples.as_ref().unwrap()[0], "A queue");
            }
            Analysis::Sales(s) => {
                assert_eq!(s.call_metrics.unwrap().next_steps_confirmed, Some(false));
                assert_eq!(s.coaching_tips.unwrap()[0].category, CoachingCategory::Strength);
            }
            Analysis::Support(s) => {
                assert_eq!(s.issue_resolution.unwrap().status, ResolutionStatus::Escalated);
                assert_eq!(s.sentiment.unwrap().key_moments.unwrap()[0].sentiment, SentimentKind::Relieved);
            }
            Analysis::Interview(i) => {
                assert_eq!(i.assessment.unwrap().overall_rating.unwrap().value(), 4.5);
                assert_eq!(i.skills_assessment.unwrap()[0].level, SkillLevel::Advanced);
                assert_eq!(i.next_steps.unwrap()[0].priority, Priority::High);
            }
            Analysis::Generic(g) => {
                assert_eq!(g.highlights.unwrap(), ["One finding"]);
                assert_eq!(g.summary.as_deref(), Some("A useful café summary with --- quoted data."));
            }
        }
    }
}

#[test]
fn sections_are_available_before_provider_finish_and_count_is_advisory_n_minus_two() {
    let mut parser = AnalysisParser::new();
    parser.push("documentCount: 8\r\n---\r\n{\"type\":\"detectedType\",\"value\":\"meeting\",\"confidence\":91}\r\n---\r\n{\"type\":\"summary\",\"content\":\"Ready now\"}\r\n---\r\n").unwrap();
    assert_eq!(parser.progress().expected_content_sections, Some(6));
    assert_eq!(parser.progress().summary.as_deref(), Some("Ready now"));
    assert_eq!(parser.progress().content_sections, 1);
    let result = parser.finish(true).unwrap();
    assert!(matches!(result.warnings.as_slice(), [AnalysisWarning::DocumentCountMismatch { expected: 8, actual: 3 }]));
    let Analysis::Meeting(m) = result.analysis else { panic!("meeting") };
    assert!(m.action_items.is_none());
}

#[test]
fn fallback_uses_javascript_truthiness_not_presence_or_array_emptiness() {
    let mut parser = AnalysisParser::new();
    parser.push("{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":0}\n---\n{\"type\":\"summary\",\"data\":false,\"items\":0,\"content\":\"Actual summary\"}\n---\n{\"type\":\"highlights\",\"data\":[],\"items\":[\"must not replace empty array\"]}").unwrap();
    let result = parser.finish(true).unwrap();
    let Analysis::Generic(g) = result.analysis else { panic!("generic") };
    assert_eq!(g.highlights, Some(vec![]));
    assert_eq!(result.summary, "Actual summary");
}

#[test]
fn semantic_duplicates_ignore_object_order_and_defaulted_fields_but_conflicts_interrupt() {
    let mut parser = AnalysisParser::new();
    parser.push("{\"type\":\"detectedType\",\"value\":\"meeting\",\"confidence\":95}\n---\n{\"type\":\"summary\",\"content\":\"Keep me\"}\n---\n{\"type\":\"actionItems\",\"data\":[{\"task\":\"Ship\"}]}\n---\n{\"type\":\"actionItems\",\"data\":[{\"status\":\"pending\",\"task\":\"Ship\"}]}\n---\n").unwrap();
    assert_eq!(parser.progress().content_sections, 2);
    parser.push("{\"type\":\"summary\",\"content\":\"Contradiction\"}").unwrap();
    let error = parser.finish(true).unwrap_err();
    assert_eq!(error, AnalysisError::ConflictingDuplicate);
    assert_eq!(parser.into_progress().summary.as_deref(), Some("Keep me"));
}

#[test]
fn incomplete_bad_schema_yaml_and_unsuccessful_terminal_preserve_valid_partial() {
    for (tail, success) in [
        ("{\"type\":\"actionItems\",\"data\":[", true),
        ("{\"type\":\"actionItems\",\"data\":[{\"task\":\"x\",\"status\":\"unknown\"}]}", true),
        ("type: actionItems\ndata: []", true),
        ("{\"type\":\"nextSteps\",\"data\":[\"Valid but interrupted\"]}", false),
    ] {
        let mut parser = AnalysisParser::new();
        parser.push("{\"type\":\"detectedType\",\"value\":\"meeting\",\"confidence\":95}\n---\n{\"type\":\"summary\",\"content\":\"Retain\"}\n---\n").unwrap();
        parser.push(tail).unwrap();
        let _ = parser.finish(success).unwrap_err();
        assert_eq!(parser.into_progress().summary.as_deref(), Some("Retain"));
    }
}

#[test]
fn nested_delimiter_is_not_a_document_boundary_and_escaped_quotes_do_not_split() {
    let text = "{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":1}\n---\n{\"type\":\"summary\",\"content\":\"quote \\\" --- café\"}";
    let mut p = AnalysisParser::new();
    p.push(text).unwrap();
    assert_eq!(p.finish(true).unwrap().summary, "quote \" --- café");
    let mut bad = AnalysisParser::new();
    bad.push("{\n---\n}").unwrap();
    assert_eq!(bad.finish(true).unwrap_err(), AnalysisError::MalformedDocument);
}

#[test]
fn missing_type_or_usable_summary_is_not_completion() {
    for text in ["", "{\"type\":\"summary\",\"content\":\"Only summary\"}", "{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":1}\n---\n{\"type\":\"summary\",\"content\":\"  \"}"] {
        let mut p = AnalysisParser::new();
        p.push(text).unwrap();
        assert!(p.finish(true).is_err());
    }
}

#[test]
fn ratings_null_and_cross_type_sections_are_validated() {
    for tail in [
        "{\"type\":\"assessment\",\"data\":{\"overallRating\":5.01}}",
        "{\"type\":\"skillsAssessment\",\"data\":[{\"skill\":\"Rust\",\"rating\":0,\"level\":\"Expert\"}]}",
        "{\"type\":\"assessment\",\"data\":{\"position\":null}}",
        "{\"type\":\"callMetrics\",\"data\":{}}",
    ] {
        let mut p = AnalysisParser::new();
        p.push("{\"type\":\"detectedType\",\"value\":\"interview\",\"confidence\":99}\n---\n{\"type\":\"summary\",\"content\":\"Usable\"}\n---\n").unwrap();
        p.push(tail).unwrap();
        assert!(p.finish(true).is_err());
    }
}

#[test]
fn explicit_document_depth_and_total_bounds_never_truncate() {
    let mut p = AnalysisParser::new();
    assert_eq!(p.push(&" ".repeat(MAX_DOCUMENT_BYTES + 1)), Err(AnalysisError::DocumentTooLarge));
    let mut p = AnalysisParser::new();
    assert_eq!(p.push(&"[".repeat(MAX_JSON_DEPTH + 1)), Err(AnalysisError::TooDeep));
    let mut p = AnalysisParser::new();
    let padding = format!("{}\n---\n", " ".repeat(MAX_DOCUMENT_BYTES - 5));
    for _ in 0..4 { p.push(&padding).unwrap(); }
    assert_eq!(p.push(&padding), Err(AnalysisError::OutputTooLarge));
}

#[test]
fn only_a_single_restricted_count_header_is_yaml_and_json_trim_matches_source_js() {
    for header in ["documentCount:\n3", "documentCount: 3\nother: 1", "documentCount: &count 3"] {
        let mut p = AnalysisParser::new();
        p.push(header).unwrap();
        assert!(p.finish(true).is_err());
    }
    let mut p = AnalysisParser::new();
    p.push("\u{feff}{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":0}\n---\n{\"type\":\"summary\",\"content\":\"Valid\"}\u{feff}").unwrap();
    assert_eq!(p.finish(true).unwrap().summary, "Valid");
    let mut p = AnalysisParser::new();
    p.push("\u{0085}{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":0}").unwrap();
    assert_eq!(p.finish(true).unwrap_err(), AnalysisError::MalformedDocument);
}

#[test]
fn section_snapshots_preserve_empty_sections_and_publish_generic_summary_only_once() {
    let mut parser = AnalysisParser::new();
    parser.push("{\"type\":\"detectedType\",\"value\":\"generic\",\"confidence\":90}\n---\n{\"type\":\"highlights\",\"data\":[]}\n---\n").unwrap();
    assert_eq!(sections(parser.progress()), vec![("highlights", serde_json::json!([]))]);
    // Arrival order differs from the documented schema order. Incomplete content
    // is not published; completing the document adds summary before highlights.
    parser.push("{\"type\":\"summary\",\"content\":\"Usable").unwrap();
    assert_eq!(sections(parser.progress()), vec![("highlights", serde_json::json!([]))]);
    parser.push(" summary\"}\n---\n").unwrap();
    assert_eq!(sections(parser.progress()), vec![
        ("summary", serde_json::json!("Usable summary")),
        ("highlights", serde_json::json!([])),
    ]);
    assert_eq!(parser.progress().content_sections, 2);
    let empty = AnalysisProgress::default();
    assert!(sections(&empty).is_empty());
}

#[test]
fn source_confidence_defaults_only_non_numbers_and_keeps_finite_out_of_range_values() {
    for (field, expected) in [
        ("", 0.0),
        (",\"confidence\":null", 0.0),
        (",\"confidence\":\"95\"", 0.0),
        (",\"confidence\":true", 0.0),
        (",\"confidence\":{}", 0.0),
        (",\"confidence\":-3.5", -3.5),
        (",\"confidence\":101", 101.0),
    ] {
        let mut p = AnalysisParser::new();
        p.push(&format!("{{\"type\":\"detectedType\",\"value\":\"meeting\"{field}}}---{{\"type\":\"summary\",\"content\":\"Keep\"}}")).unwrap();
        let result = p.finish(true).unwrap();
        assert_eq!(result.detected.confidence, expected);
        assert_eq!(result.detected.kind, AnalysisType::Meeting);
    }
}

#[test]
fn sections_before_type_are_deferred_then_validated_and_empty_sections_survive() {
    let mut p = AnalysisParser::new();
    p.push("{\"type\":\"actionItems\",\"data\":[{\"task\":\"Keep\"}]}---{\"type\":\"nextSteps\",\"data\":[]}---{\"type\":\"summary\",\"content\":\"Early summary\"}---").unwrap();
    assert_eq!(p.progress().summary.as_deref(), Some("Early summary"));
    assert!(p.progress().detected.is_none());
    assert_eq!(p.progress().content_sections, 1);
    p.push("{\"type\":\"detectedType\",\"value\":\"meeting\"}---").unwrap();
    assert_eq!(p.progress().content_sections, 3);
    let result = p.finish(true).unwrap();
    let Analysis::Meeting(m) = result.analysis else { panic!("meeting") };
    assert_eq!(m.action_items.unwrap()[0].status, ActionStatus::Pending);
    assert_eq!(m.next_steps, Some(vec![]));
}

#[test]
fn d8_framing_accepts_whitespace_and_adjacent_documents_at_every_character_boundary() {
    let text = include_str!("fixtures/d8-order-framing.txt");
    let mut p = AnalysisParser::new();
    for c in text.chars() { p.push(c.encode_utf8(&mut [0; 4])).unwrap(); }
    let result = p.finish(true).unwrap();
    assert_eq!(result.detected.confidence, 101.0);
    assert_eq!(result.summary, "Quoted --- and escaped \"---\" stay data.");
    assert!(result.warnings.is_empty());
    let Analysis::Meeting(m) = result.analysis else { panic!("meeting") };
    assert_eq!(m.action_items.unwrap()[0].task, "Keep");
    assert_eq!(m.next_steps, Some(vec![]));
}

#[test]
fn all_falsy_and_inapplicable_sections_interrupt_without_discarding_valid_partials() {
    for tail in [
        "{\"type\":\"summary\",\"data\":false,\"items\":0,\"content\":\"\"}",
        "{\"type\":\"nextSteps\",\"data\":null,\"items\":false,\"content\":0}",
        "{\"type\":\"nextSteps\",\"data\":0,\"items\":\"\",\"content\":null}",
        "{\"type\":\"topics\",\"data\":[]}",
    ] {
        let mut p = AnalysisParser::new();
        p.push("{\"type\":\"summary\",\"content\":\"Keep\"}---{\"type\":\"actionItems\",\"data\":[{\"task\":\"Valid\"}]}---{\"type\":\"detectedType\",\"value\":\"meeting\"}---").unwrap();
        p.push(tail).unwrap();
        let error = p.finish(true).unwrap_err();
        assert_eq!(error, AnalysisError::InvalidSection);
        let partial = p.into_progress();
        assert_eq!(partial.summary.as_deref(), Some("Keep"));
        assert_eq!(partial.content_sections, 2);
        let Some(Analysis::Meeting(m)) = partial.analysis else { panic!("meeting") };
        assert_eq!(m.action_items.unwrap()[0].task, "Valid");
    }
}

#[test]
fn pretype_invalid_section_keeps_preceding_typed_partials_when_type_arrives() {
    let mut p = AnalysisParser::new();
    p.push("{\"type\":\"actionItems\",\"data\":[{\"task\":\"Retain\"}]}---{\"type\":\"highlights\",\"data\":[\"Wrong type\"]}---{\"type\":\"summary\",\"content\":\"Keep\"}---").unwrap();
    assert_eq!(p.push("{\"type\":\"detectedType\",\"value\":\"meeting\"}---"), Err(AnalysisError::InvalidSection));
    let _ = p.finish(true).unwrap_err();
    let partial = p.into_progress();
    assert_eq!(partial.summary.as_deref(), Some("Keep"));
    assert_eq!(partial.content_sections, 2);
    let Some(Analysis::Meeting(m)) = partial.analysis else { panic!("meeting") };
    assert_eq!(m.action_items.unwrap()[0].task, "Retain");
}

#[test]
fn buffered_pretype_sections_cannot_escape_document_depth_or_aggregate_limits() {
    let mut p = AnalysisParser::new();
    let section = format!("{{\"type\":\"nextSteps\",\"data\":[\"{}\"]}}---", "a".repeat(200_000));
    for _ in 0..5 { p.push(&section).unwrap(); }
    assert_eq!(p.push(&section), Err(AnalysisError::OutputTooLarge));
    assert_eq!(p.finish(true).unwrap_err(), AnalysisError::OutputTooLarge);
    assert_eq!(p.into_progress().content_sections, 0);
    let mut p = AnalysisParser::new();
    assert_eq!(p.push(&format!("{{\"type\":\"nextSteps\",\"data\":[\"{}", "a".repeat(MAX_DOCUMENT_BYTES))), Err(AnalysisError::DocumentTooLarge));
    let mut p = AnalysisParser::new();
    assert_eq!(p.push(&format!("{{\"type\":\"nextSteps\",\"data\":{}", "[".repeat(MAX_JSON_DEPTH))), Err(AnalysisError::TooDeep));
}

#[test]
fn failed_finish_retains_valid_final_document_and_is_terminal_without_duplicate_warnings() {
    let mut parser = AnalysisParser::new();
    parser.push("{\"documentCount\":9}---{\"type\":\"detectedType\",\"value\":\"meeting\"}---{\"type\":\"summary\",\"content\":\"Retain\"}---{\"type\":\"nextSteps\",\"data\":[\"Valid final section\"]}").unwrap();
    assert_eq!(parser.finish(false), Err(AnalysisError::UnsuccessfulFinish));
    assert_eq!(parser.progress().summary.as_deref(), Some("Retain"));
    assert_eq!(parser.progress().content_sections, 2);
    assert_eq!(parser.progress().warnings, [AnalysisWarning::DocumentCountMismatch { expected: 9, actual: 4 }]);
    assert_eq!(parser.finish(true), Err(AnalysisError::UnsuccessfulFinish));
    assert_eq!(parser.push("---"), Err(AnalysisError::UnsuccessfulFinish));
    let partial = parser.into_progress();
    assert_eq!(partial.warnings, [AnalysisWarning::DocumentCountMismatch { expected: 9, actual: 4 }]);
    let Some(Analysis::Meeting(m)) = partial.analysis else { panic!("meeting") };
    assert_eq!(m.next_steps.unwrap(), ["Valid final section"]);
}

#[test]
fn successful_finish_moves_result_and_rejects_reuse_instead_of_consuming_state_twice() {
    let mut parser = AnalysisParser::new();
    parser.push("{\"type\":\"detectedType\",\"value\":\"generic\"}---{\"type\":\"summary\",\"content\":\"Complete\"}").unwrap();
    assert_eq!(parser.finish(true).unwrap().summary, "Complete");
    assert_eq!(parser.progress(), &AnalysisProgress::default());
    assert_eq!(parser.finish(true), Err(AnalysisError::AlreadyFinished));
    assert_eq!(parser.push("{\"type\":\"summary\",\"content\":\"Late\"}"), Err(AnalysisError::AlreadyFinished));
}
