//! Bounded context: personal tasks.
//!
//! - `domain`: the `TaskList` aggregate and its invariants (pure).
//! - `app`: use cases and the ports they own (`TaskListRepository`).
//! - `presentation`: the shared ViewModel state machine every UI renders.

pub mod app;
pub mod domain;
pub mod presentation;
