//! The copy button on each specialized section header (`SectionHeader`
//! `onCopy`): plain text plus rich HTML, formatted exactly as the source's
//! per-section `format*AsText`/`format*AsHtml` helpers. One deliberate change:
//! model text is HTML-escaped, where the source interpolated it raw into the
//! clipboard markup.
//!
//! Sections the source renders without a copy button (generic highlights and
//! discussion points, support issue resolution and customer sentiment) have no
//! id here.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::ai::analysis::{
    ActionStatus, Analysis, Assessment, CoachingCategory, InterviewNextStep, ResponseAssessment, SkillLevel, Priority,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SectionId {
    ActionItems,
    Decisions,
    DiscussionPoints,
    NextSteps,
    Topics,
    Concepts,
    ReviewQuestions,
    StudyGuide,
    CallMetrics,
    CoachingTips,
    CustomerInsights,
    SalesNextSteps,
    KbUpdates,
    FollowUpActions,
    SupportInsights,
    CandidateAssessment,
    SkillsAssessment,
    NotableResponses,
    InterviewNextSteps,
}

const DIV: &str = "<div style=\"font-family: -apple-system, BlinkMacSystemFont, sans-serif;\">";
const H2: &str = "<h2 style=\"font-size: 1.2em; margin-bottom: 0.5em;\">";

/// `(text, html)` for a section that is displayed, or `None` when the section
/// is absent or empty and so has no button.
pub fn section_copy(analysis: &Analysis, section: SectionId) -> Option<(String, String)> {
    use SectionId as S;
    match (analysis, section) {
        (Analysis::Meeting(m), S::ActionItems) => {
            let items = nonempty(&m.action_items)?;
            let mut text = String::from("## Action Items\n\n");
            let mut html = format!("{DIV}{H2}Action Items</h2><ul style=\"margin: 0; padding-left: 1.5em;\">");
            for item in items {
                let (box_text, box_html) = match item.status {
                    ActionStatus::Done => ("[x]", "☑"),
                    ActionStatus::InProgress => ("[-]", "◐"),
                    ActionStatus::Pending => ("[ ]", "☐"),
                };
                text.push_str(box_text);
                text.push(' ');
                let _ = write!(html, "<li style=\"margin-bottom: 0.3em;\">{box_html} ");
                if let Some(owner) = &item.owner {
                    let _ = write!(text, "{owner}: ");
                    let _ = write!(html, "<strong>{}:</strong> ", esc(owner));
                }
                text.push_str(&item.task);
                html.push_str(&esc(&item.task));
                if let Some(deadline) = &item.deadline {
                    let _ = write!(text, " (Due: {deadline})");
                    let _ = write!(html, " <em style=\"color: #666;\">(Due: {})</em>", esc(deadline));
                }
                text.push('\n');
                html.push_str("</li>");
            }
            html.push_str("</ul></div>");
            Some((text, html))
        }
        (Analysis::Meeting(m), S::Decisions) => {
            let items = nonempty(&m.decisions)?;
            let mut text = String::from("## Decisions Made\n\n");
            let mut html = format!("{DIV}{H2}Decisions Made</h2><ul style=\"margin: 0; padding-left: 1.5em;\">");
            for item in items {
                let _ = write!(text, "- {}", item.decision);
                let _ = write!(html, "<li style=\"margin-bottom: 0.5em;\"><strong>{}</strong>", esc(&item.decision));
                if let Some(people) = item.participants.as_ref().filter(|p| !p.is_empty()) {
                    let joined = people.join(", ");
                    let _ = write!(text, " (Participants: {joined})");
                    let _ = write!(html, "<br/><em style=\"color: #666; font-size: 0.9em;\">Participants: {}</em>", esc(&joined));
                }
                if let Some(at) = item.timestamp {
                    let _ = write!(text, " [{}]", clock(at));
                    let _ = write!(html, "<br/><em style=\"color: #888; font-size: 0.9em;\">At: {}</em>", clock(at));
                }
                text.push('\n');
                html.push_str("</li>");
            }
            html.push_str("</ul></div>");
            Some((text, html))
        }
        (Analysis::Meeting(m), S::DiscussionPoints) => {
            let items = nonempty(&m.discussion_points)?;
            let text = format!("## Discussion Points\n\n{}", items.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n"));
            let html = format!("<h2>Discussion Points</h2><ul>{}</ul>", items.iter().map(|p| format!("<li>{}</li>", esc(p))).collect::<String>());
            Some((text, html))
        }
        (Analysis::Meeting(m), S::NextSteps) => {
            let items = nonempty(&m.next_steps)?;
            let text = format!(
                "## Next Steps\n\n{}",
                items.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n")
            );
            let html = format!("<h2>Next Steps</h2><ol>{}</ol>", items.iter().map(|s| format!("<li>{}</li>", esc(s))).collect::<String>());
            Some((text, html))
        }
        (Analysis::Lecture(l), S::Topics) => {
            let items = nonempty(&l.topics)?;
            let mut text = String::from("## Topics Covered\n\n");
            let mut html = format!("{DIV}{H2}Topics Covered</h2><ol style=\"margin: 0; padding-left: 1.5em;\">");
            for (i, topic) in items.iter().enumerate() {
                let _ = write!(text, "{}. {}", i + 1, topic.title);
                if let Some(at) = topic.timestamp {
                    let _ = write!(text, " [{}]", clock(at));
                }
                if let Some(d) = topic.duration {
                    let _ = write!(text, " ({})", duration(d));
                }
                text.push('\n');
                let _ = write!(html, "<li style=\"margin-bottom: 0.5em;\"><strong>{}</strong>", esc(&topic.title));
                if topic.timestamp.is_some() || topic.duration.is_some() {
                    html.push_str("<br/><em style=\"color: #666; font-size: 0.9em;\">");
                    if let Some(at) = topic.timestamp {
                        let _ = write!(html, "Starts: {}", clock(at));
                    }
                    if let Some(d) = topic.duration {
                        if topic.timestamp.is_some() {
                            html.push_str(" | ");
                        }
                        let _ = write!(html, "Duration: {}", duration(d));
                    }
                    html.push_str("</em>");
                }
                html.push_str("</li>");
            }
            html.push_str("</ol></div>");
            Some((text, html))
        }
        (Analysis::Lecture(l), S::Concepts) => {
            let items = nonempty(&l.concepts)?;
            let mut text = String::from("## Key Concepts\n\n");
            let mut html = format!("{DIV}{H2}Key Concepts</h2>");
            for item in items {
                let _ = write!(text, "### {}\n{}\n", item.concept, item.explanation);
                html.push_str("<div style=\"margin-bottom: 1em; padding: 0.5em; border: 1px solid #ddd; border-radius: 4px;\">");
                let _ = write!(html, "<h3 style=\"font-size: 1em; margin: 0 0 0.5em 0;\">{}</h3>", esc(&item.concept));
                let _ = write!(html, "<p style=\"margin: 0 0 0.5em 0; color: #666;\">{}</p>", esc(&item.explanation));
                if let Some(examples) = item.examples.as_ref().filter(|e| !e.is_empty()) {
                    let joined = examples.join(", ");
                    let _ = writeln!(text, "Examples: {joined}");
                    let _ = write!(html, "<p style=\"margin: 0; font-size: 0.9em;\"><strong>Examples:</strong> {}</p>", esc(&joined));
                }
                text.push('\n');
                html.push_str("</div>");
            }
            html.push_str("</div>");
            Some((text, html))
        }
        (Analysis::Lecture(l), S::ReviewQuestions) => numbered("Review Questions", nonempty(&l.review_questions)?),
        (Analysis::Lecture(l), S::StudyGuide) => bulleted("Study Guide", nonempty(&l.study_guide)?, "- ", "📚 "),
        (Analysis::Sales(s), S::CallMetrics) => {
            let m = s.call_metrics.as_ref()?;
            let mut rows = Vec::new();
            if let Some(ratio) = m.talk_listen_ratio.as_deref().filter(|r| !r.is_empty()) {
                rows.push(format!("Talk:Listen Ratio: {ratio}"));
            }
            if let Some(n) = m.questions_asked {
                rows.push(format!("Questions Asked: {}", number(n)));
            }
            if let Some(n) = m.objections_handled {
                rows.push(format!("Objections Handled: {}", number(n)));
            }
            if let Some(yes) = m.next_steps_confirmed {
                rows.push(format!("Next Steps Confirmed: {}", if yes { "Yes" } else { "No" }));
            }
            if rows.is_empty() {
                return None;
            }
            let text = std::iter::once("## Call Metrics\n".to_string()).chain(rows.iter().map(|r| format!("- {r}"))).collect::<Vec<_>>().join("\n");
            let html = format!(
                "{DIV}{H2}Call Metrics</h2><ul style=\"margin: 0; padding-left: 1.5em;\">{}</ul></div>",
                rows.iter().map(|r| format!("<li>{}</li>", esc(r))).collect::<String>()
            );
            Some((text, html))
        }
        (Analysis::Sales(s), S::CoachingTips) => {
            let tips = nonempty(&s.coaching_tips)?;
            let strengths: Vec<&str> = tips.iter().filter(|t| t.category == CoachingCategory::Strength).map(|t| t.tip.as_str()).collect();
            let improvements: Vec<&str> = tips.iter().filter(|t| t.category == CoachingCategory::Improvement).map(|t| t.tip.as_str()).collect();
            Some(two_lists("Coaching Tips", &strengths, &improvements))
        }
        (Analysis::Sales(s), S::CustomerInsights) => {
            let c = s.customer_insights.as_ref()?;
            let groups = [
                ("Pain Points", &c.pain_points, "#dc2626"),
                ("Budget Signals", &c.budget_signals, "#2563eb"),
                ("Objections", &c.objections, "#d97706"),
                ("Buying Signals", &c.buying_signals, "#16a34a"),
            ];
            let present: Vec<_> = groups.iter().filter_map(|(title, items, color)| nonempty(items).map(|i| (*title, i, *color))).collect();
            if present.is_empty() {
                return None;
            }
            let mut text = String::from("## Customer Insights\n\n");
            let mut html = format!("{DIV}{H2}Customer Insights</h2>");
            for (title, items, color) in &present {
                let _ = writeln!(text, "### {title}");
                for item in *items {
                    let _ = writeln!(text, "- {item}");
                }
                // The source ends every group with a blank line except buying signals.
                if *title != "Buying Signals" {
                    text.push('\n');
                }
                let _ = write!(html, "<h3 style=\"font-size: 1em; color: {color}; margin-bottom: 0.3em;\">{title}</h3><ul style=\"margin: 0 0 1em 0; padding-left: 1.5em;\">");
                for item in *items {
                    let _ = write!(html, "<li style=\"margin-bottom: 0.3em;\">{}</li>", esc(item));
                }
                html.push_str("</ul>");
            }
            html.push_str("</div>");
            Some((text, html))
        }
        (Analysis::Sales(s), S::SalesNextSteps) => numbered("Recommended Next Steps", nonempty(&s.next_steps)?),
        (Analysis::Support(s), S::KbUpdates) => bulleted("Knowledge Base Updates", nonempty(&s.kb_updates)?, "- ", "📝 "),
        (Analysis::Support(s), S::FollowUpActions) => bulleted("Follow-Up Actions", nonempty(&s.follow_up_actions)?, "- [ ] ", "☐ "),
        (Analysis::Support(s), S::SupportInsights) => {
            let i = s.insights.as_ref()?;
            let strengths: Vec<&str> = i.strengths.iter().flatten().map(String::as_str).collect();
            let improvements: Vec<&str> = i.improvements.iter().flatten().map(String::as_str).collect();
            if strengths.is_empty() && improvements.is_empty() {
                return None;
            }
            Some(two_lists("Agent Performance", &strengths, &improvements))
        }
        (Analysis::Interview(v), S::CandidateAssessment) => assessment(v.assessment.as_ref()?),
        (Analysis::Interview(v), S::SkillsAssessment) => {
            let items = nonempty(&v.skills_assessment)?;
            let mut text = String::from("## Skills Assessment\n\n");
            let mut html = format!(
                "{DIV}{H2}Skills Assessment</h2><table style=\"border-collapse: collapse; width: 100%;\"><tr><th style=\"text-align: left; padding: 0.5em;\">Skill</th><th style=\"text-align: left; padding: 0.5em;\">Level</th><th style=\"text-align: left; padding: 0.5em;\">Rating</th></tr>"
            );
            for skill in items {
                let stars = stars(skill.rating.value());
                let _ = writeln!(text, "- {}: {} ({stars})", skill.skill, level(skill.level));
                let _ = write!(
                    html,
                    "<tr><td style=\"padding: 0.5em;\">{}</td><td style=\"padding: 0.5em;\">{}</td><td style=\"padding: 0.5em; color: #f59e0b;\">{stars}</td></tr>",
                    esc(&skill.skill),
                    level(skill.level)
                );
            }
            html.push_str("</table></div>");
            Some((text, html))
        }
        (Analysis::Interview(v), S::NotableResponses) => {
            let items = nonempty(&v.notable_responses)?;
            let mut text = String::from("## Notable Responses\n\n");
            let mut html = format!("{DIV}{H2}Notable Responses</h2>");
            for r in items {
                let (icon, word) = match r.assessment {
                    ResponseAssessment::Positive => ("👍", "positive"),
                    ResponseAssessment::Neutral => ("➖", "neutral"),
                    ResponseAssessment::Negative => ("👎", "negative"),
                };
                let _ = write!(text, "### Q: {}\nA: {}\nAssessment: {icon} {word}\n\n", r.question, r.answer);
                html.push_str("<div style=\"margin-bottom: 1em; padding: 0.5em; border-left: 3px solid #ddd;\">");
                let _ = write!(html, "<p style=\"margin: 0 0 0.5em 0; font-weight: bold; color: #666;\">Q: {}</p>", esc(&r.question));
                let _ = write!(html, "<p style=\"margin: 0 0 0.5em 0;\">A: {}</p>", esc(&r.answer));
                let _ = write!(html, "<p style=\"margin: 0; font-size: 0.9em; color: #888;\">{icon} {word}</p></div>");
            }
            html.push_str("</div>");
            Some((text, html))
        }
        (Analysis::Interview(v), S::InterviewNextSteps) => {
            let items = by_priority(nonempty(&v.next_steps)?);
            let mut text = String::from("## Recommended Next Steps\n\n");
            let mut html = format!("{DIV}{H2}Recommended Next Steps</h2><ul style=\"margin: 0; padding-left: 1.5em;\">");
            for step in &items {
                let (label, color) = match step.priority {
                    Priority::High => ("HIGH", "#dc2626"),
                    Priority::Medium => ("MEDIUM", "#d97706"),
                    Priority::Low => ("LOW", "#6b7280"),
                };
                let _ = writeln!(text, "- [{label}] {}", step.action);
                let _ = write!(
                    html,
                    "<li style=\"margin-bottom: 0.5em;\"><span style=\"color: {color}; font-weight: bold;\">[{label}]</span> {}</li>",
                    esc(&step.action)
                );
            }
            html.push_str("</ul></div>");
            Some((text, html))
        }
        _ => None,
    }
}

fn nonempty<T>(items: &Option<Vec<T>>) -> Option<&[T]> {
    items.as_deref().filter(|i| !i.is_empty())
}

/// The source shows and copies interview next steps high, medium, then low,
/// keeping the model's order within a priority (a stable sort).
pub fn by_priority(steps: &[InterviewNextStep]) -> Vec<InterviewNextStep> {
    let rank = |p: Priority| match p {
        Priority::High => 0,
        Priority::Medium => 1,
        Priority::Low => 2,
    };
    let mut sorted = steps.to_vec();
    sorted.sort_by_key(|s| rank(s.priority));
    sorted
}

fn numbered(title: &str, items: &[String]) -> Option<(String, String)> {
    let mut text = format!("## {title}\n\n");
    let mut html = format!("{DIV}{H2}{title}</h2><ol style=\"margin: 0; padding-left: 1.5em;\">");
    for (i, item) in items.iter().enumerate() {
        let _ = writeln!(text, "{}. {item}", i + 1);
        let _ = write!(html, "<li style=\"margin-bottom: 0.5em;\">{}</li>", esc(item));
    }
    html.push_str("</ol></div>");
    Some((text, html))
}

fn bulleted(title: &str, items: &[String], text_mark: &str, html_mark: &str) -> Option<(String, String)> {
    let mut text = format!("## {title}\n\n");
    let mut html = format!("{DIV}{H2}{title}</h2><ul style=\"margin: 0; padding-left: 1.5em;\">");
    for item in items {
        let _ = writeln!(text, "{text_mark}{item}");
        let _ = write!(html, "<li style=\"margin-bottom: 0.5em;\">{html_mark}{}</li>", esc(item));
    }
    html.push_str("</ul></div>");
    Some((text, html))
}

/// Coaching tips and agent performance: strengths, then areas for improvement.
fn two_lists(title: &str, strengths: &[&str], improvements: &[&str]) -> (String, String) {
    let mut text = format!("## {title}\n\n");
    let mut html = format!("{DIV}{H2}{title}</h2>");
    if !strengths.is_empty() {
        text.push_str("### Strengths\n");
        html.push_str("<h3 style=\"font-size: 1em; color: #16a34a; margin-bottom: 0.3em;\">Strengths</h3><ul style=\"margin: 0 0 1em 0; padding-left: 1.5em;\">");
        for s in strengths {
            let _ = writeln!(text, "- {s}");
            let _ = write!(html, "<li style=\"margin-bottom: 0.3em;\">✓ {}</li>", esc(s));
        }
        text.push('\n');
        html.push_str("</ul>");
    }
    if !improvements.is_empty() {
        text.push_str("### Areas for Improvement\n");
        html.push_str("<h3 style=\"font-size: 1em; color: #d97706; margin-bottom: 0.3em;\">Areas for Improvement</h3><ul style=\"margin: 0 0 1em 0; padding-left: 1.5em;\">");
        for s in improvements {
            let _ = writeln!(text, "- {s}");
            let _ = write!(html, "<li style=\"margin-bottom: 0.3em;\">→ {}</li>", esc(s));
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");
    (text, html)
}

fn assessment(a: &Assessment) -> Option<(String, String)> {
    let strengths = nonempty(&a.strengths);
    let concerns = nonempty(&a.concerns);
    if a.position.is_none() && a.overall_rating.is_none() && strengths.is_none() && concerns.is_none() {
        return None;
    }
    let mut text = String::from("## Candidate Assessment\n\n");
    let mut html = String::from("<div style=\"font-family: -apple-system, sans-serif;\"><h2>Candidate Assessment</h2>");
    if let Some(position) = a.position.as_deref().filter(|p| !p.is_empty()) {
        let _ = writeln!(text, "Position: {position}");
        let _ = write!(html, "<p><strong>Position:</strong> {}</p>", esc(position));
    }
    if let Some(rating) = a.overall_rating {
        let r = rating.value();
        let _ = writeln!(text, "Overall Rating: {} ({}/5)", stars(r), number(r));
        let _ = write!(html, "<p><strong>Rating:</strong> {} ({}/5)</p>", stars(r), number(r));
    }
    if let Some(items) = strengths {
        text.push_str("\nStrengths:\n");
        html.push_str("<h3 style=\"color:#16a34a\">Strengths</h3><ul>");
        for s in items {
            let _ = writeln!(text, "- ✓ {s}");
            let _ = write!(html, "<li>✓ {}</li>", esc(s));
        }
        html.push_str("</ul>");
    }
    if let Some(items) = concerns {
        text.push_str("\nConcerns:\n");
        html.push_str("<h3 style=\"color:#d97706\">Concerns</h3><ul>");
        for c in items {
            let _ = writeln!(text, "- ⚠ {c}");
            let _ = write!(html, "<li>⚠ {}</li>", esc(c));
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");
    Some((text, html))
}

fn level(l: SkillLevel) -> &'static str {
    match l {
        SkillLevel::Beginner => "Beginner",
        SkillLevel::Intermediate => "Intermediate",
        SkillLevel::Advanced => "Advanced",
        SkillLevel::Expert => "Expert",
    }
}

/// `'★'.repeat(r) + '☆'.repeat(5 - r)`: `repeat` truncates fractional counts.
fn stars(rating: f64) -> String {
    let full = rating.trunc().clamp(0.0, 5.0) as usize;
    let empty = (5.0 - rating).trunc().clamp(0.0, 5.0) as usize;
    format!("{}{}", "★".repeat(full), "☆".repeat(empty))
}

/// A JavaScript number's default display for the values these sections hold.
fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

/// The source's `m:ss` from seconds (`Math.floor` of minutes and of `s % 60`).
fn clock(seconds: f64) -> String {
    let mins = (seconds / 60.0).floor() as i64;
    let secs = (seconds % 60.0).floor() as i64;
    format!("{mins}:{secs:02}")
}

/// `formatDuration`: seconds under a minute, otherwise whole minutes.
fn duration(seconds: f64) -> String {
    if seconds < 60.0 { format!("{}s", number(seconds)) } else { format!("{} min", (seconds / 60.0).floor() as i64) }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
