//! Retention, privacy preferences and account erasure.
//!
//! Three meanings stay separate (PRV-014..017, PRV-032): the job list's
//! one-month window is a view filter, the daily sweep deletes transcript lines,
//! and account erasure is a durable saga over local and remote resources.

pub mod app;
pub mod erasure;
pub mod retention;
pub mod settings;
