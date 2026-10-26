//! Independent prompt evidence: call actual product builders on canonical Lines.
//! No embedded prompt wording, networking, credentials or source-repository reads.
use std::io::Read;
use serde::{Deserialize, Serialize};
use transgate_core::ai::{analysis::{AnalysisFailure, AnalysisParser}, prompts, sanitize};
use transgate_core::transcript::Line;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: String,
    lines: Vec<Line>,
    language: Option<String>,
    #[serde(default)] detailed: bool,
    question: Option<String>,
    #[serde(default)] history: Vec<Message>,
    #[serde(default)] chunks: Vec<String>,
    #[serde(default)] successful_stop: bool,
    #[serde(default)] capture_progress: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message { role: String, content: String }
#[derive(Serialize)]
struct Report {
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis_input_estimated_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis_parse: Option<serde_json::Value>,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input_path = args.next().ok_or("usage: e4_prompts <input.json> <output.json>")?;
    let output_path = args.next().ok_or("usage: e4_prompts <input.json> <output.json>")?;
    if args.next().is_some() { return Err("usage: e4_prompts <input.json> <output.json>".into()); }
    let file = std::fs::File::open(input_path)?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 { return Err("E4 prompt input exceeds 16 MiB".into()); }
    let input: Input = serde_json::from_slice(&bytes)?;
    let mut analysis_input_estimated_tokens = None;
    let mut analysis_parse = None;
    let messages = match input.kind.as_str() {
        "analysis" | "summary" => {
            if input.kind == "analysis" {
                analysis_input_estimated_tokens = Some(prompts::analysis_input_estimated_tokens(&input.lines));
            }
            let pair = if input.kind == "analysis" { prompts::analysis_messages(&input.lines) }
                else { prompts::summary_messages(&input.lines, input.detailed) };
            vec![Message { role: "system".into(), content: pair.system },
                 Message { role: "user".into(), content: pair.user }]
        }
        "chat" => {
            let context = sanitize::input(&sanitize::transcript_text(&input.lines, true));
            let mut messages = Vec::with_capacity(input.history.len() + 2);
            messages.push(Message { role: "system".into(),
                content: prompts::chat_system_message(&context, input.language.as_deref()) });
            for message in input.history {
                if !matches!(message.role.as_str(), "user" | "assistant") {
                    return Err("E4 history requires user/assistant roles".into());
                }
                messages.push(Message { role: message.role, content: sanitize::input(&message.content) });
            }
            let question = input.question.ok_or("E4 chat question missing")?;
            messages.push(Message { role: "user".into(), content: sanitize::input(&question) });
            messages
        }
        "parse_analysis" => {
            let mut parser = AnalysisParser::new();
            let mut push_error = None;
            let mut checkpoints = Vec::new();
            for chunk in &input.chunks {
                if let Err(error) = parser.push(chunk) {
                    push_error = Some(serde_json::to_value(error)?);
                    break;
                }
                if input.capture_progress { checkpoints.push(serde_json::to_value(parser.progress())?); }
            }
            let progress = serde_json::to_value(parser.progress())?;
            let (result, failure) = match parser.finish(input.successful_stop && push_error.is_none()) {
                Ok(result) => (Some(serde_json::to_value(result)?), None),
                Err(error) => (None, Some(serde_json::to_value(AnalysisFailure {
                    error,
                    partial: parser.into_progress(),
                })?)),
            };
            analysis_parse = Some(serde_json::json!({
                "push_error": push_error, "progress": progress, "checkpoints": checkpoints,
                "result": result, "failure": failure,
            }));
            Vec::new()
        }
        _ => return Err("unknown E4 prompt kind".into()),
    };
    let file = std::fs::File::create(output_path)?;
    serde_json::to_writer(file, &Report { messages, analysis_input_estimated_tokens, analysis_parse })?;
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => { eprintln!("E4 prompt capture: {error}"); std::process::ExitCode::FAILURE }
    }
}
