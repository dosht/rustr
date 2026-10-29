//! The Transgate terminal shell.
pub mod config;
pub mod design_tokens;
pub mod host;
pub mod run;
pub mod ui;
pub mod view;

#[cfg(test)]
#[path = "../../cli/tests/support/mod.rs"]
mod auth_test_support;
