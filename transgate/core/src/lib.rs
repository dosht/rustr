//! Transgate's transcription and privacy core: the Job aggregate, provider
//! routing and retry decisions, transcript normalization and editing, export
//! models, retention and account-erasure decisions, and the presentation state
//! the shells render.
//!
//! Pure Rust: no network, SQL or subprocess. Persistence, transports and HTTP
//! implement the ports declared here and in `media-ports`; billing is composed
//! through `billing-core`'s host transaction seam.

pub mod account;
pub mod ai;
pub mod api;
pub mod documents;
pub mod export;
pub mod flags;
pub mod ids;
pub mod jobs;
pub mod languages;
pub mod presentation;
pub mod privacy;
pub mod public_transcript;
pub mod schedule;
pub mod sources;
pub mod time;
/// Free-tool processor policy, owned by the processor slice.
pub mod tools;
pub mod transcript;

pub use ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
pub use jobs::domain::{Attempt, AttemptState, DeferredCharge, Job, JobSource, JobStatus, Provider, StorageProvider};
pub use languages::Language;
