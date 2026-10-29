//! A second tab on a job whose chat answer is still running: the same session
//! `GET` chat-history route reports the in-flight generation, and the real chat
//! leaf answers with exactly one watch, never a start (REPAIR-P5-5, P5-D6).
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::Path;
use axum::routing::get;
use axum::{Json, Router};
use llm_core::{ApplicationFrame, Persistence, StreamIdentity, StreamStatus, TextEvent};
use serde_json::{Value, json};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::ai::ports::ChatMessage;
use transgate_core::ai::{
    ChatHistory, ChatRole, Draft, GenerationId, GenerationKind, GenerationStatus, GenerationView, InFlightChat,
};
use transgate_core::presentation::ai::WatchKind;
use transgate_core::presentation::ai::chat::{ChatPanelState, Effect, Event};
use transgate_core::{AccountId, JobId, Micros};
use uuid::Uuid;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);
impl AuthStorage for Memory {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().unwrap().get(key).cloned() }
    fn set(&self, key: &str, value: &str) { self.0.lock().unwrap().insert(key.into(), value.into()); }
    fn remove(&self, key: &str) { self.0.lock().unwrap().remove(key); }
}
struct NoContention;
impl SessionLock for NoContention {
    fn acquire(&self) -> BoxFuture<'_, Result<LockGuard, String>> { Box::pin(async { Ok(LockGuard::new(|| {})) }) }
}
struct Fixed(AtomicI64);
impl Clock for Fixed {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}

const USER: Uuid = Uuid::from_u128(1);
const RUNNING: GenerationId = GenerationId(Uuid::from_u128(77));

fn view(status: GenerationStatus, text: &str) -> GenerationView {
    GenerationView {
        id: RUNNING, job: JobId(1), kind: GenerationKind::Chat, status, sequence: 3, source_revision: 1, consent_epoch: 1,
        draft: Draft::Text { text: text.into() }, final_content: None, durable: false, remote_outcome_unknown: false,
        execution: Some(Uuid::from_u128(5)), tokens_used: None, interruption: None, created_at: Micros(1), finished_at: None,
    }
}

fn saved() -> ChatMessage {
    ChatMessage {
        id: Uuid::from_u128(11), account: AccountId(Uuid::from_u128(2)), job: JobId(1), generation: GenerationId(Uuid::from_u128(60)),
        role: ChatRole::User, content: "Earlier question".into(), order: 1, consent_epoch: 1, created_at: Micros(1),
    }
}

async fn host() -> String {
    let chat = |Path(job): Path<i64>| async move {
        let body: Value = match job {
            // The answer is running: the saved messages, plus its question and held partial.
            1 => serde_json::to_value(ChatHistory {
                messages: vec![saved()],
                in_flight: Some(InFlightChat { question: "When is it due?".into(), generation: view(GenerationStatus::Running, "Hel") }),
            })
            .unwrap(),
            // A slow answer: the user can type before it lands.
            3 => {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                json!({"messages": [], "in_flight": null})
            }
            _ => json!({"messages": [], "in_flight": null}),
        };
        Json(body)
    };
    let routes = Router::new().route("/api/ai/jobs/{job}/chat", get(chat));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    base
}

fn api(base: &str) -> HttpApi {
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &json!({"access_token": "t", "refresh_token": "r", "expires_at": 4_000_000_000i64,
                                    "user_id": USER, "email": "ada@example.test"}).to_string());
    let ports = AuthPorts { storage, random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    HttpApi::new(base, Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports)))
}

fn loading(job: JobId) -> ChatPanelState {
    let mut chat = ChatPanelState::default();
    chat.update(Event::Selected { job });
    let effects = chat.update(Event::ConsentChanged { granted: true });
    assert!(effects.contains(&Effect::LoadHistory { job }));
    chat
}

#[tokio::test]
async fn an_in_flight_history_reattaches_with_one_watch_and_no_second_start() {
    let api = api(&host().await);
    let history = api.chat_history(USER, JobId(1)).await.unwrap();
    assert_eq!(history.in_flight.as_ref().map(|f| f.generation.id), Some(RUNNING));

    let mut chat = loading(JobId(1));
    let effects = chat.update(Event::HistoryLoaded { job: JobId(1), result: Ok(history) });
    assert_eq!(effects, vec![Effect::Watch { generation: RUNNING, kind: WatchKind::Text, after: None }]);
    assert!(!effects.iter().any(|e| matches!(e, Effect::StartChat { .. } | Effect::RetryChat { .. })));
}

#[tokio::test]
async fn a_history_without_an_in_flight_answer_opens_no_watch() {
    let api = api(&host().await);
    let history = api.chat_history(USER, JobId(2)).await.unwrap();
    assert!(history.in_flight.is_none());
    let mut chat = loading(JobId(2));
    assert_eq!(chat.update(Event::HistoryLoaded { job: JobId(2), result: Ok(history) }), vec![]);
}

fn frame(text: &str, status: StreamStatus, seq: u64) -> ApplicationFrame {
    let identity = StreamIdentity { generation: RUNNING.to_string(), content_revision: 1, consent_epoch: 1, execution: "e".into() };
    // The host marks the completed answer as saved; a provisional snapshot is never terminal.
    let persistence = if status == StreamStatus::Completed { Persistence::Saved } else { Persistence::Provisional };
    ApplicationFrame { identity, seq, event: TextEvent::Snapshot { text: text.into(), status, persistence } }
}

/// The own-tab race behind the candidate6 blank chat: a slow history response must
/// never wipe a question this tab sent, and a question typed before history landed
/// is refused rather than lost. After history, the send keeps its question and
/// streams its answer through to done.
#[tokio::test]
async fn a_slow_history_never_erases_a_question_sent_after_it() {
    let api = Arc::new(api(&host().await));
    let job = JobId(3);
    let mut chat = loading(job);
    let key = Uuid::from_u128(9);

    let history = tokio::spawn({
        let api = api.clone();
        async move { api.chat_history(USER, job).await }
    });
    // Typed while the history GET is still in flight.
    let early = chat.update(Event::SendRequested { text: "When is it due?".into(), key });
    assert_eq!(early, vec![], "no question starts before the history is known");
    assert!(chat.view(Micros(0), false).lines.is_empty());

    let loaded = chat.update(Event::HistoryLoaded { job, result: Ok(history.await.unwrap().unwrap()) });
    assert_eq!(loaded, vec![]);

    let sent = chat.update(Event::SendRequested { text: "When is it due?".into(), key });
    assert!(matches!(sent.as_slice(), [Effect::StartChat { job: j, .. }] if *j == job), "{sent:?}");
    assert_eq!(
        chat.update(Event::StartAccepted { key, generation: RUNNING }),
        vec![Effect::Watch { generation: RUNNING, kind: WatchKind::Text, after: None }]
    );
    chat.update(Event::WatchFrame { generation: RUNNING, frame: frame("Fri", StreamStatus::Streaming, 1) });
    let mid = chat.view(Micros(0), false);
    assert_eq!(
        mid.lines.iter().map(|l| (l.role, l.content.as_str(), l.streaming)).collect::<Vec<_>>(),
        vec![(ChatRole::User, "When is it due?", false), (ChatRole::Assistant, "Fri", true)]
    );
    let done = chat.update(Event::WatchFrame { generation: RUNNING, frame: frame("Friday.", StreamStatus::Completed, 2) });
    assert_eq!(done, vec![Effect::LoadLimits { job }]);
    let end = chat.view(Micros(0), false);
    assert_eq!(
        end.lines.iter().map(|l| (l.role, l.content.as_str())).collect::<Vec<_>>(),
        vec![(ChatRole::User, "When is it due?"), (ChatRole::Assistant, "Friday.")]
    );
    assert!(end.can_send);
}
