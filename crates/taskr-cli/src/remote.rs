//! Signed-in mode: which server, how to sign in, and the HTTP adapters for it.

use std::path::Path;
use std::sync::Arc;

use taskr_client::HttpApi;
pub use taskr_client::config::RemoteConfig as Config;
use taskr_core::session::app::SessionStore;
use taskr_core::shared::access::WorkspaceId;
use taskr_core::workspaces::app::WorkspacesApi;
use taskr_infra::paths;
use taskr_infra::session_file::FileSessionStore;

use crate::Failure;

pub struct Remote {
    config: Config,
    pub api: Arc<HttpApi>,
}

impl Remote {
    /// Present only after a successful `login` that has not been logged out.
    pub fn open(home: &Path) -> Option<Remote> {
        let config = Config::load(home)?;
        let store: Arc<dyn SessionStore> =
            Arc::new(FileSessionStore::new(paths::session_file(home)));
        store.load()?;
        let api = Arc::new(HttpApi::new(config.server.clone(), store));
        Some(Remote { config, api })
    }

    pub fn server(&self) -> &str {
        &self.config.server
    }

    /// By id or by name (ignoring case); the first workspace when not given.
    pub async fn workspace(&self, wanted: Option<&str>) -> Result<WorkspaceId, Failure> {
        let me = self.api.me().await?;
        let found = match wanted {
            None => me.workspaces.first(),
            Some(w) => me.workspaces.iter().find(|s| s.id.0 == w || s.name.eq_ignore_ascii_case(w)),
        };
        found.map(|s| s.id.clone()).ok_or_else(|| {
            let names: Vec<_> = me.workspaces.iter().map(|w| w.name.as_str()).collect();
            Failure::rejected(format!(
                "No workspace {:?}. You have: {}.",
                wanted.unwrap_or(""),
                names.join(", ")
            ))
        })
    }
}
