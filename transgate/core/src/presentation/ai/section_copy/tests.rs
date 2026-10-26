use serde_json::json;

use super::*;

fn analysis(value: serde_json::Value) -> Analysis {
    serde_json::from_value(value).expect("valid analysis")
}

#[test]
fn action_items_copy_matches_the_source_text_and_escapes_html() {
    let a = analysis(json!({"type": "meeting", "actionItems": [
        {"task": "Ship <b>it</b>", "owner": "Ana", "deadline": "Friday", "status": "done"},
        {"task": "Review", "status": "in-progress"},
        {"task": "Plan"},
    ]}));
    let (text, html) = section_copy(&a, SectionId::ActionItems).unwrap();
    assert_eq!(text, "## Action Items\n\n[x] Ana: Ship <b>it</b> (Due: Friday)\n[-] Review\n[ ] Plan\n");
    assert!(html.contains("☑ <strong>Ana:</strong> Ship &lt;b&gt;it&lt;/b&gt; <em style=\"color: #666;\">(Due: Friday)</em>"), "{html}");
    assert!(!html.contains("<b>it</b>"), "model text never becomes clipboard markup");
}

#[test]
fn decisions_and_topics_format_source_clock_and_duration() {
    let meeting = analysis(json!({"type": "meeting", "decisions": [
        {"decision": "Go", "timestamp": 125.9, "participants": ["A", "B"]},
        {"decision": "Stop", "participants": []},
    ]}));
    assert_eq!(section_copy(&meeting, SectionId::Decisions).unwrap().0, "## Decisions Made\n\n- Go (Participants: A, B) [2:05]\n- Stop\n");

    let lecture = analysis(json!({"type": "lecture", "topics": [
        {"title": "Intro", "timestamp": 0, "duration": 45},
        {"title": "Body", "duration": 610},
    ]}));
    let (text, html) = section_copy(&lecture, SectionId::Topics).unwrap();
    assert_eq!(text, "## Topics Covered\n\n1. Intro [0:00] (45s)\n2. Body (10 min)\n");
    assert!(html.contains("Starts: 0:00 | Duration: 45s") && html.contains("Duration: 10 min"), "{html}");
}

#[test]
fn meeting_lists_join_without_a_trailing_newline_as_the_source_does() {
    let a = analysis(json!({"type": "meeting", "discussionPoints": ["One", "Two"], "nextSteps": ["Do", "Done"]}));
    assert_eq!(section_copy(&a, SectionId::DiscussionPoints).unwrap(), (
        "## Discussion Points\n\n- One\n- Two".to_string(),
        "<h2>Discussion Points</h2><ul><li>One</li><li>Two</li></ul>".to_string(),
    ));
    assert_eq!(section_copy(&a, SectionId::NextSteps).unwrap().0, "## Next Steps\n\n1. Do\n2. Done");
}

#[test]
fn ratings_truncate_fractional_stars_like_string_repeat() {
    let a = analysis(json!({"type": "interview",
        "assessment": {"position": "Engineer", "overallRating": 3.5, "strengths": ["Clear"], "concerns": ["Slow"]},
        "skillsAssessment": [{"skill": "Rust", "rating": 4, "level": "Advanced"}],
    }));
    assert_eq!(
        section_copy(&a, SectionId::CandidateAssessment).unwrap().0,
        "## Candidate Assessment\n\nPosition: Engineer\nOverall Rating: ★★★☆ (3.5/5)\n\nStrengths:\n- ✓ Clear\n\nConcerns:\n- ⚠ Slow\n"
    );
    assert_eq!(section_copy(&a, SectionId::SkillsAssessment).unwrap().0, "## Skills Assessment\n\n- Rust: Advanced (★★★★☆)\n");
}

#[test]
fn grouped_sections_keep_the_source_separators() {
    let sales = analysis(json!({"type": "sales",
        "customerInsights": {"painPoints": ["Cost"], "buyingSignals": ["Asked price"]},
        "coachingTips": [{"category": "improvement", "tip": "Listen"}, {"category": "strength", "tip": "Rapport"}],
        "callMetrics": {"questionsAsked": 4, "nextStepsConfirmed": false},
    }));
    assert_eq!(
        section_copy(&sales, SectionId::CustomerInsights).unwrap().0,
        "## Customer Insights\n\n### Pain Points\n- Cost\n\n### Buying Signals\n- Asked price\n"
    );
    assert_eq!(
        section_copy(&sales, SectionId::CoachingTips).unwrap().0,
        "## Coaching Tips\n\n### Strengths\n- Rapport\n\n### Areas for Improvement\n- Listen\n"
    );
    assert_eq!(section_copy(&sales, SectionId::CallMetrics).unwrap().0, "## Call Metrics\n\n- Questions Asked: 4\n- Next Steps Confirmed: No");
}

#[test]
fn interview_steps_and_responses_use_source_labels() {
    let a = analysis(json!({"type": "interview",
        "notableResponses": [{"question": "Why?", "answer": "Because", "assessment": "negative"}],
        "nextSteps": [{"action": "Call back", "priority": "medium"}, {"action": "Offer", "priority": "high"}, {"action": "Reference", "priority": "medium"}],
    }));
    assert_eq!(section_copy(&a, SectionId::NotableResponses).unwrap().0, "## Notable Responses\n\n### Q: Why?\nA: Because\nAssessment: 👎 negative\n\n");
    assert_eq!(
        section_copy(&a, SectionId::InterviewNextSteps).unwrap().0,
        "## Recommended Next Steps\n\n- [HIGH] Offer\n- [MEDIUM] Call back\n- [MEDIUM] Reference\n",
        "sorted high to low, stable within a priority"
    );
}

#[test]
fn absent_empty_or_foreign_sections_have_nothing_to_copy() {
    let support = analysis(json!({"type": "support", "kbUpdates": [], "followUpActions": ["Email"], "insights": {}}));
    assert_eq!(section_copy(&support, SectionId::KbUpdates), None, "an empty list renders no section");
    assert_eq!(section_copy(&support, SectionId::SupportInsights), None);
    assert_eq!(section_copy(&support, SectionId::FollowUpActions).unwrap().0, "## Follow-Up Actions\n\n- [ ] Email\n");
    assert_eq!(section_copy(&support, SectionId::ActionItems), None, "a meeting section never copies from a support analysis");
}
