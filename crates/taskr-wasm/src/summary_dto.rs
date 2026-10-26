//! Generated TypeScript contracts for the independent audio-work presenter.
use serde::{Deserialize, Serialize};
use taskr_client::audio_summary::{AudioWorkRow, SummaryView};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioWorkRowView { pub id: String, pub name: String, pub status: String, pub selected: bool }
impl From<AudioWorkRow> for AudioWorkRowView {
    fn from(row: AudioWorkRow) -> Self { Self { id: row.id, name: row.name, status: row.status, selected: row.selected } }
}
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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
        Self { works: view.works.into_iter().map(Into::into).collect(), selected_id: view.selected_id,
            generation_id: view.generation_id, title: view.title, text: view.text, status_label: view.status_label,
            disclosure: view.disclosure, is_loading: view.is_loading, can_start: view.can_start,
            can_cancel: view.can_cancel, can_suspend: view.can_suspend, can_resume: view.can_resume,
            durable: view.durable, remote_outcome_unknown: view.remote_outcome_unknown, error: view.error }
    }
}
#[derive(Clone, Debug, Deserialize, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum AudioSummaryIntent {
    Refresh,
    Select { id: String },
    Start { request_id: String, accepted: bool },
    Resume,
    Suspend,
    Cancel,
}
