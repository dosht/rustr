//! The HTTP contract of the source routes. Each parser takes the raw body and
//! either returns a typed request for a core use case or the source's own 400
//! message; each response type has the source's field names. Routing,
//! authentication and I/O live in `transgate-http`.
//!
//! Bodies are parsed from `serde_json::Value` rather than derived, because the
//! source validates by JavaScript `typeof` and its messages quote the offending
//! values (`Invalid parameters jobId: undefined, fileName: x`).

pub mod account;
pub mod ai;
pub mod openapi;
pub mod public;

use serde_json::{Value, json};
use transgate_core::jobs::app::{DraftOptions, StartAnswer, StartRequest, UrlItem, UrlRequest};
use transgate_core::jobs::domain::Provider;
use transgate_core::jobs::ports::LanguageUsage;
use transgate_core::jobs::routing::Preferences;
use transgate_core::{JobId, Language, Micros};

/// A route's 400 message, in the source's words.
pub type Invalid = String;

fn object(body: &[u8]) -> Result<serde_json::Map<String, Value>, Invalid> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err("Request body is required".into()),
    }
}

/// JavaScript's template-literal rendering of a body value, for the source's messages.
fn js_display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) => "null".into(),
        Some(other) => other.to_string(),
    }
}

fn whole_number(value: Option<&Value>) -> Option<i64> {
    value.and_then(|v| v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15).map(|f| f as i64)))
}

/// `/api/init-upload` and `/api/streaming-url`: `{ jobId: number, fileName: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobFile {
    pub job: JobId,
    pub file_name: String,
}

pub fn parse_job_file(body: &[u8]) -> Result<JobFile, Invalid> {
    let map = object(body).unwrap_or_default();
    let (job, name) = (map.get("jobId"), map.get("fileName"));
    match (whole_number(job), name) {
        (Some(id), Some(Value::String(file_name))) if job.is_some_and(Value::is_number) => Ok(JobFile { job: JobId(id), file_name: file_name.clone() }),
        _ => Err(format!("Invalid parameters jobId: {}, fileName: {}", js_display(job), js_display(name))),
    }
}

/// `/api/youtube/add`: `{ youtubeUrl: string, language?: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YoutubeAdd {
    pub url: String,
    pub language: Option<String>,
}

pub fn parse_youtube_add(body: &[u8]) -> Result<YoutubeAdd, Invalid> {
    let map = object(body).unwrap_or_default();
    match map.get("youtubeUrl") {
        Some(Value::String(url)) if !url.is_empty() => Ok(YoutubeAdd {
            url: url.clone(),
            language: map.get("language").and_then(Value::as_str).filter(|l| !l.is_empty()).map(str::to_string),
        }),
        _ => Err("youtubeUrl is required".into()),
    }
}

/// `/api/youtube/metadata`: `{ videoId: string }`, a bare id or any YouTube URL.
pub fn parse_youtube_metadata(body: &[u8]) -> Result<String, Invalid> {
    match object(body).unwrap_or_default().get("videoId") {
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        _ => Err("videoId is required".into()),
    }
}

/// `/api/youtube/resolve`: `{ jobId: number }`.
pub fn parse_youtube_resolve(body: &[u8]) -> Result<JobId, Invalid> {
    let map = object(body).unwrap_or_default();
    let id = map.get("jobId");
    match whole_number(id) {
        Some(n) if n != 0 && id.is_some_and(Value::is_number) => Ok(JobId(n)),
        _ => Err("jobId (number) is required".into()),
    }
}

/// The `{ status, jobId, error? }` answer of `/api/youtube/resolve`.
pub fn resolve_response(answer: &transgate_core::jobs::app::ResolveAnswer) -> Value {
    use transgate_core::jobs::app::ResolveAnswer;
    match answer {
        ResolveAnswer::Completed { job_id } => json!({"status": "completed", "jobId": job_id.0}),
        ResolveAnswer::Failed { job_id, code, message } => {
            json!({"status": "failed", "jobId": job_id.0, "error": {"code": code, "message": message}})
        }
    }
}

/// The two request modes of `/api/v1/transcriptions`.
#[derive(Clone, Debug, PartialEq)]
pub enum Transcriptions {
    JobIds(StartRequest),
    AudioUrls(UrlRequest),
}

fn is_url(text: &str) -> bool {
    // `new URL(...)`: a scheme, then something. Destination safety is checked at fetch time, not here.
    text.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && !rest.is_empty()
    })
}

fn preferences(options: Option<&Value>) -> Result<Preferences, Invalid> {
    let Some(options) = options.filter(|o| !o.is_null() && o.as_bool() != Some(false)) else { return Ok(Preferences::default()) };
    let Some(options) = options.as_object() else { return Err("options must be an object".into()) };
    if let Some(hook) = options.get("webhookUrl").filter(|v| !v.is_null() && v.as_str() != Some("")) {
        match hook.as_str() {
            None => return Err("options.webhookUrl must be a string".into()),
            Some(text) if !is_url(text) => return Err("options.webhookUrl must be a valid URL".into()),
            Some(_) => {}
        }
    }
    let preferred = match options.get("preferredProvider").and_then(Value::as_str) {
        None | Some("") => None,
        Some(text) => Some(Provider::parse(text).ok_or_else(|| format!("Unknown preferredProvider: {text}"))?),
    };
    Ok(Preferences { preferred, fallback_enabled: options.get("fallbackEnabled") != Some(&Value::Bool(false)) })
}

/// `validateTranscriptionRequest`. `audioUrls` is accepted here as a request shape; whether the
/// caller may use it is decided by the route (it needs API authentication).
pub fn parse_transcriptions(body: &[u8]) -> Result<Transcriptions, Invalid> {
    let map = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => return Err("Request body is required".into()),
    };
    let nonempty = |key: &str| map.get(key).and_then(Value::as_array).filter(|a| !a.is_empty());
    let (ids, urls) = (nonempty("jobIds"), nonempty("audioUrls"));
    if ids.is_none() && urls.is_none() {
        return Err("Either jobIds or audioUrls must be provided".into());
    }
    if ids.is_some() && urls.is_some() {
        return Err("Cannot provide both jobIds and audioUrls in the same request".into());
    }
    let locale = match map.get("locale") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => return Err("locale must be a string".into()),
    };
    let preferences = preferences(map.get("options"))?;
    if let Some(ids) = ids {
        let parsed: Option<Vec<JobId>> = ids.iter().map(|v| whole_number(Some(v)).filter(|n| *n > 0 && v.is_number()).map(JobId)).collect();
        let job_ids = parsed.ok_or("All jobIds must be positive integers")?;
        return Ok(Transcriptions::JobIds(StartRequest { job_ids, locale, preferences }));
    }
    let mut items = Vec::new();
    for (i, entry) in urls.into_iter().flatten().enumerate() {
        let Some(entry) = entry.as_object() else { return Err(format!("audioUrls[{i}] must be an object")) };
        let audio_url = match entry.get("audioUrl") {
            Some(Value::String(text)) if !text.is_empty() => text.clone(),
            _ => return Err(format!("audioUrls[{i}].audioUrl is required and must be a string")),
        };
        if !is_url(&audio_url) {
            return Err(format!("audioUrls[{i}].audioUrl must be a valid URL"));
        }
        let callback_url = match entry.get("callbackUrl") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if text.is_empty() => None,
            Some(Value::String(text)) if is_url(text) => Some(text.clone()),
            Some(Value::String(_)) => return Err(format!("audioUrls[{i}].callbackUrl must be a valid URL")),
            Some(_) => return Err(format!("audioUrls[{i}].callbackUrl must be a string")),
        };
        let language = match entry.get("language") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) if text.is_empty() => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => return Err(format!("audioUrls[{i}].language must be a string")),
        };
        items.push(UrlItem { audio_url, callback_url, language });
    }
    Ok(Transcriptions::AudioUrls(UrlRequest { items, locale, preferences, whisper_params: None }))
}

/// `PATCH /api/jobs/options`: `{ jobIds: [..], language?: name|null, translateTo?: [name], speakers?: bool }`.
/// An absent `language` leaves it alone; `null` is auto-detect (a double option core cannot read
/// from plain serde, where both collapse to nothing).
pub fn parse_draft_options(body: &[u8]) -> Result<(Vec<JobId>, DraftOptions), Invalid> {
    let map = object(body).unwrap_or_default();
    let ids = map.get("jobIds").and_then(Value::as_array).filter(|a| !a.is_empty()).ok_or("jobIds must be a nonempty array")?;
    let job_ids = ids
        .iter()
        .map(|v| whole_number(Some(v)).filter(|n| *n > 0 && v.is_number()).map(JobId))
        .collect::<Option<Vec<_>>>()
        .ok_or("All jobIds must be positive integers")?;
    let language = match map.get("language") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(name)) => Some(Some(Language::from_name(name).ok_or_else(|| format!("Unsupported language: {name}"))?)),
        Some(_) => return Err("language must be a string or null".into()),
    };
    let translate_to = match map.get("translateTo") {
        None => None,
        Some(Value::Array(names)) => Some(
            names
                .iter()
                .map(|v| v.as_str().and_then(Language::from_name).ok_or_else(|| format!("Unsupported language: {}", js_display(Some(v)))))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err("translateTo must be an array".into()),
    };
    let speakers = match map.get("speakers") {
        None => None,
        Some(Value::Bool(flag)) => Some(*flag),
        Some(_) => return Err("speakers must be a boolean".into()),
    };
    Ok((job_ids, DraftOptions { language, translate_to, speakers }))
}

/// `GET /api/languages/usage`: the most used languages and translation targets, most used first.
pub fn language_usage_response(usage: &LanguageUsage) -> Value {
    let rows = |list: &[(Language, u64)]| list.iter().map(|(language, count)| json!({"language": language.name(), "count": count})).collect::<Vec<_>>();
    json!({"languages": rows(&usage.languages), "targets": rows(&usage.targets)})
}


/// RFC 3339 UTC with milliseconds, the JavaScript `toISOString()` shape.
pub fn iso8601(at: Micros) -> String {
    let millis_total = at.0.div_euclid(1000);
    let (days, ms_of_day) = (millis_total.div_euclid(86_400_000), millis_total.rem_euclid(86_400_000));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s, ms) = (ms_of_day / 3_600_000, ms_of_day / 60_000 % 60, ms_of_day / 1000 % 60, ms_of_day % 1000);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

/// The 201 session answer of `/api/jobs/start`: `{ success, data: { success, results, metadata }, metadata }`.
/// Built from the same DTOs as the public route, so the two bodies cannot drift.
pub fn start_response(answer: &StartAnswer, request_id: &str, at: Micros, processing_ms: u64, client_type: &str) -> Value {
    let response = public::StartResponse {
        success: true,
        data: public::StartData {
            success: true,
            results: answer.results.iter().map(public::result_view).collect(),
            metadata: public::meta_view(&answer.meta),
        },
        metadata: public::metadata(request_id, at, processing_ms, client_type),
    };
    serde_json::to_value(response).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests;
