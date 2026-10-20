//! Which server a native shell signed in to, saved next to its data so the
//! CLI and the TUI share it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taskr_core::session::app::AuthClient;

use crate::{DevAuth, SupabaseAuth};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RemoteConfig {
    /// API root, for example `http://localhost:8787/api`.
    pub server: String,
    #[serde(default)]
    pub auth_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub dev: bool,
}

impl RemoteConfig {
    pub fn file(home: &Path) -> PathBuf {
        home.join("remote.json")
    }

    pub fn load(home: &Path) -> Option<RemoteConfig> {
        serde_json::from_str(&fs::read_to_string(Self::file(home)).ok()?).ok()
    }

    pub fn save(&self, home: &Path) -> Result<(), String> {
        fs::create_dir_all(home).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).expect("plain struct");
        fs::write(Self::file(home), text).map_err(|e| e.to_string())
    }

    pub fn remove(home: &Path) {
        let _ = fs::remove_file(Self::file(home));
    }

    /// `None` when neither an auth URL nor dev sign-in is configured.
    pub fn auth(&self) -> Option<Box<dyn AuthClient>> {
        match (&self.auth_url, self.dev) {
            (_, true) => Some(Box::new(DevAuth)),
            (Some(url), false) => {
                Some(Box::new(SupabaseAuth::new(url.clone(), self.api_key.clone())))
            }
            (None, false) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_and_removes() {
        let home = std::env::temp_dir().join(format!("taskr-remote-{}", std::process::id()));
        let c = RemoteConfig {
            server: "http://x/api".into(),
            auth_url: None,
            api_key: None,
            dev: true,
        };
        assert_eq!(RemoteConfig::load(&home), None);
        c.save(&home).unwrap();
        assert_eq!(RemoteConfig::load(&home), Some(c.clone()));
        assert!(c.auth().is_some());
        RemoteConfig::remove(&home);
        assert_eq!(RemoteConfig::load(&home), None);
    }
}
