//! Loopback-only E4 report runner. Provider errors are report data, not process
//! failures; invalid input or failure to save a report exits nonzero.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::Read;
    use llm_core::{CompletionEvent, CompletionRequest, ErrorKind, FinishReason, Llm, LlmError, Message, RemoteOutcome, ResponseFormat, Role};
    use llm_http::{DispatchCapacity, HttpLlm, ProviderConfig};
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input { provider: String, base_url: String, model: String, messages: Vec<InputMessage>, response_format: Option<String>, api_key: String }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InputMessage { role: String, content: String }
    #[derive(Default, Serialize)]
    struct Report { content: String, terminal: bool, finish_reason: Option<String>, error: Option<ErrorReport> }
    #[derive(Serialize)]
    struct ErrorReport { kind: &'static str, remote: &'static str, status: Option<u16>, retry_after: Option<String> }
    fn report_error(error: LlmError) -> ErrorReport {
        let (kind, status, retry_after) = match error.kind {
            ErrorKind::Configuration => ("configuration", None, None), ErrorKind::InvalidRequest => ("invalid_request", None, None),
            ErrorKind::Capacity => ("capacity", None, None), ErrorKind::Http { status, retry_after } => ("http", Some(status), retry_after),
            ErrorKind::Transport => ("transport", None, None), ErrorKind::Timeout => ("timeout", None, None),
            ErrorKind::Cancelled => ("cancelled", None, None), ErrorKind::Protocol => ("protocol", None, None), ErrorKind::Limit => ("limit", None, None),
        };
        let remote = match error.remote { RemoteOutcome::NotDispatched => "not_dispatched", RemoteOutcome::Rejected => "rejected", RemoteOutcome::Unknown => "unknown", RemoteOutcome::Finished => "finished" };
        ErrorReport { kind, remote, status, retry_after }
    }
    pub async fn run() -> Result<(), &'static str> {
        let mut args = std::env::args_os().skip(1);
        let input_path = args.next().ok_or("usage: e4_capture <input.json> <output.json>")?;
        let output_path = args.next().ok_or("usage: e4_capture <input.json> <output.json>")?;
        if args.next().is_some() { return Err("usage: e4_capture <input.json> <output.json>"); }
        let file = std::fs::File::open(input_path).map_err(|_| "cannot read E4 input")?;
        let mut bytes = Vec::new(); file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "cannot read E4 input")?;
        if bytes.len() > 16 * 1024 * 1024 { return Err("E4 input exceeds 16 MiB"); }
        let input: Input = serde_json::from_slice(&bytes).map_err(|_| "invalid E4 input")?;
        let url = reqwest::Url::parse(&input.base_url).map_err(|_| "invalid E4 URL")?;
        if url.scheme() != "http" || !url.host_str().and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok()).is_some_and(|ip| ip.is_loopback()) {
            return Err("E4 example accepts loopback HTTP only");
        }
        if !input.api_key.starts_with("e4-") { return Err("E4 example requires a synthetic e4- credential"); }
        let messages = input.messages.into_iter().map(|m| {
            let role = match m.role.as_str() { "system" => Role::System, "user" => Role::User, "assistant" => Role::Assistant, _ => return Err("invalid E4 role") };
            Ok(Message { role, content: m.content })
        }).collect::<Result<Vec<_>, _>>()?;
        let mut request = CompletionRequest::new(input.model, messages);
        request.response_format = match input.response_format.as_deref() { None => None, Some("json_object") => Some(ResponseFormat::JsonObject), _ => return Err("invalid E4 response format") };
        let config = ProviderConfig::new(input.base_url, input.api_key);
        let adapter = match input.provider.as_str() {
            "deepseek" => HttpLlm::deepseek(config, DispatchCapacity::default()),
            "openai" => HttpLlm::openai(config, DispatchCapacity::default()),
            _ => return Err("invalid E4 provider"),
        };
        let mut report = Report::default();
        match adapter {
            Err(error) => report.error = Some(report_error(error)),
            Ok(adapter) => match adapter.open(request).await {
                Err(error) => report.error = Some(report_error(error)),
                Ok(mut stream) => loop {
                    match stream.next().await {
                        Ok(Some(CompletionEvent::Delta(text))) => report.content.push_str(&text),
                        Ok(Some(CompletionEvent::Metadata { .. })) => {}
                        Ok(Some(CompletionEvent::Finished(finish))) => {
                            report.terminal = true;
                            report.finish_reason = Some(match finish.reason { FinishReason::Stop => "stop".into(), FinishReason::Length => "length".into(),
                                FinishReason::ContentFilter => "content_filter".into(), FinishReason::ToolCalls => "tool_calls".into(), FinishReason::Other(s) => s });
                        }
                        Ok(None) => break,
                        Err(error) => { report.error = Some(report_error(error)); break; }
                    }
                },
            },
        }
        let file = std::fs::File::create(output_path).map_err(|_| "cannot create E4 report")?;
        serde_json::to_writer(file, &report).map_err(|_| "cannot save E4 report")?;
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match native::run().await { Ok(()) => std::process::ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); std::process::ExitCode::FAILURE } }
}
#[cfg(target_arch = "wasm32")]
fn main() { compile_error!("e4_capture is a native loopback-only executable"); }
