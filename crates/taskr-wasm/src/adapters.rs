use js_sys::{Date, Reflect};
use taskr_core::shared::clock::{Clock, Millis};
use taskr_core::shared::telemetry::{Level, LogEvent, Telemetry};
use taskr_core::tasks::app::{StorageError, TaskListRepository};
use taskr_core::tasks::domain::TaskList;
use taskr_infra::snapshot_json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Storage, console};

const STORAGE_KEY: &str = "taskr.tasks";

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

impl TaskListRepository for LocalStorageTaskListRepository {
    fn load(&self) -> Result<TaskList, StorageError> {
        match Self::storage()?.get_item(STORAGE_KEY) {
            Ok(Some(text)) => snapshot_json::decode(&text),
            Ok(None) => Ok(TaskList::new()),
            Err(_) => Err(StorageError("could not read localStorage".into())),
        }
    }

    fn save(&self, list: &TaskList) -> Result<(), StorageError> {
        Self::storage()?
            .set_item(STORAGE_KEY, &snapshot_json::encode(list))
            .map_err(|_| StorageError("could not write localStorage (quota?)".into()))
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
