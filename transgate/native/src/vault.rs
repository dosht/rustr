//! The stored session (D8): the OS keyring by default. A headless machine has no
//! keyring, so `TRANSGATE_SESSION_FILE` names a JSON file (mode 0600, replaced
//! atomically) that holds the same keys instead. Failures are kept, not
//! swallowed: the `NativePorts` storage calls cannot return them, so the shell
//! asks [`Vault::take_error`] after a sign-in.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use transgate_client::auth::{LockGuard, SessionLock};
use transgate_client::future::BoxFuture;

struct RefreshLock(PathBuf, fn(&std::fs::File) -> std::io::Result<()>, Arc<Mutex<Option<String>>>);

impl SessionLock for RefreshLock {
    fn acquire(&self) -> BoxFuture<'_, Result<LockGuard, String>> {
        let path = self.0.clone();
        let acquire = self.1;
        let errors = self.2.clone();
        Box::pin(async move {
            let held = tokio::task::spawn_blocking(move || {
                let file = open_lock_file(&path)?;
                acquire(&file)?;
                Ok::<_, std::io::Error>(file)
            })
                .await.map_err(|error| format!("refresh lock worker failed: {error}"))
                .and_then(|result| result.map_err(|error| format!("refresh lock unavailable: {error}")));
            match held {
                Ok(held) => Ok(LockGuard::new(move || drop(held))),
                Err(error) => {
                    *errors.lock().expect("the error lock is never poisoned") = Some(error.clone());
                    Err(error)
                }
            }
        })
    }
}

fn open_lock_file(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(std::io::Error::other("lock file must be private and owned by this user"));
        }
    }
    Ok(file)
}

fn lock_file(path: &Path) -> std::io::Result<std::fs::File> {
    let file = open_lock_file(path)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(error) => return Err(std::io::Error::other(format!("session storage lock unavailable: {error}"))),
        }
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub const SERVICE: &str = "transgate";
pub const FILE_ENV: &str = "TRANSGATE_SESSION_FILE";

enum Backend {
    Keyring,
    File(PathBuf),
}

pub struct Vault {
    backend: Backend,
    namespace: String,
    last_error: Arc<Mutex<Option<String>>>,
}

impl Vault {
    pub fn keyring() -> Vault {
        Vault { namespace: String::new(), backend: Backend::Keyring, last_error: Arc::default() }
    }

    pub fn file(path: impl Into<PathBuf>) -> Vault {
        Vault { namespace: String::new(), backend: Backend::File(path.into()), last_error: Arc::default() }
    }

    /// The file named by `TRANSGATE_SESSION_FILE` when set, else the OS keyring.
    pub fn from_env() -> Vault {
        match std::env::var_os(FILE_ENV).filter(|p| !p.is_empty()) {
            Some(path) => Vault::file(path),
            None => Vault::keyring(),
        }
    }

    pub fn for_host(mut self, host: &crate::HostConfig) -> Self {
        self.namespace = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    host.api_url.trim_end_matches('/'),
                    host.auth_url.trim_end_matches('/')
                ))
                .expect("deployment identity serializes")
            )
        );
        self
    }

    pub fn refresh_lock(&self) -> std::io::Result<Arc<dyn SessionLock>> {
        let path = match &self.backend {
            Backend::File(path) => sibling(path, &format!(".{}.refresh.lock", self.namespace)),
            Backend::Keyring => {
                let home = std::env::var_os("HOME").ok_or_else(|| {
                    std::io::Error::other("HOME is required for the keyring refresh lock")
                })?;
                PathBuf::from(home)
                    .join(".local/state/transgate/locks")
                    .join(format!("{}.lock", self.namespace))
            }
        };
        drop(open_lock_file(&path)?);
        Ok(Arc::new(RefreshLock(path, std::fs::File::lock, self.last_error.clone())))
    }

    fn key(&self, key: &str) -> String {
        if self.namespace.is_empty() { key.into() } else { format!("{}:{key}", self.namespace) }
    }

    pub fn has_error(&self) -> bool {
        self.last_error.lock().expect("the error lock is never poisoned").is_some()
    }

    /// The last storage failure since the previous call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.last_error.lock().expect("the error lock is never poisoned").take()
    }

    fn fail(&self, what: &str, error: impl std::fmt::Display) {
        *self.last_error.lock().expect("the error lock is never poisoned") = Some(format!("{what}: {error}"));
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let key = self.key(key);
        match &self.backend {
            Backend::Keyring => match entry(&key).and_then(|e| e.get_password()) {
                Ok(value) => Some(value),
                Err(keyring::Error::NoEntry) => None,
                Err(error) => {
                    self.fail("reading the OS keyring", error);
                    None
                }
            },
            Backend::File(path) => match read_map(path) {
                Ok(mut map) => map.remove(&key),
                Err(error) => {
                    self.fail("reading the session file", error);
                    None
                }
            },
        }
    }

    pub fn set(&self, key: &str, value: &str) {
        let key = self.key(key);
        match &self.backend {
            Backend::Keyring => {
                if let Err(error) = entry(&key).and_then(|e| e.set_password(value)) {
                    self.fail("writing the OS keyring", error);
                }
            }
            Backend::File(path) => {
                let result = lock_file(&sibling(path, ".lock")).and_then(|_held| {
                    read_map(path).and_then(|mut map| {
                        map.insert(key.to_string(), value.to_string());
                        write_map(path, &map)
                    })
                });
                if let Err(error) = result {
                    self.fail("writing the session file", error);
                }
            }
        }
    }

    pub fn remove(&self, key: &str) {
        let key = self.key(key);
        match &self.backend {
            Backend::Keyring => match entry(&key).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(error) => self.fail("clearing the OS keyring", error),
            },
            Backend::File(path) => {
                let result = lock_file(&sibling(path, ".lock")).and_then(|_held| {
                    read_map(path).and_then(|mut map| {
                        if map.remove(&key).is_some() { write_map(path, &map) } else { Ok(()) }
                    }) });
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
    let parent =
        path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(serde_json::to_string(map).map_err(std::io::Error::other)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(std::io::Error::other)?;
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restoration_failure_is_visible(vault: Arc<Vault>, lock: Arc<dyn SessionLock>) {
        use transgate_core::presentation::app::{AppIntent, AppVm};
        struct Listener;
        impl transgate_ffi::ViewListener for Listener {
            fn view_changed(&self, _: AppVm) {}
        }
        let account = transgate_core::presentation::fixtures::account().0;
        vault.set("transgate.session", &format!(r#"{{"access_token":"old","refresh_token":"old-ref","expires_at":1,"user_id":"{account}","email":"a@example.test"}}"#));
        let signal = Arc::new(crate::Signal::default());
        let ports = Arc::new(crate::TerminalPorts::new(vault, std::env::temp_dir(), Box::new(|_| {}), signal));
        let host = crate::HostConfig::from_origin("https://a.example", None);
        let presenter = transgate_ffi::TransgatePresenter::with_session_lock(host.presenter("http://localhost", "test"), ports, Arc::new(Listener), lock).unwrap();
        presenter.dispatch(AppIntent::Started { url: "/home".into(), viewport: transgate_core::presentation::layout::Viewport { touch: false, width: 1280 }, utc_offset_minutes: 0 }).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match presenter.current().unwrap() {
                AppVm::Restoring { .. } => assert!(std::time::Instant::now() < deadline, "restoration must report the lock failure"),
                AppVm::SignedOut(out) => { assert!(out.auth.error.is_some()); break; }
                other => panic!("unexpected view: {other:?}"),
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        presenter.dispose();
    }

    #[test]
    fn invalidated_lock_path_completes_presenter_restoration_with_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let vault = Arc::new(Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None)));
        let lock = vault.refresh_lock().unwrap();
        let lock_path = sibling(&path, &format!(".{}.refresh.lock", vault.namespace));
        std::fs::remove_file(&lock_path).unwrap();
        std::fs::create_dir(&lock_path).unwrap();
        restoration_failure_is_visible(vault, lock);
    }

    #[test]
    fn lock_syscall_failure_completes_presenter_restoration_with_error() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Arc::new(Vault::file(dir.path().join("session.json")).for_host(&crate::HostConfig::from_origin("https://a.example", None)));
        let lock = Arc::new(RefreshLock(dir.path().join("refresh.lock"), |_| Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "locking unsupported")), vault.last_error.clone()));
        restoration_failure_is_visible(vault, lock);
    }

    #[test]
    fn invalidated_refresh_lock_returns_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let vault = Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None));
        let lock = vault.refresh_lock().unwrap();
        let lock_path = sibling(&path, &format!(".{}.refresh.lock", vault.namespace));
        std::fs::remove_file(&lock_path).unwrap();
        std::fs::create_dir(&lock_path).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let joined = runtime.block_on(async { tokio::spawn(async move { lock.acquire().await }).await });
        assert!(joined.is_ok(), "lock failure must return through auth, not kill its task");
        assert!(joined.unwrap().is_err());
    }

    #[test]
    fn vault_process_worker() {
        let Ok(path) = std::env::var("TRANSGATE_VAULT_TEST_PATH") else { return };
        let vault =
            Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None));
        if let Ok(worker) = std::env::var("TRANSGATE_VAULT_TEST_WRITER") {
            for key in 0..40 {
                vault.set(&format!("{worker}-{key}"), "saved");
                assert_eq!(vault.take_error(), None);
            }
        } else {
            let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let lock = vault.refresh_lock().unwrap();
            std::fs::write(format!("{path}.ready"), "ready").unwrap();
            let _held = runtime.block_on(lock.acquire()).unwrap();
            std::fs::write(format!("{path}.acquired"), "acquired").unwrap();
        }
    }

    fn worker(path: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "vault::tests::vault_process_worker", "--nocapture"])
            .env("TRANSGATE_VAULT_TEST_PATH", path);
        command
    }

    #[test]
    fn refresh_lock_excludes_another_process_until_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let vault =
            Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None));
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let lock = vault.refresh_lock().unwrap();
        let held = runtime.block_on(lock.acquire()).unwrap();
        let mut child = worker(&path).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sibling(&path, ".ready").exists() {
            assert!(std::time::Instant::now() < deadline);
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!sibling(&path, ".acquired").exists());
        assert!(child.try_wait().unwrap().is_none());
        drop(held);
        assert!(child.wait().unwrap().success());
        assert!(sibling(&path, ".acquired").exists());
    }

    #[test]
    fn concurrent_process_transactions_preserve_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let children: Vec<_> = (0..4)
            .map(|writer| {
                worker(&path)
                    .env("TRANSGATE_VAULT_TEST_WRITER", writer.to_string())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let vault =
            Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None));
        for writer in 0..4 {
            for key in 0..40 {
                assert_eq!(vault.get(&format!("{writer}-{key}")).as_deref(), Some("saved"));
            }
        }
    }

    #[test]
    fn deployments_do_not_adopt_each_others_or_legacy_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        Vault::file(&path).set("transgate.session", "legacy");
        let a =
            Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://a.example", None));
        let b =
            Vault::file(&path).for_host(&crate::HostConfig::from_origin("https://b.example", None));
        assert_eq!(a.get("transgate.session"), None);
        a.set("transgate.session", "a-token");
        a.set("transgate.pkce", "a-verifier");
        assert_eq!(b.get("transgate.session"), None);
        assert_eq!(b.get("transgate.pkce"), None);
        b.remove("transgate.session");
        assert_eq!(a.get("transgate.session").as_deref(), Some("a-token"));
    }

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
