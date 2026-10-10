//! Browser composition root. Same `TaskListController` as every other shell,
//! with browser adapters: localStorage for storage, the console for telemetry,
//! `Date.now()` for time. The web app gets offline support for free.

mod adapters;
mod dto;

use std::sync::Arc;

use taskr_core::shared::telemetry::{LogEvent, Telemetry};
use taskr_core::tasks::app::Tasks;
use taskr_core::tasks::presentation::{TaskListController, fixtures};
use wasm_bindgen::prelude::*;

use adapters::{BrowserClock, ConsoleTelemetry, LocalStorageTaskListRepository};
use dto::{PreviewScreen, TaskIntent, TaskListView};

#[wasm_bindgen]
pub struct TaskListPresenter {
    controller: TaskListController,
}

#[wasm_bindgen]
impl TaskListPresenter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> TaskListPresenter {
        let telemetry: Arc<dyn Telemetry> = Arc::new(ConsoleTelemetry);
        telemetry.record(LogEvent::info("app.started").field("shell", "web"));
        let tasks =
            Tasks::new(Arc::new(LocalStorageTaskListRepository), Arc::new(BrowserClock), telemetry);
        TaskListPresenter { controller: TaskListController::new(tasks) }
    }

    /// Takes a `TaskIntent`, returns a `TaskListView` (see the generated TS types).
    pub fn dispatch(&mut self, intent: JsValue) -> Result<JsValue, JsError> {
        let intent: TaskIntent = serde_wasm_bindgen::from_value(intent)?;
        to_js(&TaskListView::from(self.controller.dispatch(intent.into())))
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

/// Fixture screens for Storybook and visual tests.
#[wasm_bindgen(js_name = previewScreens)]
pub fn preview_screens() -> Result<JsValue, JsError> {
    let screens: Vec<PreviewScreen> = fixtures::all()
        .into_iter()
        .map(|(name, vm)| PreviewScreen { name: name.to_string(), view: vm.into() })
        .collect();
    to_js(&screens)
}

fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    Ok(value.serialize(&serializer)?)
}
