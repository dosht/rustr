//! The shared core. Every shell (CLI, TUI, web, Android, iOS) talks to this crate
//! and nothing else talks to the outside world from here: all I/O goes through ports.

pub mod shared;
pub mod tasks;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
