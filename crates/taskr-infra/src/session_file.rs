use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use taskr_core::session::app::{Session, SessionStore};

/// The CLI's session, as a JSON file readable only by its owner.
pub struct FileSessionStore {
    path: PathBuf,
}

impl FileSessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Serialize, Deserialize)]
struct SessionV1 {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    email: String,
    #[serde(default)]
    expires_at: Option<i64>,
}

impl SessionStore for FileSessionStore {
    fn load(&self) -> Option<Session> {
        let s: SessionV1 = serde_json::from_str(&fs::read_to_string(&self.path).ok()?).ok()?;
        Some(Session {
            access_token: s.access_token,
            refresh_token: s.refresh_token,
            email: s.email,
            expires_at: s.expires_at,
        })
    }

    fn save(&self, s: &Session) {
        let file = SessionV1 {
            access_token: s.access_token.clone(),
            refresh_token: s.refresh_token.clone(),
            email: s.email.clone(),
            expires_at: s.expires_at,
        };
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(&file).expect("plain struct");
        if fs::write(&self.path, text).is_ok() {
            restrict_to_owner(&self.path);
        }
    }

    fn clear(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn restrict_to_owner(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_and_clears() {
        let path = std::env::temp_dir().join(format!("taskr-session-{}.json", std::process::id()));
        let store = FileSessionStore::new(&path);
        assert_eq!(store.load(), None);
        let s = Session {
            access_token: "t".into(),
            refresh_token: Some("r".into()),
            email: "a@b.co".into(),
            expires_at: Some(5),
        };
        store.save(&s);
        assert_eq!(store.load(), Some(s));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        store.clear();
        assert_eq!(store.load(), None);
    }
}
