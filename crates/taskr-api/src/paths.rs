//! Every route, relative to the API base URL. The server's router and the
//! client's requests are both built from these.

use taskr_core::shared::access::WorkspaceId;

pub const HEALTH: &str = "/health";
pub const ME: &str = "/me";

pub fn tasks(ws: &WorkspaceId) -> String {
    format!("/workspaces/{}/tasks", ws.0)
}

pub fn complete(ws: &WorkspaceId, id: u32) -> String {
    format!("/workspaces/{}/tasks/{id}/complete", ws.0)
}

pub fn reopen(ws: &WorkspaceId, id: u32) -> String {
    format!("/workspaces/{}/tasks/{id}/reopen", ws.0)
}

pub fn members(ws: &WorkspaceId) -> String {
    format!("/workspaces/{}/members", ws.0)
}

/// The same routes in the router's own syntax.
pub mod routes {
    pub const TASKS: &str = "/workspaces/{ws}/tasks";
    pub const COMPLETE: &str = "/workspaces/{ws}/tasks/{id}/complete";
    pub const REOPEN: &str = "/workspaces/{ws}/tasks/{id}/reopen";
    pub const MEMBERS: &str = "/workspaces/{ws}/members";
}
