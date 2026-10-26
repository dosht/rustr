//! Real browser presenter: existing authenticated client, shared portable text
//! reducer/codec, progressive views delivered before the dispatch promise settles.
use std::rc::Rc;
use std::sync::Arc;
use taskr_client::{HttpApi, audio_summary::SummaryController};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{adapters::LocalStorageSessionStore, summary_dto::{AudioSummaryIntent, AudioSummaryView}, to_js};

#[wasm_bindgen]
pub struct AudioSummaryPresenter { controller: Rc<SummaryController> }
#[wasm_bindgen]
impl AudioSummaryPresenter {
    #[wasm_bindgen(constructor)]
    pub fn new(api_url: String) -> Self {
        let api = Arc::new(HttpApi::new(api_url, Arc::new(LocalStorageSessionStore)));
        Self { controller: Rc::new(SummaryController::new(api)) }
    }
    pub fn current(&self) -> Result<JsValue, JsError> { to_js(&AudioSummaryView::from(self.controller.view())) }
    pub fn dispatch(&self, intent: JsValue, on_view: Option<js_sys::Function>) -> Result<js_sys::Promise, JsError> {
        let intent: AudioSummaryIntent = serde_wasm_bindgen::from_value(intent)?;
        let controller = self.controller.clone();
        Ok(future_to_promise(async move {
            let publish = |view| {
                if let Some(callback) = &on_view && let Ok(view) = to_js(&AudioSummaryView::from(view)) { let _ = callback.call1(&JsValue::NULL, &view); }
            };
            let watch = match intent {
                AudioSummaryIntent::Refresh => { controller.refresh().await; false },
                AudioSummaryIntent::Select { id } => { controller.select(id).await; true },
                AudioSummaryIntent::Start { request_id, accepted } => { controller.start(request_id, accepted).await; true },
                AudioSummaryIntent::Resume => true,
                AudioSummaryIntent::Suspend => { controller.suspend(); false },
                AudioSummaryIntent::Cancel => { controller.cancel().await; false },
            };
            publish(controller.view());
            if watch { controller.watch(&publish).await; }
            let view = AudioSummaryView::from(controller.view());
            to_js(&view).map_err(JsValue::from)
        }))
    }
    /// Synchronous: invalidates in-flight callbacks before the UI unmounts.
    pub fn dispose(&self) { self.controller.dispose(); }
}
