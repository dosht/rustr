//! The shared core. Every shell (CLI, TUI, web, Android, iOS, server) talks to
//! this crate and nothing else talks to the outside world from here: all I/O
//! goes through ports.

pub mod api;
pub mod session;
pub mod shared;
pub mod tasks;
pub mod workspaces;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
