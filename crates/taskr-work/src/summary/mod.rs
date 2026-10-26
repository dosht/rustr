//! Explicit taskr audio-work summaries using the shared LLM port and text reducer.
//! Existing identity, account ownership and erasure apply; no Transgate consent/quota policy.

pub mod app;
pub mod compose;
pub mod domain;
pub mod error;
pub mod http;

#[cfg(test)]
mod pg_tests;
