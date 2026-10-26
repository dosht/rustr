//! Literal source prompts, including generated schema documentation from
//! schema-to-prompt.ts:240-338. Analysis and general summary ignore language;
//! chat alone appends the exact source locale-name instruction.
use crate::transcript::Line;
use super::sanitize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptMessages {
    pub system: String,
    pub user: String,
}

pub const ANALYSIS_SYSTEM: &str = "You are an expert transcript analyst. Your task is to:\n1. Identify the type of transcript (lecture, meeting, sales call, customer support call, or interview)\n2. Extract specialized insights appropriate for that type\n3. Return the results in a multi-document JSON format for progressive rendering\n\nIMPORTANT: You MUST return valid JSON objects. Do NOT return YAML.\n\nBe objective, accurate, and concise. Only include sections that are relevant to the transcript content.\nIf a section doesn't apply to the transcript, omit it entirely.";

pub const ANALYSIS_SCHEMA_DOCUMENTATION: &str = r#"**MEETING:**
- type: actionItems, data: Array of {task: string, owner?: string, deadline?: string, status: "pending"|"in-progress"|"done" (default: "pending")}
- type: decisions, data: Array of {decision: string, timestamp?: number, participants?: string[]}
- type: discussionPoints, data: Array of strings
- type: nextSteps, data: Array of strings

**LECTURE:**
- type: topics, data: Array of {title: string, timestamp?: number, duration?: number}
- type: concepts, data: Array of {concept: string, explanation: string, examples?: string[]}
- type: reviewQuestions, data: Array of strings
- type: studyGuide, data: Array of strings

**SALES:**
- type: callMetrics, data: {talkListenRatio?: string, questionsAsked?: number, objectionsHandled?: number, nextStepsConfirmed?: boolean}
- type: coachingTips, data: Array of {category: "strength"|"improvement", tip: string}
- type: customerInsights, data: {painPoints?: string[], budgetSignals?: string[], objections?: string[], buyingSignals?: string[]}
- type: nextSteps, data: Array of strings

**SUPPORT:**
- type: issueResolution, data: {problem: string, rootCause?: string, resolution?: string, status: "resolved"|"unresolved"|"escalated", resolutionTimeMinutes?: number}
- type: sentiment, data: {initial: string, final: string, keyMoments?: Array}
- type: insights, data: {strengths?: string[], improvements?: string[]}
- type: kbUpdates, data: Array of strings
- type: followUpActions, data: Array of strings

**INTERVIEW:**
- type: assessment, data: {position?: string, overallRating?: number (min: 1, max: 5), strengths?: string[], concerns?: string[]}
- type: skillsAssessment, data: Array of {skill: string, rating: number (min: 1, max: 5), level: "Beginner"|"Intermediate"|"Advanced"|"Expert"}
- type: notableResponses, data: Array of {question: string, answer: string, assessment: "positive"|"neutral"|"negative"}
- type: nextSteps, data: Array of {action: string, priority: "high"|"medium"|"low"}

**GENERIC:**
- type: summary, data: string
- type: highlights, data: Array of strings"#;

const ANALYSIS_PREFIX: &str = r#"Analyze the following transcript and return specialized insights in multi-document JSON format.

IMPORTANT: Your response MUST be valid JSON objects separated by "---". Do NOT use YAML format.

Format your response exactly like this example:

{"documentCount": 5}
---
{"type": "detectedType", "value": "meeting", "confidence": 95}
---
{"type": "summary", "content": "This was a team meeting discussing..."}
---
{"type": "actionItems", "data": [{"task": "Review proposal", "owner": "John", "status": "pending"}]}
---
{"type": "decisions", "data": [{"decision": "Approved budget increase"}]}

Rules:
1. First line must be a JSON object with "documentCount" (total number of documents including this one)
2. Separate each JSON document with "---" on its own line
3. Each document must be a valid JSON object with "type" field
4. Use "value" for detectedType, "content" for summary, "data" for all other types

For the "detectedType" document:
- value: one of "lecture", "meeting", "sales", "support", "interview", "generic"
- confidence: number from 0 to 100

For the "summary" document:
- content: concise paragraph under 300 words

For specialized documents based on detected type, include these (only if relevant):

"#;

const ANALYSIS_BEFORE_TRANSCRIPT: &str = "\n\nOnly include documents that are relevant to the transcript. If a section doesn't apply, omit it entirely.\n\nTranscript:\n";
const ANALYSIS_SUFFIX: &str = "\n\nRemember: Return valid JSON objects only. Each line with \"---\" separates documents. Start with documentCount.";

/// Baseline source template. The host/adapter chooses the named D1 omission of
/// response_format separately; prompt bytes do not change with that correction.
pub fn analysis_messages(lines: &[Line]) -> PromptMessages {
    let mut user = String::with_capacity(ANALYSIS_PREFIX.len() + ANALYSIS_SCHEMA_DOCUMENTATION.len() + ANALYSIS_BEFORE_TRANSCRIPT.len() + ANALYSIS_SUFFIX.len());
    user.push_str(ANALYSIS_PREFIX);
    user.push_str(ANALYSIS_SCHEMA_DOCUMENTATION);
    user.push_str(ANALYSIS_BEFORE_TRANSCRIPT);
    for (index, line) in lines.iter().enumerate() {
        if index != 0 { user.push('\n'); }
        user.push_str(line.speaker.as_deref().filter(|s| !s.is_empty()).unwrap_or("Speaker"));
        user.push_str(": ");
        user.push_str(&line.line);
    }
    user.push_str(ANALYSIS_SUFFIX);
    PromptMessages { system: ANALYSIS_SYSTEM.to_owned(), user }
}

/// D8 correction to analyze.tokensUsed: estimate the actual submitted canonical
/// speaker/line transcript, not the legacy route's nonexistent text columns.
/// JavaScript Math.round(nonnegative UTF-16 units / 4), not chat's ceiling.
/// Keep all raw lines, fallback speakers and newline separators as above.
pub fn analysis_input_estimated_tokens(lines: &[Line]) -> usize {
    let units = lines.iter().map(|line| {
        line.speaker.as_deref().filter(|s| !s.is_empty()).unwrap_or("Speaker").encode_utf16().count()
            + 2 + line.line.encode_utf16().count()
    }).sum::<usize>() + lines.len().saturating_sub(1);
    units / 4 + usize::from(units % 4 >= 2)
}

pub const SUMMARY_SYSTEM: &str = "You are a helpful assistant that creates concise, accurate summaries of transcript content.";

/// General/detailed summary uses sanitized space-joined content. No language
/// parameter: the legacy summary path accepts but does not use that option.
pub fn summary_messages(lines: &[Line], detailed: bool) -> PromptMessages {
    let content = sanitize::input(&sanitize::transcript_text(lines, true));
    let style = if detailed { "detailed" } else { "concise" };
    PromptMessages {
        system: SUMMARY_SYSTEM.to_owned(),
        user: format!("Please create a {style} summary of the following transcript. Keep it under 300 words.\n\nTranscript:\n{content}\n\nSummary:"),
    }
}

/// Context is already sanitized by the chat admission use case. Caller history
/// remains ordered (including explicit retry's repeated question) outside here.
pub fn chat_system_message(sanitized_context: &str, language: Option<&str>) -> String {
    let language_instruction = match language.filter(|l| !l.is_empty() && *l != "en") {
        Some(code) => format!(" Please respond in {}.", language_name(code)),
        None => String::new(),
    };
    format!("You are an AI assistant that helps users understand transcript content. You should only answer questions related to the provided transcript context. If a user asks about something not related to the transcript, politely redirect them to ask about the transcript content.{language_instruction}\n\nKey instructions:\n- Only answer questions about the transcript content provided\n- If asked about unrelated topics, politely redirect to transcript-related questions\n- Be helpful and informative about the transcript content\n- Keep responses concise but comprehensive\n- If you're unsure if something is in the transcript, say so\n\nTranscript context:\n{sanitized_context}")
}

fn language_name(code: &str) -> &str {
    match code {
        "af" => "Afrikaans", "ar" => "Arabic", "hy" => "Armenian", "az" => "Azerbaijani",
        "be" => "Belarusian", "bs" => "Bosnian", "bg" => "Bulgarian", "ca" => "Catalan",
        "zh" => "Chinese", "hr" => "Croatian", "cs" => "Czech", "da" => "Danish",
        "nl" => "Dutch", "en" => "English", "et" => "Estonian", "fi" => "Finnish",
        "fr" => "French", "gl" => "Galician", "de" => "German", "el" => "Greek",
        "he" => "Hebrew", "hi" => "Hindi", "hu" => "Hungarian", "is" => "Icelandic",
        "id" => "Indonesian", "it" => "Italian", "ja" => "Japanese", "kn" => "Kannada",
        "kk" => "Kazakh", "ko" => "Korean", "lv" => "Latvian", "lt" => "Lithuanian",
        "mk" => "Macedonian", "ms" => "Malay", "mr" => "Marathi", "mi" => "Maori",
        "ne" => "Nepali", "no" => "Norwegian", "fa" => "Persian", "pl" => "Polish",
        "pt" => "Portuguese", "ro" => "Romanian", "ru" => "Russian", "sr" => "Serbian",
        "sk" => "Slovak", "sl" => "Slovenian", "es" => "Spanish", "sw" => "Swahili",
        "sv" => "Swedish", "tl" => "Tagalog", "ta" => "Tamil", "th" => "Thai",
        "tr" => "Turkish", "uk" => "Ukrainian", "ur" => "Urdu", "vi" => "Vietnamese",
        "cy" => "Welsh", other => other,
    }
}
