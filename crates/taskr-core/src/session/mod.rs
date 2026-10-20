//! Signing in and moving around, for shells that talk to a server.
//!
//! - `app`: the client-side auth ports (`AuthClient`, `SessionStore`).
//! - `presentation`: `AppState`, the parent state machine. It owns sign-in,
//!   navigation (which workspace, which screen) and session expiry, and
//!   composes the task list and members screens as children.

pub mod app;
pub mod presentation;
