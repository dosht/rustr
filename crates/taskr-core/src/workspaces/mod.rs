//! Bounded context: workspaces and who belongs to them.
//!
//! - `domain`: the `Workspace` aggregate (members, roles, "always one owner").
//! - `app`: use cases, the `WorkspaceRepository` port and the client-facing `WorkspacesApi`.
//! - `presentation`: the members screen state machine.

pub mod app;
pub mod domain;
pub mod presentation;
