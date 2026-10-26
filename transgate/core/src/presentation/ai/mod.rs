//! AI panels (WEB-027..030, WEB-041/042, WEB-PRV-009/010, AI-F01..F10): the
//! consent gate and settings tab, the specialized analysis tab and the Ask AI
//! chat, as pure presentation over the unchanged P4 session API.
//!
//! The panels follow P5 D6: an AI start is the existing POST with a request key
//! minted by the app composer, the driver reads `x-generation-id`, drops the
//! body and the panel attaches the canonical watch. An unknown start outcome
//! replays the same key; an explicit retry is a new key. There is no cancel
//! control and no automatic model retry. Commercial and consent policy stays in
//! `crate::ai` on the host; these states only render its answers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::GenerationId;
use crate::ai::analysis::AnalysisType;
use crate::ai::ports::Period;

pub mod analysis;
pub mod chat;
pub mod consent;
pub mod fixtures;
pub mod section_copy;
pub use crate::presentation::failure::ApiFailure;

/// A presenter-minted request key (D6): exact replay of the same logical
/// request reuses it, an explicit retry receives a new one.
pub type RequestKey = Uuid;

/// Which decoder the driver attaches a watch with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// `llm_core` application frames through `ApplicationDecoder`.
    Text,
    /// The source's named analysis events (AI-003).
    Analysis,
}

/// One named analysis event as the driver decoded it from the canonical watch.
/// Every attachment begins from a full resnapshot, so a panel applies these
/// idempotently by section name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnalysisWire {
    SectionCount { total: usize },
    TypeDetected { kind: AnalysisType, confidence: f64 },
    SectionReady { section_type: String, data: serde_json::Value },
    Complete { tokens_used: i64, processing_time_ms: i64 },
    Error { code: String, message: String },
}

/// Why an attachment ended without a terminal event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WatchFailure {
    Disconnected,
    Http { failure: ApiFailure },
    Protocol,
}

/// The JSON body of a refused AI request (`transgate/http/src/ai.rs`): every
/// error carries `error` and `code`; a chat cap adds the counters and reset,
/// `ai_busy` names the running generation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRejection {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub current: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    /// ISO 8601, displayed by the source only inside the server's message.
    #[serde(default)]
    pub reset_date: Option<String>,
    #[serde(default)]
    pub reset_period: Option<Period>,
    #[serde(default)]
    pub generation_id: Option<GenerationId>,
}
