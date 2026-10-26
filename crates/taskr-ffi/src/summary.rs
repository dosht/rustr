//! Native consumers call blocking operations off their UI thread. Listener calls
//! occur outside the shared reducer lock and may synchronously suspend/current.
use std::sync::Arc;
use taskr_client::{HttpApi, audio_summary::{SummaryController, SummaryView}};
use crate::{SessionVault, app::VaultStore};

#[derive(Clone, Debug, uniffi::Record)]
pub struct AudioWorkRowView { pub id: String, pub name: String, pub status: String, pub selected: bool }
#[derive(Clone, Debug, uniffi::Record)]
pub struct AudioSummaryView {
    pub works: Vec<AudioWorkRowView>,
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
impl From<SummaryView> for AudioSummaryView {
    fn from(view: SummaryView) -> Self {
        Self { works: view.works.into_iter().map(|row| AudioWorkRowView { id: row.id, name: row.name, status: row.status, selected: row.selected }).collect(),
            selected_id: view.selected_id, generation_id: view.generation_id, title: view.title, text: view.text,
            status_label: view.status_label, disclosure: view.disclosure, is_loading: view.is_loading,
            can_start: view.can_start, can_cancel: view.can_cancel, can_suspend: view.can_suspend, can_resume: view.can_resume,
            durable: view.durable, remote_outcome_unknown: view.remote_outcome_unknown, error: view.error }
    }
}
#[uniffi::export(callback_interface)]
pub trait AudioSummaryListener: Send + Sync { fn on_view(&self, view: AudioSummaryView); }

#[derive(uniffi::Object)]
pub struct AudioSummaryPresenter { controller: SummaryController, runtime: tokio::runtime::Runtime }
#[uniffi::export]
impl AudioSummaryPresenter {
    #[uniffi::constructor]
    pub fn new(api_url: String, vault: Box<dyn SessionVault>) -> Self {
        let api = Arc::new(HttpApi::new(api_url, Arc::new(VaultStore(vault))));
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().expect("native summary runtime starts");
        Self { controller: SummaryController::new(api), runtime }
    }
    pub fn current(&self) -> AudioSummaryView { self.controller.view().into() }
    pub fn refresh(&self) -> AudioSummaryView { self.runtime.block_on(self.controller.refresh()); self.current() }
    pub fn select(&self, work_id: String) -> AudioSummaryView { self.runtime.block_on(self.controller.select(work_id)); self.current() }
    /// Start returns the durable generation identity; watch never starts work.
    /// The caller retains request_id when retrying a lost start response.
    pub fn start(&self, request_id: String, accepted: bool) -> AudioSummaryView {
        self.runtime.block_on(self.controller.start(request_id, accepted)); self.current()
    }
    pub fn watch(&self, listener: Box<dyn AudioSummaryListener>) -> AudioSummaryView {
        self.runtime.block_on(self.controller.watch(|view| listener.on_view(view.into())));
        let view = self.current();
        listener.on_view(view.clone());
        view
    }
    pub fn suspend(&self) -> AudioSummaryView { self.controller.suspend(); self.current() }
    pub fn cancel(&self) -> AudioSummaryView { self.runtime.block_on(self.controller.cancel()); self.current() }
    pub fn dispose(&self) { self.controller.dispose(); }
}
