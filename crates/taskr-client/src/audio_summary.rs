//! Taskr's audio-work shell boundary. Credentials stay in the existing session
//! store; provider policy and summary persistence stay on the authenticated host.
use std::sync::{Arc, Mutex};

use reqwest::Method;
use serde::{Deserialize, Serialize};
use futures_util::future::{AbortHandle, Abortable};
use llm_core::{ApplicationFrame, Persistence, StreamIdentity, StreamState, StreamStatus, StreamUpdate, TextEvent};

use crate::{Failure, HttpApi, NO_BODY};

pub mod fixtures;
mod transport;

#[derive(Clone, Debug, Deserialize)]
pub struct AudioWork {
    pub id: String,
    pub name: String,
    pub status: String,
    pub transcript: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct SavedSummary {
    pub generation_id: String,
    pub work_id: String,
    pub source_revision: u64,
    pub text: String,
    pub saved_at_us: i64,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus { Accepted, Running, Completed, Interrupted, Cancelled }
impl GenerationStatus {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Interrupted | Self::Cancelled)
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Generation {
    pub generation_id: String,
    pub execution_id: String,
    pub work_id: String,
    pub source_revision: u64,
    pub status: GenerationStatus,
    pub sequence: u64,
    pub text: String,
    pub durable: bool,
    pub remote_outcome_unknown: bool,
    pub error: Option<String>,
}
impl Generation {
    fn identity(&self) -> StreamIdentity {
        StreamIdentity { generation: self.generation_id.clone(), content_revision: self.source_revision, consent_epoch: 0, execution: self.execution_id.clone() }
    }
    fn stream_status(&self) -> StreamStatus {
        match self.status {
            GenerationStatus::Accepted => StreamStatus::Starting,
            GenerationStatus::Running => StreamStatus::Streaming,
            GenerationStatus::Completed => StreamStatus::Completed,
            GenerationStatus::Interrupted => StreamStatus::Interrupted,
            GenerationStatus::Cancelled => StreamStatus::Cancelled,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct SummaryRead {
    pub summary: Option<SavedSummary>,
    #[serde(default)]
    pub generation: Option<Generation>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioWorkRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub selected: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    pub works: Vec<AudioWorkRow>,
    pub selected_id: Option<String>,
    pub generation_id: Option<String>,
    pub title: String,
    pub text: String,
    pub status_label: String,
    pub disclosure: String,
    pub is_loading: bool,
    pub can_start: bool,
    pub can_cancel: bool,
    pub can_suspend: bool,
    pub can_resume: bool,
    pub durable: bool,
    pub remote_outcome_unknown: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryError {
    pub code: String,
    pub message: String,
}
impl From<Failure> for SummaryError {
    fn from(value: Failure) -> Self {
        match value {
            Failure::NoSession => Self { code: "unauthenticated".into(), message: "Your session has ended. Sign in again.".into() },
            Failure::Server(_, wire) => Self { code: wire.code, message: wire.message },
            Failure::Network(message) => Self { code: "network".into(), message },
        }
    }
}

impl HttpApi {
    pub async fn audio_works(&self) -> Result<Vec<AudioWork>, SummaryError> {
        #[derive(Deserialize)] struct List { works: Vec<AudioWork> }
        Ok(self.call::<List>(Method::GET, "/audio-work", NO_BODY).await?.works)
    }
    pub async fn audio_summary(&self, work_id: &str) -> Result<SummaryRead, SummaryError> {
        Ok(self.call(Method::GET, &format!("/audio-work/{work_id}/summary"), NO_BODY).await?)
    }
    pub async fn start_audio_summary(&self, work_id: &str, request_id: &str, external_processing_accepted: bool) -> Result<Generation, SummaryError> {
        #[derive(Serialize)] struct Start<'a> { request_id: &'a str, external_processing_accepted: bool }
        Ok(self.call(Method::POST, &format!("/audio-work/{work_id}/summary/generations"), Some(&Start { request_id, external_processing_accepted })).await?)
    }
    pub async fn audio_generation(&self, work_id: &str, generation_id: &str) -> Result<Generation, SummaryError> {
        Ok(self.call(Method::GET, &format!("/audio-work/{work_id}/summary/generations/{generation_id}"), NO_BODY).await?)
    }
    pub async fn cancel_audio_summary(&self, work_id: &str, generation_id: &str) -> Result<Generation, SummaryError> {
        Ok(self.call(Method::POST, &format!("/audio-work/{work_id}/summary/generations/{generation_id}/cancel"), NO_BODY).await?)
    }
}

struct State {
    session: Option<String>,
    epoch: u64,
    works: Vec<AudioWork>,
    selected: Option<String>,
    generation: Option<Generation>,
    saved: Option<SavedSummary>,
    loading: bool,
    attached: bool,
    error: Option<String>,
    stream: StreamState,
    abort: Option<AbortHandle>,
    disposed: bool,
    pending_request: Option<(String, String)>,
}
impl State {
    fn new(session: Option<String>) -> Self {
        Self { session, epoch: 0, works: vec![], selected: None, generation: None, saved: None, loading: false, attached: false, error: None, stream: StreamState::default(), abort: None, disposed: false, pending_request: None }
    }
    fn can_start(&self) -> bool {
        !self.disposed && self.session.is_some() && !self.loading
            && self.selected_work().is_some_and(|work| work.status == "completed" && work.transcript.as_ref().is_some_and(|text| !text.is_empty()))
            && !self.generation.as_ref().is_some_and(|generation| !generation.status.terminal())
    }
    fn selected_work(&self) -> Option<&AudioWork> {
        self.works.iter().find(|work| Some(&work.id) == self.selected.as_ref())
    }
    fn view(&self) -> SummaryView {
        let work = self.selected_work();
        let active = self.generation.as_ref().is_some_and(|generation| !generation.status.terminal());
        let ready = work.is_some_and(|work| work.status == "completed" && work.transcript.as_ref().is_some_and(|text| !text.is_empty()));
        let generation = self.generation.as_ref();
        let status = match generation.map(|g| g.status) {
            Some(GenerationStatus::Accepted) => "Summary accepted.",
            Some(GenerationStatus::Running) if self.attached => "Receiving provisional summary…",
            Some(GenerationStatus::Running) => "Updates paused. Resume the same generation.",
            Some(GenerationStatus::Completed) => "Summary saved.",
            Some(GenerationStatus::Interrupted) => "Summary interrupted. Partial text is not a completed result.",
            Some(GenerationStatus::Cancelled) => "Stopped locally. Remote processing may still have occurred.",
            None if self.saved.is_some() => "Summary saved.",
            None if work.is_some() && !ready => "A completed transcript is required.",
            None if ready => "Ready to summarize.",
            None => "Select audio work to view or generate a summary.",
        };
        SummaryView {
            works: self.works.iter().map(|work| AudioWorkRow { id: work.id.clone(), name: work.name.clone(), status: work.status.clone(), selected: Some(&work.id) == self.selected.as_ref() }).collect(),
            selected_id: self.selected.clone(),
            generation_id: generation.map(|g| g.generation_id.clone()).or_else(|| self.saved.as_ref().map(|s| s.generation_id.clone())),
            title: work.map(|w| w.name.clone()).unwrap_or_else(|| "Audio summaries".into()),
            text: if self.stream.view().identity.is_some() { self.stream.view().text.to_string() } else { generation.map(|g| g.text.clone()).or_else(|| self.saved.as_ref().map(|s| s.text.clone())).unwrap_or_default() },
            status_label: status.into(),
            disclosure: "Generating a summary sends this audio work’s transcript to the configured external AI provider. Only start if you agree to this processing.".into(),
            is_loading: self.loading,
            can_start: self.can_start(),
            can_cancel: active && !self.loading,
            can_suspend: active && self.attached,
            can_resume: active && !self.attached && !self.loading,
            durable: generation.map(|g| g.durable && g.status == GenerationStatus::Completed).unwrap_or(self.saved.is_some()),
            remote_outcome_unknown: generation.is_some_and(|g| g.remote_outcome_unknown),
            error: self.error.clone().or_else(|| generation.and_then(|g| g.error.clone())),
        }
    }
    fn load_generation(&mut self, mut generation: Generation) {
        self.pending_request = None;
        let identity = generation.identity();
        self.stream.start(identity.clone());
        let status = generation.stream_status();
        let persistence = if generation.status == GenerationStatus::Completed && generation.durable { Persistence::Saved } else { Persistence::Provisional };
        let text = std::mem::take(&mut generation.text);
        if self.stream.update(ApplicationFrame { identity, seq: generation.sequence, event: TextEvent::Snapshot { text, status, persistence } }).is_err() {
            self.stream.clear();
            self.error = Some("The summary snapshot exceeded the supported output limit.".into());
        }
        self.generation = Some(generation);
    }
    fn stop_attachment(&mut self) {
        if let Some(abort) = self.abort.take() { abort.abort(); }
        self.stream.detach();
        self.attached = false;
    }
    fn reject(&mut self, error: SummaryError) {
        if matches!(error.code.as_str(), "unauthenticated" | "erased" | "not_found") {
            self.stop_attachment();
            self.works.clear(); self.selected = None; self.generation = None; self.saved = None; self.stream.clear(); self.pending_request = None;
        }
        self.error = Some(error.message);
    }
}

/// Shared shell controller, not taskr-core: a thin host client plus the reusable
/// text reducer. A session/selection epoch prevents old requests publishing views.
pub struct SummaryController {
    api: Arc<HttpApi>,
    state: Mutex<State>,
    #[cfg(not(target_arch = "wasm32"))]
    application: llm_http::ApplicationClient,
}
impl SummaryController {
    pub fn new(api: Arc<HttpApi>) -> Self {
        let session = api.sessions.load().map(|s| s.access_token);
        Self { api, state: Mutex::new(State::new(session)),
            #[cfg(not(target_arch = "wasm32"))]
            application: llm_http::ApplicationClient::new(llm_core::Limits::default(), std::time::Duration::from_secs(60)).expect("fixed application limits are valid"),
        }
    }
    fn synchronize_session(&self, state: &mut State) {
        if state.disposed { return; }
        let current = self.api.sessions.load().map(|s| s.access_token);
        if current != state.session {
            state.stop_attachment();
            let epoch = state.epoch.wrapping_add(1);
            *state = State::new(current);
            state.epoch = epoch;
        }
    }
    pub fn view(&self) -> SummaryView {
        let mut state = self.state.lock().unwrap();
        self.synchronize_session(&mut state);
        state.view()
    }
    fn begin(&self, update: impl FnOnce(&mut State)) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        self.synchronize_session(&mut state);
        if state.disposed || state.session.is_none() { return None; }
        state.stop_attachment();
        state.epoch = state.epoch.wrapping_add(1);
        state.loading = true;
        state.error = None;
        update(&mut state);
        Some(state.epoch)
    }
    fn finish(&self, epoch: u64, update: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap();
        self.synchronize_session(&mut state);
        if !state.disposed && state.epoch == epoch {
            state.loading = false;
            update(&mut state);
        }
    }
    pub async fn refresh(&self) {
        let Some(epoch) = self.begin(|_| {}) else { return; };
        let result = self.api.audio_works().await;
        self.finish(epoch, |state| match result {
            Ok(works) => { state.works = works; if state.selected_work().is_none() { state.selected = None; state.saved = None; state.generation = None; state.stream.clear(); state.pending_request = None; } },
            Err(error) => state.reject(error),
        });
    }
    pub async fn select(&self, work_id: String) {
        let Some(epoch) = self.begin(|state| {
            state.selected = Some(work_id.clone());
            state.stream.clear();
            state.generation = None;
            state.saved = None;
        }) else { return; };
        let result = self.api.audio_summary(&work_id).await;
        self.finish(epoch, |state| match result {
            Ok(read) => { state.saved = read.summary; if let Some(generation) = read.generation { state.load_generation(generation); } },
            Err(error) => state.reject(error),
        });
    }
    pub async fn start(&self, request_id: String, accepted: bool) {
        let (work_id, request_id, epoch) = {
            let mut state = self.state.lock().unwrap();
            self.synchronize_session(&mut state);
            if !accepted { state.error = Some("Agree to external processing before generating a summary.".into()); return; }
            if !state.can_start() { return; }
            let work = state.selected.clone().expect("a ready work is selected");
            let key = state.pending_request.as_ref().filter(|(id, _)| *id == work).map(|(_, key)| key.clone()).unwrap_or(request_id);
            state.pending_request = Some((work.clone(), key.clone()));
            state.stop_attachment();
            state.epoch = state.epoch.wrapping_add(1);
            state.loading = true;
            state.error = None;
            (work, key, state.epoch)
        };
        let result = self.api.start_audio_summary(&work_id, &request_id, accepted).await;
        self.finish(epoch, |state| match result {
            Ok(generation) => { state.pending_request = None; state.load_generation(generation); },
            Err(error) => { if error.code != "network" { state.pending_request = None; } state.reject(error); },
        });
    }
    pub fn suspend(&self) {
        let mut state = self.state.lock().unwrap();
        state.stop_attachment();
        state.epoch = state.epoch.wrapping_add(1);
        state.loading = false;
    }
    pub async fn cancel(&self) {
        let (work_id, generation_id, epoch) = {
            let mut state = self.state.lock().unwrap();
            self.synchronize_session(&mut state);
            if state.disposed { return; }
            let Some(generation) = state.generation.as_ref() else { return; };
            let target = (generation.work_id.clone(), generation.generation_id.clone());
            state.stop_attachment();
            state.epoch = state.epoch.wrapping_add(1);
            state.loading = true;
            state.error = None;
            (target.0, target.1, state.epoch)
        };
        let result = self.api.cancel_audio_summary(&work_id, &generation_id).await;
        self.finish(epoch, |state| match result {
            Ok(generation) => state.load_generation(generation),
            Err(error) => state.reject(error),
        });
    }
    /// Detach/resume only. Never calls the generation-start route. `on_view` is
    /// invoked after releasing all state locks and may synchronously re-enter.
    pub async fn watch(&self, on_view: impl Fn(SummaryView)) {
        let (epoch, work_id, generation_id, after, registration) = {
            let mut state = self.state.lock().unwrap();
            self.synchronize_session(&mut state);
            if state.disposed || state.session.is_none() || state.attached { return; }
            let Some(generation) = &state.generation else { return; };
            if generation.status.terminal() { return; }
            let target = (generation.work_id.clone(), generation.generation_id.clone(), state.stream.view().durable_seq.unwrap_or(generation.sequence));
            state.stop_attachment();
            state.epoch = state.epoch.wrapping_add(1);
            state.stream.resume();
            state.attached = true;
            state.error = None;
            let (abort, registration) = AbortHandle::new_pair();
            state.abort = Some(abort);
            (state.epoch, target.0, target.1, target.2, registration)
        };
        self.publish(epoch, &on_view);
        let path = format!("/audio-work/{work_id}/summary/generations/{generation_id}/watch?after={after}");
        let delivery = self.receive(epoch, &path, &on_view);
        let result = Abortable::new(delivery, registration).await;
        // Frames carry presentation data, not provider outcome/error metadata.
        // Refresh only unsuccessful terminals from the owner-scoped read API.
        let read_terminal = {
            let mut state = self.state.lock().unwrap();
            self.synchronize_session(&mut state);
            matches!(&result, Ok(Ok(()))) && state.epoch == epoch && !state.disposed
                && state.generation.as_ref().is_some_and(|g| matches!(g.status, GenerationStatus::Interrupted | GenerationStatus::Cancelled))
        };
        if read_terminal {
            let latest = self.api.audio_generation(&work_id, &generation_id).await;
            self.finish(epoch, |state| match latest {
                Ok(generation) => state.load_generation(generation),
                Err(error) => state.reject(error),
            });
        }
        self.finish(epoch, |state| {
            state.stop_attachment();
            if let Ok(Err(error)) = result { state.reject(error); }
        });
        self.publish(epoch, &on_view);
    }
    fn publish(&self, epoch: u64, listener: &impl Fn(SummaryView)) {
        let view = {
            let mut state = self.state.lock().unwrap();
            self.synchronize_session(&mut state);
            if state.epoch != epoch || state.disposed { return; }
            state.view()
        };
        listener(view);
    }
    fn apply_frame(&self, epoch: u64, frame: ApplicationFrame) -> Result<bool, SummaryError> {
        let mut state = self.state.lock().unwrap();
        self.synchronize_session(&mut state);
        if state.epoch != epoch || state.disposed { return Ok(true); }
        let Some(generation) = state.generation.as_ref() else { return Ok(true); };
        if frame.identity.generation != generation.generation_id
            || frame.identity.content_revision != generation.source_revision
            || frame.identity.consent_epoch != 0
            || frame.identity.execution != generation.execution_id
        {
            return Err(SummaryError { code: "identity".into(), message: "The stream does not belong to the selected generation.".into() });
        }
        if matches!(&frame.event, TextEvent::Revoked) {
            state.reject(SummaryError { code: "erased".into(), message: "This session can no longer access the summary.".into() });
            return Ok(true);
        }
        let seq = frame.seq;
        let transient = match &frame.event {
            TextEvent::Interrupted(error) if matches!(error.code.as_str(), "identity_unavailable" | "unavailable") => Some(SummaryError { code: error.code.clone(), message: error.message.clone() }),
            _ => None,
        };
        match state.stream.update(frame).map_err(|e| SummaryError { code: "protocol".into(), message: e.to_string() })? {
            StreamUpdate::ResnapshotRequired => return Err(SummaryError { code: "resnapshot".into(), message: "A fresh snapshot is required. Resume updates.".into() }),
            StreamUpdate::Ignored => return Ok(false),
            StreamUpdate::Applied { .. } => {}
        }
        if let Some(error) = transient { return Err(error); }
        let view = state.stream.view();
        let status = match view.status {
            StreamStatus::Starting => GenerationStatus::Accepted,
            StreamStatus::Streaming => GenerationStatus::Running,
            StreamStatus::Completed => GenerationStatus::Completed,
            StreamStatus::Interrupted | StreamStatus::Revoked => GenerationStatus::Interrupted,
            StreamStatus::Cancelled => GenerationStatus::Cancelled,
        };
        let durable = view.persistence == Persistence::Saved;
        let error = view.error.map(|error| error.message.clone());
        let generation = state.generation.as_mut().expect("generation checked");
        generation.sequence = seq;
        generation.status = status;
        generation.durable = durable;
        generation.error = error;
        if status == GenerationStatus::Completed { generation.remote_outcome_unknown = false; }
        if status == GenerationStatus::Running { generation.remote_outcome_unknown = true; }
        Ok(status.terminal())
    }
    pub fn dispose(&self) {
        let mut state = self.state.lock().unwrap();
        state.stop_attachment();
        let epoch = state.epoch.wrapping_add(1);
        *state = State::new(None);
        state.epoch = epoch;
        state.disposed = true;
    }
}
