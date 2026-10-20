use js_sys::{Date, Reflect};
use serde::{Deserialize, Serialize};
use taskr_core::session::app::{Session, SessionStore};
use taskr_core::shared::access::WorkspaceId;
use taskr_core::shared::clock::{Clock, Millis};
use taskr_core::shared::events::DomainEvent;
use taskr_core::shared::future::BoxFuture;
use taskr_core::shared::storage::{Revision, SaveError};
use taskr_core::shared::telemetry::{Level, LogEvent, Telemetry};
use taskr_core::tasks::app::{StorageError, TaskListRepository};
use taskr_core::tasks::domain::TaskList;
use taskr_infra::snapshot_json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Storage, console};

const STORAGE_KEY: &str = "taskr.tasks";
const SESSION_KEY: &str = "taskr.session";

pub struct BrowserClock;

impl Clock for BrowserClock {
    fn now(&self) -> Millis {
        Date::now() as Millis
    }
}

/// Looks `localStorage` up on the global object instead of `window`, so the
/// same build also runs under Node (which has a global `localStorage`).
pub struct LocalStorageTaskListRepository;

impl LocalStorageTaskListRepository {
    fn storage() -> Result<Storage, StorageError> {
        let value = Reflect::get(&js_sys::global(), &JsValue::from_str("localStorage"))
            .map_err(|_| StorageError("localStorage is not reachable".into()))?;
        if value.is_undefined() || value.is_null() {
            return Err(StorageError("localStorage is not available".into()));
        }
        Ok(value.unchecked_into())
    }
}

/// The local workspace keeps the original key, so existing data still loads.
fn key_for(workspace: &WorkspaceId) -> String {
    if *workspace == WorkspaceId::local() {
        STORAGE_KEY.to_string()
    } else {
        format!("{STORAGE_KEY}.{workspace}")
    }
}

impl LocalStorageTaskListRepository {
    fn read(key: &str) -> Result<(TaskList, Revision), StorageError> {
        match Self::storage()?.get_item(key) {
            Ok(Some(text)) => snapshot_json::decode(&text),
            Ok(None) => Ok((TaskList::new(), Revision::NEW)),
            Err(_) => Err(StorageError("could not read localStorage".into())),
        }
    }
}

/// No outbox: the browser's local mode is single device, like the CLI's files.
impl TaskListRepository for LocalStorageTaskListRepository {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
        Box::pin(async move { Self::read(&key_for(workspace)) })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        list: &'a TaskList,
        expected: Revision,
        _events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            let key = key_for(workspace);
            if Self::read(&key)?.1 != expected {
                return Err(SaveError::Conflict);
            }
            Self::storage()?
                .set_item(&key, &snapshot_json::encode(list, expected.next()))
                .map_err(|_| StorageError("could not write localStorage (quota?)".into()))?;
            Ok(())
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredSession {
    access_token: String,
    refresh_token: Option<String>,
    email: String,
    expires_at: Option<i64>,
}

/// Keeps the session in localStorage, so a reload stays signed in.
pub struct LocalStorageSessionStore;

impl SessionStore for LocalStorageSessionStore {
    fn load(&self) -> Option<Session> {
        let text = LocalStorageTaskListRepository::storage().ok()?.get_item(SESSION_KEY).ok()??;
        let s: StoredSession = serde_json::from_str(&text).ok()?;
        Some(Session {
            access_token: s.access_token,
            refresh_token: s.refresh_token,
            email: s.email,
            expires_at: s.expires_at,
        })
    }

    fn save(&self, s: &Session) {
        let stored = StoredSession {
            access_token: s.access_token.clone(),
            refresh_token: s.refresh_token.clone(),
            email: s.email.clone(),
            expires_at: s.expires_at,
        };
        if let Ok(storage) = LocalStorageTaskListRepository::storage() {
            let _ = storage
                .set_item(SESSION_KEY, &serde_json::to_string(&stored).expect("plain struct"));
        }
    }

    fn clear(&self) {
        if let Ok(storage) = LocalStorageTaskListRepository::storage() {
            let _ = storage.remove_item(SESSION_KEY);
        }
    }
}

pub struct ConsoleTelemetry;

impl Telemetry for ConsoleTelemetry {
    fn record(&self, event: LogEvent) {
        let fields: Vec<String> = event.fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let line = JsValue::from_str(&format!("[taskr] {} {}", event.name, fields.join(" ")));
        match event.level {
            Level::Debug => console::debug_1(&line),
            Level::Info => console::info_1(&line),
            Level::Warn => console::warn_1(&line),
            Level::Error => console::error_1(&line),
        }
    }
}
