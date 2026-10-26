//! Presentation state machines the shells render (P5 web, P8 native/CLI/TUI).
//! Each is `update(event) -> effects` plus `view()`; shells execute effects and
//! draw. Copy is a [`msg::Msg`]: a source translation key with typed
//! arguments that the shell's catalog translates, as the source's `t()` did.

/// AI panels (consent, analysis, chat), owned by the AI panel slice.
pub mod ai;
pub mod app;
pub mod billing;
pub mod data_management;
pub mod dates;
pub mod failure;
pub mod feedback;
pub mod fixtures;
pub mod job_list;
pub mod layout;
pub mod msg;
pub mod route;
pub mod settings;
pub mod support;
pub mod telemetry;
pub mod transcript;
pub mod upload_queue;
