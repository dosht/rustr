//! Iyzico adapter: signed hosted checkout (pay-as-you-go and subscription),
//! subscription detail and fail-closed V3 renewal verification.
//!
//! Fixtures here are documentation- and source-derived, never recordings of
//! real transactions; see `fixtures/PROVENANCE.md`.

mod client;
pub mod sign;
mod webhook;

pub use client::{IyzicoClient, IyzicoConfig, parse_detail, segment};
pub use webhook::IyzicoWebhook;
