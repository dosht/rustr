use std::collections::HashSet;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_ports::*;
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use crate::http::{self, HttpConfig};

#[derive(Clone, Debug)]
pub struct SonioxConfig {
    /// API origin, normally https://api.soniox.com (no /v1 suffix).
    pub base_url: String,
    pub api_key: SecretString,
    pub project: String,
    pub model: String,
    pub http: HttpConfig,
}
pub struct Soniox { client: Client, base: Url, config: SonioxConfig }
#[derive(Clone, Debug)]
pub struct DeepgramConfig {
    /// Global API origin https://api.deepgram.com; D4 does not force EU residency.
    pub base_url: String,
    pub api_key: SecretString,
    pub project: String,
    pub http: HttpConfig,
}
pub struct Deepgram { client: Client, base: Url, config: DeepgramConfig }

fn remote_matches(provider: Provider, project: &str, remote: &RemoteIdentity) -> Result<(), TransportError> {
    if remote.provider != provider || remote.project != project || remote.id.is_empty() {
        Err(TransportError::Rejected { status: None, code: "remote_identity".into() })
    } else { Ok(()) }
}
fn callback_url(request: &TranscriptionRequest) -> Result<Url, TransportError> {
    let url = Url::parse(request.callback.url.expose()).map_err(|_| TransportError::Configuration("callback URL".into()))?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some() || request.callback.secret.expose().is_empty() {
        return Err(TransportError::Configuration("authenticated HTTPS callback required".into()));
    }
    let attempts: Vec<_> = url.query_pairs().filter(|(name, _)| name == "attempt").collect();
    if attempts.len() != 1 || attempts[0].1 != request.attempt.to_string() {
        return Err(TransportError::Configuration("callback attempt binding".into()));
    }
    Ok(url)
}
fn request_matches(request: &TranscriptionRequest, provider: Provider, project: &str) -> Result<(), TransportError> {
    if request.provider != provider || request.project != project || request.client_reference.is_empty()
        || request.audio.method != AccessMethod::Get {
        return Err(TransportError::Rejected { status: None, code: "submission_identity".into() });
    }
    let audio = Url::parse(request.audio.url.expose()).map_err(|_| TransportError::Configuration("audio access URL".into()))?;
    if !matches!(audio.scheme(), "http" | "https") || audio.host_str().is_none()
        || !audio.username().is_empty() || audio.password().is_some() || audio.fragment().is_some() {
        return Err(TransportError::Configuration("signed HTTP(S) audio access required".into()));
    }
    Ok(())
}
async fn submitted(response: Result<Response, reqwest::Error>, config: &HttpConfig, provider: Provider, project: &str, field: &str) -> Result<Submission, TransportError> {
    let response = match response {
        Ok(response) => response,
        Err(_) => return Ok(Submission::Uncertain { status: None, operation: "submit".into() }),
    };
    let status = response.status();
    if status.is_client_error() && status.as_u16() != 408 {
        return Ok(Submission::Rejected { status: Some(status.as_u16()), code: "provider_rejected".into() });
    }
    if !status.is_success() {
        return Ok(Submission::Uncertain { status: Some(status.as_u16()), operation: "submit".into() });
    }
    let value: Value = match http::json(response, config, "submit answer").await {
        Ok(value) => value,
        Err(_) => return Ok(Submission::Uncertain { status: Some(status.as_u16()), operation: "submit answer".into() }),
    };
    match value.get(field).and_then(Value::as_str).filter(|s| !s.is_empty()) {
        Some(id) => Ok(Submission::Accepted { remote: RemoteIdentity { provider, project: project.into(), id: id.into() } }),
        None => Ok(Submission::Uncertain { status: Some(status.as_u16()), operation: "submit identity".into() }),
    }
}
impl Soniox {
    pub fn new(config: SonioxConfig) -> Result<Self, TransportError> {
        if config.api_key.expose().is_empty() || config.project.is_empty() || config.model != "stt-async-v5" {
            return Err(TransportError::Configuration("Soniox key, project and explicit stt-async-v5 required".into()));
        }
        Ok(Self { client: http::client(&config.http)?, base: http::configured_url(&config.base_url)?, config })
    }
    async fn reported(&self, remote: &RemoteIdentity) -> Result<SonioxReported, TransportError> {
        remote_matches(Provider::Soniox, &self.config.project, remote)?;
        let url = http::endpoint(&self.base, &["v1", "transcriptions", &remote.id])?;
        let response = self.client.get(url).bearer_auth(self.config.api_key.expose()).send().await
            .map_err(|_| http::unavailable("Soniox status", None))?;
        let reported: SonioxReported = http::json(response, &self.config.http, "Soniox status").await?;
        if reported.id != remote.id { return Err(TransportError::Malformed("Soniox status identity".into())); }
        Ok(reported)
    }
    async fn transcript(&self, remote: &RemoteIdentity) -> Result<SonioxTranscript, TransportError> {
        remote_matches(Provider::Soniox, &self.config.project, remote)?;
        let url = http::endpoint(&self.base, &["v1", "transcriptions", &remote.id, "transcript"])?;
        let response = self.client.get(url).bearer_auth(self.config.api_key.expose()).send().await
            .map_err(|_| http::unavailable("Soniox transcript", None))?;
        let transcript: SonioxTranscript = http::json(response, &self.config.http, "Soniox transcript").await?;
        if transcript.id != remote.id { return Err(TransportError::Malformed("Soniox transcript identity".into())); }
        Ok(transcript)
    }
}
impl Transcriber for Soniox {
    fn provider(&self) -> Provider { Provider::Soniox }
    fn project(&self) -> &str { &self.config.project }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        Box::pin(async move {
            request_matches(request, self.provider(), self.project())?;
            if request.model != self.config.model {
                return Err(TransportError::Rejected { status: None, code: "configured_model_mismatch".into() });
            }
            let callback = callback_url(request)?;
            let mut body = json!({
                "model": request.model, "audio_url": request.audio.url.expose(),
                "enable_language_identification": true, "enable_speaker_diarization": request.diarize,
                "client_reference_id": request.client_reference, "webhook_url": callback.as_str(),
                "webhook_auth_header_name": "X-Webhook-Secret", "webhook_auth_header_value": request.callback.secret.expose()
            });
            if let Some(language) = &request.language { body["language_hints"] = json!([language.to_lowercase()]); }
            if let Some(target) = &request.translation_target {
                if target.is_empty() { return Err(TransportError::Rejected { status: None, code: "translation_language".into() }); }
                body["translation"] = json!({"type":"one_way", "target_language":target.to_lowercase()});
            }
            let url = http::endpoint(&self.base, &["v1", "transcriptions"])?;
            let response = self.client.post(url).bearer_auth(self.config.api_key.expose()).json(&body).send().await;
            submitted(response, &self.config.http, self.provider(), self.project(), "id").await
        })
    }
    fn status<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async move { Ok(ProviderObservation { remote: remote.clone(), origin: ObservationOrigin::RecoveryPoll,
            payload: ProviderPayload::Soniox { reported: self.reported(remote).await?, transcript: None } }) })
    }
    fn result<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            let reported = self.reported(remote).await?;
            let transcript = if reported.status == "completed" { Some(self.transcript(remote).await?) } else { None };
            Ok(ProviderObservation { remote: remote.clone(), origin: ObservationOrigin::RecoveryPoll,
                payload: ProviderPayload::Soniox { reported, transcript } })
        })
    }
    fn delete_remote<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            remote_matches(self.provider(), self.project(), remote)?;
            let url = http::endpoint(&self.base, &["v1", "transcriptions", &remote.id])?;
            let response = self.client.delete(url).bearer_auth(self.config.api_key.expose()).send().await
                .map_err(|_| http::unavailable("Soniox delete", None))?;
            match response.status().as_u16() {
                200 | 204 => Ok(DeleteOutcome::Deleted),
                404 => Ok(DeleteOutcome::Absent),
                409 => Ok(DeleteOutcome::Retained { until: None, reason: "provider processing or state conflict".into() }),
                status => Err(http::unavailable("Soniox delete", Some(status))),
            }
        })
    }
    fn find_by_reference<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async move {
            #[derive(Deserialize)]
            struct Row {
                id: String, status: String, model: String, created_at: String,
                client_reference_id: Option<String>, audio_url: Option<SensitiveUrl>, webhook_url: Option<SensitiveUrl>,
            }
            #[derive(Deserialize)]
            struct Page { transcriptions: Vec<Row>, next_page_cursor: Option<String> }
            let mut cursor: Option<String> = None;
            let mut seen = HashSet::new();
            let mut matches = Vec::new();
            loop {
                let mut url = http::endpoint(&self.base, &["v1", "transcriptions"])?;
                url.query_pairs_mut().append_pair("limit", "100");
                if let Some(value) = &cursor { url.query_pairs_mut().append_pair("cursor", value); }
                let response = self.client.get(url).bearer_auth(self.config.api_key.expose()).send().await
                    .map_err(|_| http::unavailable("Soniox list", None))?;
                let page: Page = http::json(response, &self.config.http, "Soniox list").await?;
                for row in page.transcriptions {
                    if row.client_reference_id.as_deref() == Some(reference) {
                        matches.push(RemoteRecord { remote: RemoteIdentity { provider:self.provider(), project:self.project().into(), id:row.id },
                            status:row.status, model:row.model, created_at:row.created_at, client_reference:row.client_reference_id,
                            audio_url:row.audio_url, callback_url:row.webhook_url });
                    }
                }
                match page.next_page_cursor {
                    None => return Ok(matches),
                    Some(next) if !next.is_empty() && seen.insert(next.clone()) => cursor = Some(next),
                    Some(_) => return Err(TransportError::Malformed("Soniox pagination cycle".into())),
                }
            }
        })
    }
    fn verify_callback(&self, binding: &CallbackBinding, headers: &Headers, body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        if !http::authentic(http::one_header(headers, "X-Webhook-Secret"), binding.secret.expose()) {
            return Err(CallbackError::Unauthorized);
        }
        if body.len() > self.config.http.max_response_bytes { return Err(CallbackError::Malformed); }
        let reported: SonioxReported = serde_json::from_slice(body).map_err(|_| CallbackError::Malformed)?;
        if binding.provider != self.provider() || binding.project != self.project() || reported.id.is_empty()
            || binding.expected_remote_id.as_ref().is_some_and(|expected| *expected != reported.id) {
            return Err(CallbackError::IdentityMismatch);
        }
        let remote = RemoteIdentity { provider:self.provider(), project:self.project().into(), id:reported.id.clone() };
        Ok(ProviderObservation { remote, origin:ObservationOrigin::Webhook,
            payload:ProviderPayload::Soniox { reported, transcript:None } })
    }
    fn complete_callback(&self, mut observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            remote_matches(self.provider(), self.project(), &observation.remote)?;
            match &mut observation.payload {
                ProviderPayload::Soniox { reported, transcript } if reported.id == observation.remote.id => {
                    if reported.status == "completed" && transcript.is_none() {
                        // D6: no GET status here, even when the webhook omits duration.
                        *transcript = Some(self.transcript(&observation.remote).await?);
                    }
                }
                _ => return Err(TransportError::Malformed("Soniox callback identity".into())),
            }
            Ok(observation)
        })
    }
}
impl Deepgram {
    pub fn new(config: DeepgramConfig) -> Result<Self, TransportError> {
        if config.api_key.expose().is_empty() || config.project.is_empty() {
            return Err(TransportError::Configuration("Deepgram key and project required".into()));
        }
        Ok(Self { client:http::client(&config.http)?, base:http::configured_url(&config.base_url)?, config })
    }
}
impl Transcriber for Deepgram {
    fn provider(&self) -> Provider { Provider::Deepgram }
    fn project(&self) -> &str { &self.config.project }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        Box::pin(async move {
            request_matches(request, self.provider(), self.project())?;
            if request.translation_target.is_some() || !matches!(request.model.as_str(), "whisper-large" | "nova-2")
                || request.language.as_deref().is_none_or(str::is_empty) {
                return Err(TransportError::Rejected { status:None, code:"unsupported_deepgram_options".into() });
            }
            let mut callback = callback_url(request)?;
            if !matches!(callback.port_or_known_default(), Some(80 | 443 | 8080 | 8443)) {
                return Err(TransportError::Configuration("Deepgram callback port".into()));
            }
            callback.set_username(&request.attempt.to_string()).map_err(|_| TransportError::Configuration("callback username".into()))?;
            callback.set_password(Some(request.callback.secret.expose())).map_err(|_| TransportError::Configuration("callback password".into()))?;
            let mut url = http::endpoint(&self.base, &["v1", "listen"])?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("model", &request.model).append_pair("smart_format", "true")
                    .append_pair("punctuate", "true").append_pair("paragraphs", "true")
                    .append_pair("diarize", if request.diarize { "true" } else { "false" })
                    .append_pair("mip_opt_out", "true").append_pair("callback", callback.as_str());
                if let Some(language) = &request.language { query.append_pair("language", &language.to_lowercase()); }
            }
            let response = self.client.post(url).header("Authorization", format!("Token {}", self.config.api_key.expose()))
                .json(&json!({"url":request.audio.url.expose()})).send().await;
            submitted(response, &self.config.http, self.provider(), self.project(), "request_id").await
        })
    }
    fn status<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            remote_matches(self.provider(), self.project(), remote)?;
            Err(TransportError::Unsupported("Deepgram does not provide asynchronous transcript recovery".into()))
        })
    }
    fn result<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> { self.status(remote) }
    fn delete_remote<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            remote_matches(self.provider(), self.project(), remote)?;
            Ok(DeleteOutcome::Unsupported { reason:"Deepgram has no per-transcript deletion API; MIP opt-out is not log/metadata erasure".into() })
        })
    }
    fn find_by_reference<'a>(&'a self, _reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("Deepgram has no transcription listing by client reference".into())) })
    }
    fn verify_callback(&self, binding: &CallbackBinding, headers: &Headers, body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        if binding.secret.expose().is_empty() { return Err(CallbackError::Unauthorized); }
        let expected = format!("Basic {}", STANDARD.encode(format!("{}:{}", binding.attempt, binding.secret.expose())));
        if !http::authentic(http::one_header(headers, "Authorization"), &expected) { return Err(CallbackError::Unauthorized); }
        if body.len() > self.config.http.max_response_bytes { return Err(CallbackError::Malformed); }
        let payload: DeepgramTranscript = serde_json::from_slice(body).map_err(|_| CallbackError::Malformed)?;
        if binding.provider != self.provider() || binding.project != self.project() || payload.metadata.request_id.is_empty()
            || binding.expected_remote_id.as_ref().is_some_and(|expected| *expected != payload.metadata.request_id) {
            return Err(CallbackError::IdentityMismatch);
        }
        let remote = RemoteIdentity { provider:self.provider(), project:self.project().into(), id:payload.metadata.request_id.clone() };
        Ok(ProviderObservation { remote, origin:ObservationOrigin::Webhook, payload:ProviderPayload::Deepgram(payload) })
    }
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            remote_matches(self.provider(), self.project(), &observation.remote)?;
            match &observation.payload {
                ProviderPayload::Deepgram(result) if result.metadata.request_id == observation.remote.id => Ok(observation),
                _ => Err(TransportError::Malformed("Deepgram callback identity".into())),
            }
        })
    }
}
