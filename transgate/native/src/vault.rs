//! The stored session (D8): the OS keyring by default. A headless machine has no
//! keyring, so `TRANSGATE_SESSION_FILE` names a JSON file (mode 0600, replaced
//! atomically) that holds the same keys instead. Failures are kept, not
//! swallowed: the `NativePorts` storage calls cannot return them, so the shell
//! asks [`Vault::take_error`] after a sign-in.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const SERVICE: &str = "transgate";
pub const FILE_ENV: &str = "TRANSGATE_SESSION_FILE";

enum Backend {
    Keyring,
    File(PathBuf),
}

pub struct Vault {
    backend: Backend,
    last_error: Mutex<Option<String>>,
}

impl Vault {
    pub fn keyring() -> Vault {
        Vault { backend: Backend::Keyring, last_error: Mutex::default() }
    }

    pub fn file(path: impl Into<PathBuf>) -> Vault {
        Vault { backend: Backend::File(path.into()), last_error: Mutex::default() }
    }

    /// The file named by `TRANSGATE_SESSION_FILE` when set, else the OS keyring.
    pub fn from_env() -> Vault {
        match std::env::var_os(FILE_ENV).filter(|p| !p.is_empty()) {
            Some(path) => Vault::file(path),
            None => Vault::keyring(),
        }
    }

    /// The last storage failure since the previous call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.last_error.lock().expect("the error lock is never poisoned").take()
    }

    fn fail(&self, what: &str, error: impl std::fmt::Display) {
        *self.last_error.lock().expect("the error lock is never poisoned") = Some(format!("{what}: {error}"));
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match &self.backend {
            Backend::Keyring => match entry(key).and_then(|e| e.get_password()) {
                Ok(value) => Some(value),
                Err(keyring::Error::NoEntry) => None,
                Err(error) => {
                    self.fail("reading the OS keyring", error);
                    None
                }
            },
            Backend::File(path) => match read_map(path) {
                Ok(mut map) => map.remove(key),
                Err(error) => {
                    self.fail("reading the session file", error);
                    None
                }
            },
        }
    }

    pub fn set(&self, key: &str, value: &str) {
        match &self.backend {
            Backend::Keyring => {
                if let Err(error) = entry(key).and_then(|e| e.set_password(value)) {
                    self.fail("writing the OS keyring", error);
                }
            }
            Backend::File(path) => {
                let result = read_map(path).and_then(|mut map| {
                    map.insert(key.to_string(), value.to_string());
                    write_map(path, &map)
                });
                if let Err(error) = result {
                    self.fail("writing the session file", error);
                }
            }
        }
    }

    pub fn remove(&self, key: &str) {
        match &self.backend {
            Backend::Keyring => match entry(key).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(error) => self.fail("clearing the OS keyring", error),
            },
            Backend::File(path) => {
                let result = read_map(path).and_then(|mut map| if map.remove(key).is_some() { write_map(path, &map) } else { Ok(()) });
                if let Err(error) = result {
                    self.fail("clearing the session file", error);
                }
            }
        }
    }
}

fn entry(key: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key)
}

fn read_map(path: &Path) -> std::io::Result<BTreeMap<String, String>> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(BTreeMap::new()),
        Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

fn write_map(path: &Path, map: &BTreeMap<String, String>) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temp)?;
    file.write_all(serde_json::to_string(map).map_err(std::io::Error::other)?.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_vault_round_trips_keys_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/session.json");
        let vault = Vault::file(&path);
        assert_eq!(vault.get("a"), None);
        vault.set("a", "1");
        vault.set("b", "2");
        vault.remove("a");
        assert_eq!((vault.get("a"), vault.get("b")), (None, Some("2".into())));
        assert_eq!(vault.take_error(), None);
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions()) & 0o777, 0o600);
    }

    #[test]
    fn a_corrupt_file_is_reported_not_treated_as_signed_out_silently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        std::fs::write(&path, "{not json").unwrap();
        let vault = Vault::file(&path);
        assert_eq!(vault.get("a"), None);
        assert!(vault.take_error().is_some_and(|e| e.contains("reading the session file")));
        assert_eq!(vault.take_error(), None, "an error is reported once");
    }
}
