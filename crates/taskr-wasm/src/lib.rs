//! Browser composition root. Same controllers as every other shell, with
//! browser adapters: localStorage for storage and the session, the console for
//! telemetry, `Date.now()` for time, and `fetch` (through taskr-client) for the
//! server. Every call returns a Promise because the core's ports are async.
//!
//! - `TaskListPresenter`: signed out, tasks in localStorage. The web app gets
//!   offline support for free.
//! - `AppPresenter`: signed in against a taskr-server, with sign-in, workspaces,
//!   members and navigation.

mod adapters;
mod app_dto;
mod dto;

use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;
use taskr_client::{DevAuth, HttpApi, SupabaseAuth};
use taskr_core::session::app::AuthClient;
use taskr_core::session::presentation::AppController;
use taskr_core::shared::telemetry::{LogEvent, Telemetry};
use taskr_core::tasks::app::Tasks;
use taskr_core::tasks::presentation::{TaskListController, fixtures};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use adapters::{
    BrowserClock, ConsoleTelemetry, LocalStorageSessionStore, LocalStorageTaskListRepository,
};
use app_dto::{AppIntentDto, AppView};
use dto::{PreviewScreen, TaskIntent, TaskListView};

#[wasm_bindgen]
pub struct TaskListPresenter {
    controller: Rc<TaskListController>,
}

#[wasm_bindgen]
impl TaskListPresenter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> TaskListPresenter {
        let telemetry: Arc<dyn Telemetry> = Arc::new(ConsoleTelemetry);
        telemetry.record(LogEvent::info("app.started").field("shell", "web"));
        let tasks =
            Tasks::new(Arc::new(LocalStorageTaskListRepository), Arc::new(BrowserClock), telemetry);
        TaskListPresenter { controller: Rc::new(TaskListController::local(tasks)) }
    }

    /// Takes a `TaskIntent`, resolves to a `TaskListView` (see the generated TS types).
    pub fn dispatch(&self, intent: JsValue) -> Result<js_sys::Promise, JsError> {
        let intent: TaskIntent = serde_wasm_bindgen::from_value(intent)?;
        let controller = self.controller.clone();
        Ok(future_to_promise(async move {
            let vm = controller.dispatch(intent.into()).await;
            to_js(&TaskListView::from(vm)).map_err(JsValue::from)
        }))
    }

    pub fn current(&self) -> Result<JsValue, JsError> {
        to_js(&TaskListView::from(self.controller.view()))
    }
}

impl Default for TaskListPresenter {
    fn default() -> Self {
        Self::new()
    }
}

/// How the signed-in app reaches the outside world.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    /// Absolute API root, e.g. `https://app.example.com/api`.
    api_url: String,
    /// Supabase Auth root; absent means dev sign-in (any email, no password).
    auth_url: Option<String>,
    auth_key: Option<String>,
}

#[wasm_bindgen]
pub struct AppPresenter {
    controller: Rc<AppController>,
}

#[wasm_bindgen]
impl AppPresenter {
    /// `config`: `{ apiUrl, authUrl?, authKey? }`.
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<AppPresenter, JsError> {
        let config: AppConfig = serde_wasm_bindgen::from_value(config)?;
        ConsoleTelemetry.record(
            LogEvent::info("app.started")
                .field("shell", "web")
                .field("auth", if config.auth_url.is_some() { "supabase" } else { "dev" }),
        );
        let sessions = Arc::new(LocalStorageSessionStore);
        let api = Arc::new(HttpApi::new(config.api_url, sessions.clone()));
        let auth: Arc<dyn AuthClient> = match config.auth_url {
            Some(url) => Arc::new(SupabaseAuth::new(url, config.auth_key)),
            None => Arc::new(DevAuth),
        };
        let controller =
            AppController::new(auth, sessions, api.clone(), api, Arc::new(BrowserClock));
        Ok(AppPresenter { controller: Rc::new(controller) })
    }

    /// Takes an `AppIntentDto`, resolves to the final `AppView`. `on_view`, if
    /// given, is called with the current `AppView` each time requests are about
    /// to run, so the page can show the pending screen meanwhile.
    pub fn dispatch(
        &self,
        intent: JsValue,
        #[wasm_bindgen(js_name = onView)] on_view: Option<js_sys::Function>,
    ) -> Result<js_sys::Promise, JsError> {
        let intent: AppIntentDto = serde_wasm_bindgen::from_value(intent)?;
        let controller = self.controller.clone();
        Ok(future_to_promise(async move {
            let vm = controller
                .dispatch_with(intent.into(), |vm| {
                    if let Some(on_view) = &on_view {
                        // A throwing callback must not abort the requests in flight.
                        if let Ok(view) = to_js(&AppView::from(vm)) {
                            let _ = on_view.call1(&JsValue::NULL, &view);
                        }
                    }
                })
                .await;
            to_js(&AppView::from(vm)).map_err(JsValue::from)
        }))
    }

    pub fn current(&self) -> Result<JsValue, JsError> {
        to_js(&AppView::from(self.controller.view()))
    }
}

/// Fixture screens for Storybook and visual tests.
#[wasm_bindgen(js_name = previewScreens)]
pub fn preview_screens() -> Result<JsValue, JsError> {
    let screens: Vec<PreviewScreen> = fixtures::all()
        .into_iter()
        .map(|(name, vm)| PreviewScreen { name: name.to_string(), view: vm.into() })
        .collect();
    to_js(&screens)
}

/// Signed-in fixtures, by name, for the app frame.
#[wasm_bindgen(js_name = appPreviewScreens)]
pub fn app_preview_screens() -> Result<JsValue, JsError> {
    to_js(&app_fixtures())
}

fn app_fixtures() -> Vec<(String, AppView)> {
    taskr_core::session::presentation::fixtures::all()
        .into_iter()
        .map(|(name, vm)| (name.to_string(), vm.into()))
        .collect()
}

fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    Ok(value.serialize(&serializer)?)
}

/// Writes every fixture screen as JSON for Storybook and the component tests,
/// next to the generated TypeScript types, so stories render exactly what the
/// core produces without loading wasm.
#[cfg(test)]
mod export_fixtures {
    use super::*;

    #[test]
    fn writes_fixtures_json() {
        let tasks: serde_json::Map<String, serde_json::Value> = fixtures::all()
            .into_iter()
            .map(|(name, vm)| (name.to_string(), serde_json::to_value(TaskListView::from(vm)).unwrap()))
            .collect();
        let app: serde_json::Map<String, serde_json::Value> = app_fixtures()
            .into_iter()
            .map(|(name, view)| (name, serde_json::to_value(view).unwrap()))
            .collect();
        let json = serde_json::json!({ "tasks": tasks, "app": app });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/web/src/lib/core/fixtures.json");
        std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
        assert!(app.contains_key("owner-members"));
    }
}
