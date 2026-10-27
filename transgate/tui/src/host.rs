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
    target.to_socket_addrs().ok().is_some_and(|mut addrs| addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok()))
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
}

pub struct Host {
    config: Config,
    host: HostConfig,
    vault: Arc<Vault>,
    session: Session,
    google: Option<GoogleWait>,
    link: Arc<Mutex<Option<String>>>,
    saved_seen: usize,
}

impl Host {
    pub fn start(config: Config) -> Result<Host, String> {
        let host = HostConfig::from_origin(&config.host, config.auth_key.clone());
        let vault = Arc::new(Vault::from_env());
        let session = open_session(&config, &host, &vault, &home_url(&config.locale), IDLE_SITE, Box::new(|_| {}))?;
        Ok(Host { config, host, vault, session, google: None, link: Arc::default(), saved_seen: 0 })
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

    fn dispatch(&mut self, intent: AppIntent, ui: &mut UiState) {
        if let Err(error) = self.session.shell.dispatch(intent) {
            ui.notice = Some(error.to_string());
        }
    }

    fn pick(&mut self, path: PathBuf, ui: &mut UiState) {
        match self.session.ports.pick(&path) {
            Ok(file) => self.dispatch(AppIntent::Upload(UploadIntent::Picked { files: vec![file] }), ui),
            Err(error) => ui.notice = Some(format!("{}: {error}", path.display())),
        }
    }

    /// Core builds the authorization URL from the site URL, so the presenter that asks
    /// for it must be started with the loopback origin; the browser returns there with
    /// a PKCE code, and a second presenter started at that target exchanges it.
    fn google(&mut self, ui: &mut UiState) {
        if self.google.is_some() {
            return;
        }
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
        self.dispatch(AppIntent::Auth(AuthIntent::GoogleChosen), ui);
        let page = crate::view::say(&self.config.locale, &auth("sign_in.loading_button_label"));
        let (send, callback) = channel();
        std::thread::spawn(move || {
            let target = loopback.wait_callback(BROWSER_WAIT, &page).ok().flatten();
            let _ = send.send(target);
        });
        self.google = Some(GoogleWait { origin, callback });
    }

    /// Once per loop turn: the loopback answer, and the host facts worth a notice.
    pub fn poll(&mut self, ui: &mut UiState) {
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
                    self.google = None;
                    ui.link = None;
                }
            }
        }
        let saved = self.session.ports.saved();
        if saved.len() > self.saved_seen {
            self.saved_seen = saved.len();
            ui.notice = saved.last().map(|p| p.display().to_string());
        }
        if let Some(error) = self.session.ports.take_save_error().or_else(|| self.vault.take_error()) {
            ui.notice = Some(error);
        }
    }

    fn finish_google(&mut self, target: Option<String>, origin: &str, ui: &mut UiState) {
        ui.link = None;
        *self.link.lock().expect("the link lock is never poisoned") = None;
        let Some(target) = target else { return };
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
