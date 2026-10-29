//! What the terminal loop does for an [`Action`]: dispatch an intent, hold a file for
//! upload, run the Google loopback flow, and surface host-side facts (a saved file,
//! a keyring failure) as notices. No product rule lives here; the presenter decides.
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use transgate_core::presentation::app::{AppIntent, AppVm, AuthFailure, AuthIntent, UploadIntent};
use transgate_core::presentation::msg::{Msg, auth};
use transgate_native::{HostConfig, Loopback, OpenUrl, Shell, Signal, TerminalPorts, Vault, browser};

use crate::config::Config;
use crate::ui::{Action, UiState};

const USER_AGENT: &str = concat!("transgate-tui/", env!("CARGO_PKG_VERSION"));
/// The origin auth redirects return to when no loopback listener is involved (never contacted).
const IDLE_SITE: &str = "http://127.0.0.1";
const BROWSER_WAIT: Duration = Duration::from_secs(300);

/// Exit codes (P8 D13), the same as the CLI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Ok = 0,
    Refused = 1,
    Usage = 2,
    Auth = 3,
    Network = 4,
}

impl From<Exit> for std::process::ExitCode {
    fn from(exit: Exit) -> std::process::ExitCode {
        std::process::ExitCode::from(exit as u8)
    }
}

/// An unreachable identity service exits 4; every other refusal the person can fix exits 3.
pub fn auth_exit(error: &Msg) -> Exit {
    if *error == AuthFailure::Network.message() || *error == AuthFailure::Unavailable.message() { Exit::Network } else { Exit::Auth }
}

/// How the run ends, from the last view: a sign-in error that was never cleared exits
/// with its code, a session that could not be read is a network failure, an erased
/// account is a refusal, anything else is a normal quit.
pub fn exit_for(vm: &AppVm) -> Exit {
    match vm {
        AppVm::SignedOut(out) => out.auth.error.as_ref().map_or(Exit::Ok, auth_exit),
        AppVm::SignedIn(inn) if inn.session_error.is_some() => Exit::Network,
        AppVm::Erased { .. } => Exit::Refused,
        _ => Exit::Ok,
    }
}

/// Whether anything answers at the host's address (a TCP connect; the answer's meaning is core's).
pub fn reachable(origin: &str, timeout: Duration) -> bool {
    let rest = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or("");
    let has_port = authority.rfind(':').is_some_and(|i| !authority[i..].contains(']'));
    let target = if has_port { authority.to_string() } else { format!("{authority}:{}", if origin.starts_with("https") { 443 } else { 80 }) };
    target.to_socket_addrs().ok().is_some_and(|mut addrs| {
        addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
    })
}

pub struct Session {
    pub shell: Shell,
    pub ports: Arc<TerminalPorts>,
}

/// Whether the loop goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

struct GoogleWait {
    origin: String,
    callback: Receiver<Option<String>>,
    session: Option<String>,
}

pub struct Host {
    config: Config,
    host: HostConfig,
    vault: Arc<Vault>,
    session: Session,
    google: Option<GoogleWait>,
    pending_google: Option<(Loopback, String)>,
    link: Arc<Mutex<Option<String>>>,
    saved_seen: usize,
}

impl Host {
    pub fn start(config: Config) -> Result<Host, String> {
        let host = HostConfig::from_origin(&config.host, config.auth_key.clone());
        let vault = Arc::new(Vault::from_env().for_host(&host));
        let session = open_session(&config, &host, &vault, &home_url(&config.locale), IDLE_SITE, Box::new(|_| {}))?;
        Ok(Host { config, host, vault, session, google: None,
            pending_google: None, link: Arc::default(), saved_seen: 0 })
    }

    pub fn locale(&self) -> &str {
        &self.config.locale
    }

    pub fn view(&self) -> Result<AppVm, String> {
        self.session.shell.view().map_err(|e| e.to_string())
    }

    pub fn apply(&mut self, action: Action, ui: &mut UiState) -> Flow {
        match action {
            Action::Quit => return Flow::Quit,
            Action::Intent(intent) => self.dispatch(intent, ui),
            Action::PickFile(path) => self.pick(path, ui),
            Action::GoogleSignIn => self.google(ui),
        }
        Flow::Continue
    }

    fn cancel_google(&mut self, ui: &mut UiState) {
        self.google = None;
        self.pending_google = None;
        self.link = Arc::default();
        ui.link = None;
    }

    fn dispatch(&mut self, intent: AppIntent, ui: &mut UiState) {
        if matches!(&intent, AppIntent::Auth(_) | AppIntent::SignOutRequested | AppIntent::LocaleChosen { .. }) {
            self.cancel_google(ui);
        }
        if let Err(error) = self.session.shell.dispatch(intent) {
            ui.notice = Some(error.to_string());
        }
    }

    fn pick(&mut self, path: PathBuf, ui: &mut UiState) {
        match self.session.ports.pick(&path) {
            Ok(file) => {
                self.dispatch(AppIntent::Upload(UploadIntent::Picked { files: vec![file] }), ui)
            }
            Err(error) => ui.notice = Some(format!("{}: {error}", path.display())),
        }
    }

    /// Core builds the authorization URL from the site URL, so the presenter that asks
    /// for it must be started with the loopback origin; the browser returns there with
    /// a PKCE code, and a second presenter started at that target exchanges it.
    fn google(&mut self, ui: &mut UiState) {
        if self.google.is_some() || self.pending_google.is_some() {
            return;
        }
        self.cancel_google(ui);
        let loopback = match Loopback::bind().and_then(|l| l.origin().map(|origin| (l, origin))) {
            Ok(pair) => pair,
            Err(error) => {
                ui.notice = Some(error.to_string());
                return;
            }
        };
        let (loopback, origin) = loopback;
        let link = self.link.clone();
        let open: OpenUrl = Box::new(move |url| {
            if !browser::open(url) {
                *link.lock().expect("the link lock is never poisoned") = Some(url.to_string());
            }
        });
        let session = match open_session(&self.config, &self.host, &self.vault, &home_url(&self.config.locale), &origin, open) {
            Ok(session) => session,
            Err(error) => {
                ui.notice = Some(error);
                return;
            }
        };
        self.session = session;
        self.pending_google = Some((loopback, origin));
    }

    fn advance_google(&mut self, ui: &mut UiState) {
        if self.pending_google.is_none() {
            return;
        }
        match self.view() {
            Ok(AppVm::Restoring { .. }) => return,
            Ok(AppVm::SignedOut(out)) if out.auth.error.is_none() => {}
            Ok(AppVm::SignedOut(out)) => {
                ui.notice = out
                    .auth
                    .error
                    .as_ref()
                    .map(|error| crate::view::say(&self.config.locale, error));
                self.cancel_google(ui);
                return;
            }
            Ok(_) => {
                self.cancel_google(ui);
                return;
            }
            Err(error) => {
                ui.notice = Some(error);
                self.cancel_google(ui);
                return;
            }
        }
        let (loopback, origin) = self.pending_google.take().expect("pending Google sign-in");
        if let Err(error) = self.session.shell.dispatch(AppIntent::Auth(AuthIntent::GoogleChosen)) {
            ui.notice = Some(error.to_string());
            return;
        }
        let page = crate::view::say(&self.config.locale, &auth("sign_in.loading_button_label"));
        let (send, callback) = channel();
        std::thread::spawn(move || {
            let target = loopback.wait_callback(BROWSER_WAIT, &page).ok().flatten();
            let _ = send.send(target);
        });
        self.google = Some(GoogleWait { origin, callback, session: self.vault.get("transgate.session") });
    }

    /// Once per loop turn: the loopback answer, and the host facts worth a notice.
    pub fn poll(&mut self, ui: &mut UiState) {
        self.advance_google(ui);
        if self.google.as_ref().is_some_and(|wait| wait.session != self.vault.get("transgate.session") || !matches!(self.view(), Ok(AppVm::SignedOut(out)) if !out.auth.busy && out.auth.error.is_none())) {
            self.cancel_google(ui);
        }
        if let Some(link) = self.link.lock().expect("the link lock is never poisoned").clone() {
            ui.link = Some(link);
        }
        if let Some(wait) = &self.google {
            match wait.callback.try_recv() {
                Err(TryRecvError::Empty) => {}
                Ok(target) => {
                    let origin = wait.origin.clone();
                    self.google = None;
                    self.finish_google(target, &origin, ui);
                }
                Err(TryRecvError::Disconnected) => {
                    self.cancel_google(ui);
                }
            }
        }
        let saved = self.session.ports.saved();
        if saved.len() > self.saved_seen {
            self.saved_seen = saved.len();
            ui.notice = saved.last().map(|p| p.display().to_string());
        }
        if let Some(error) = self.session.ports.take_save_error() {
            ui.notice = Some(error);
        }
        if let Some(error) = self.vault.take_error() {
            self.cancel_google(ui);
            ui.notice = Some(error);
        }
    }

    fn finish_google(&mut self, target: Option<String>, origin: &str, ui: &mut UiState) {
        self.cancel_google(ui);
        let Some(target) = target else { return };
        if !matches!(self.view(), Ok(AppVm::SignedOut(out)) if !out.auth.busy && out.auth.error.is_none()) {
            return;
        }
        match open_session(&self.config, &self.host, &self.vault, &target, origin, Box::new(|_| {})) {
            Ok(session) => {
                self.session = session;
                self.saved_seen = 0;
            }
            Err(error) => ui.notice = Some(error),
        }
    }
}

fn home_url(locale: &str) -> String {
    if locale == "en" { "/home".into() } else { format!("/{locale}/home") }
}

fn open_session(config: &Config, host: &HostConfig, vault: &Arc<Vault>, url: &str, site_url: &str, open: OpenUrl) -> Result<Session, String> {
    let signal = Arc::new(Signal::default());
    let ports = Arc::new(TerminalPorts::new(vault.clone(), config.out.clone(), open, signal.clone()));
    let shell = Shell::start(host.presenter(site_url, USER_AGENT), ports.clone(), signal, url).map_err(|e| e.to_string())?;
    Ok(Session { shell, ports })
}

#[cfg(test)]
mod tests {
    use super::*;
    use transgate_core::presentation::fixtures;

    #[test]
    fn storage_contention_keeps_view_and_quit_responsive_and_allows_retry() {
        let service = crate::auth_test_support::host();
        let dir = tempfile::tempdir().unwrap();
        let config = Config { host: service.origin.clone(), auth_key: None, locale: "en".into(), out: dir.path().into() };
        let host_config = HostConfig::from_origin(&config.host, None);
        let path = dir.path().join("session.json");
        let vault = Arc::new(Vault::file(&path).for_host(&host_config));
        let session = open_session(&config, &host_config, &vault, "/home", IDLE_SITE, Box::new(|_| {})).unwrap();
        session.shell.settled(Duration::from_secs(5)).unwrap();
        let mut host = Host { config, host: host_config, vault: vault.clone(), session, google: None, pending_google: None, link: Arc::default(), saved_seen: 0 };
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let held = options.open(dir.path().join("session.json.lock")).unwrap();
        held.lock().unwrap();
        let mut ui = UiState::default();
        let sign_in = |host: &mut Host, ui: &mut UiState| {
            for intent in [AuthIntent::EmailChanged { text: crate::auth_test_support::EMAIL.into() }, AuthIntent::PasswordChanged { text: "correct".into() }, AuthIntent::Submitted] {
                host.apply(Action::Intent(AppIntent::Auth(intent)), ui);
            }
        };
        sign_in(&mut host, &mut ui);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !vault.has_error() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = std::time::Instant::now();
        host.view().unwrap();
        host.poll(&mut ui);
        assert_eq!(host.apply(Action::Quit, &mut ui), Flow::Quit);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(ui.notice.as_deref().is_some_and(|notice| notice.contains("writing the session file")));
        drop(held);
        host.session.shell.wait(Duration::from_secs(5), |view| matches!(view, AppVm::SignedOut(_)).then_some(())).unwrap();
        sign_in(&mut host, &mut ui);
        host.session.shell.wait(Duration::from_secs(5), |view| matches!(view, AppVm::SignedIn(_)).then_some(())).unwrap();
        assert!(vault.get("transgate.session").is_some());
    }

    #[test]
    fn expired_or_disconnected_google_wait_clears_links_and_allows_retry() {
        for disconnected in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let config = Config { host: "https://unused.example".into(), auth_key: None, locale: "en".into(), out: dir.path().into() };
            let host_config = HostConfig::from_origin(&config.host, None);
            let vault = Arc::new(Vault::file(dir.path().join("session.json")).for_host(&host_config));
            let session = open_session(&config, &host_config, &vault, "/home", IDLE_SITE, Box::new(|_| {})).unwrap();
            session.shell.settled(Duration::from_secs(5)).unwrap();
            let (send, callback) = channel();
            if !disconnected { send.send(None).unwrap(); }
            drop(send);
            let old_link = Arc::new(Mutex::new(Some("expired URL".into())));
            let mut host = Host { config, host: host_config, vault, session, google: Some(GoogleWait { origin: IDLE_SITE.into(), callback, session: None }), pending_google: None, link: old_link.clone(), saved_seen: 0 };
            let mut ui = UiState::default();
            host.poll(&mut ui);
            *old_link.lock().unwrap() = Some("late URL".into());
            host.poll(&mut ui);
            assert!(host.google.is_none());
            assert!(ui.link.is_none());
            host.google(&mut ui);
            assert!(host.pending_google.is_some());
            host.cancel_google(&mut ui);
        }
    }

    #[test]
    fn password_sign_in_supersedes_google_callback_and_fallback_link() {
        use std::io::Write;
        let service = crate::auth_test_support::host();
        let dir = tempfile::tempdir().unwrap();
        let config = Config { host: service.origin.clone(), auth_key: None, locale: "en".into(), out: dir.path().into() };
        let host_config = HostConfig::from_origin(&config.host, None);
        let vault = Arc::new(Vault::file(dir.path().join("session.json")).for_host(&host_config));
        let loopback = Loopback::bind().unwrap();
        let origin = loopback.origin().unwrap();
        let link: Arc<Mutex<Option<String>>> = Arc::default();
        let old_link = link.clone();
        let session = open_session(&config, &host_config, &vault, "/home", &origin, Box::new(move |url| *old_link.lock().unwrap() = Some(url.into()))).unwrap();
        let old_link = link.clone();
        let mut host = Host { config, host: host_config, vault: vault.clone(), session, google: None, pending_google: Some((loopback, origin.clone())), link, saved_seen: 0 };
        let mut ui = UiState::default();
        host.session.shell.settled(Duration::from_secs(5)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while old_link.lock().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            host.poll(&mut ui);
            std::thread::sleep(Duration::from_millis(5));
        }
        for intent in [AuthIntent::EmailChanged { text: crate::auth_test_support::EMAIL.into() }, AuthIntent::PasswordChanged { text: "correct".into() }, AuthIntent::Submitted] {
            host.apply(Action::Intent(AppIntent::Auth(intent)), &mut ui);
        }
        host.session.shell.wait(Duration::from_secs(5), |view| matches!(view, AppVm::SignedIn(_)).then_some(())).unwrap();
        let stored = vault.get("transgate.session").unwrap();
        let mut callback = TcpStream::connect(origin.trim_start_matches("http://")).unwrap();
        callback.write_all(format!("GET /home?code={} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", crate::auth_test_support::GOOD_CODE).as_bytes()).unwrap();
        *old_link.lock().unwrap() = Some("stale browser URL".into());
        for _ in 0..20 { host.poll(&mut ui); std::thread::sleep(Duration::from_millis(10)); }
        assert!(host.google.is_none());
        assert!(host.pending_google.is_none());
        assert!(ui.link.is_none());
        assert!(matches!(host.view().unwrap(), AppVm::SignedIn(_)));
        assert_eq!(vault.get("transgate.session").as_deref(), Some(stored.as_str()));
        assert!(!service.state.lock().unwrap().requests.iter().any(|request| request == "auth:pkce"));
    }

    #[test]
    fn google_waits_for_restoration_without_blocking_and_opens_once() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            host: "https://unused.example".into(),
            auth_key: None,
            locale: "en".into(),
            out: dir.path().into(),
        };
        let host_config = HostConfig::from_origin(&config.host, None);
        let vault = Arc::new(Vault::file(dir.path().join("session.json")).for_host(&host_config));
        let account = fixtures::account().0;
        vault.set("transgate.session", &format!(r#"{{"access_token":"old","refresh_token":"old-ref","expires_at":1,"user_id":"{account}","email":"a@example.test"}}"#));
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let lock = vault.refresh_lock().unwrap();
        let held = runtime.block_on(lock.acquire()).unwrap();
        let loopback = Loopback::bind().unwrap();
        let origin = loopback.origin().unwrap();
        let opened = Arc::new(Mutex::new(Vec::new()));
        let urls = opened.clone();
        let session = open_session(
            &config,
            &host_config,
            &vault,
            "/home",
            &origin,
            Box::new(move |url| urls.lock().unwrap().push(url.to_string())),
        )
        .unwrap();
        let mut host = Host {
            config,
            host: host_config,
            vault: vault.clone(),
            session,
            google: None,
            pending_google: Some((loopback, origin)),
            link: Arc::default(),
            saved_seen: 0,
        };
        let mut ui = UiState::default();
        assert!(matches!(host.view().unwrap(), AppVm::Restoring { .. }));
        let started = std::time::Instant::now();
        host.poll(&mut ui);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(opened.lock().unwrap().is_empty());
        assert!(host.google.is_none());
        vault.remove("transgate.session");
        drop(held);
        assert!(matches!(
            host.session.shell.settled(Duration::from_secs(5)),
            Some(AppVm::SignedOut(_))
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while opened.lock().unwrap().is_empty() {
            assert!(std::time::Instant::now() < deadline);
            host.poll(&mut ui);
            std::thread::sleep(Duration::from_millis(10));
        }
        for _ in 0..5 {
            host.poll(&mut ui);
        }
        assert_eq!(opened.lock().unwrap().len(), 1);
        assert!(opened.lock().unwrap()[0].contains("/authorize?"));
        assert!(host.pending_google.is_none());
        assert!(host.google.is_some());
    }

    #[test]
    fn a_listening_port_is_reachable_and_a_closed_one_is_not() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(reachable(&format!("http://127.0.0.1:{port}"), Duration::from_secs(2)));
        drop(listener);
        assert!(!reachable(&format!("http://127.0.0.1:{port}/api"), Duration::from_millis(500)));
    }

    #[test]
    fn only_reachability_failures_exit_four_and_other_sign_in_errors_exit_three() {
        for kind in [AuthFailure::Network, AuthFailure::Unavailable] {
            assert_eq!(auth_exit(&kind.message()), Exit::Network);
        }
        assert_eq!(auth_exit(&AuthFailure::InvalidCredentials.message()), Exit::Auth);
    }

    #[test]
    fn the_exit_code_follows_the_last_view() {
        let views = fixtures::app_fixtures();
        let named = |n: &str| views.iter().find(|(name, _)| *name == n).unwrap().1.clone();
        assert_eq!(exit_for(&named("desktop_new")), Exit::Ok);
        assert_eq!(exit_for(&named("erased")), Exit::Refused);
        let out = fixtures::auth_fixtures().into_iter().find(|(n, _)| *n == "error").unwrap().1;
        let AppVm::SignedOut(mut signed_out) = named("signed_out") else { panic!("signed out") };
        signed_out.auth = out;
        assert_eq!(exit_for(&AppVm::SignedOut(signed_out)), Exit::Auth);
    }
}
