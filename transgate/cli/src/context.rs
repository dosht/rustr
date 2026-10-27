//! What every command shares: the host, the locale, the vault, and how a command
//! starts the presenter and waits for a settled signed-in view. All copy goes
//! through `transgate_i18n::translate`; a missing key prints its name (a bug, not
//! a fallback language).
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use transgate_core::presentation::app::{AppVm, SESSION_UNAVAILABLE, SignedInVm};
use transgate_core::presentation::msg::{Msg, common};
use transgate_native::{HostConfig, OpenUrl, Shell, Signal, TerminalPorts, Vault};

use crate::exit::{Exit, Failure, Outcome, auth_exit};

const USER_AGENT: &str = concat!("transgate-cli/", env!("CARGO_PKG_VERSION"));
/// The origin auth redirects return to when no loopback listener is involved (never contacted).
const IDLE_SITE: &str = "http://127.0.0.1";

pub struct Context {
    pub locale: String,
    pub host: HostConfig,
    pub vault: Arc<Vault>,
    pub timeout: Duration,
    pub out_dir: PathBuf,
}

pub struct Session {
    pub shell: Shell,
    pub ports: Arc<TerminalPorts>,
}

impl Context {
    pub fn say(&self, msg: &Msg) -> String {
        transgate_i18n::translate(&self.locale, msg).unwrap_or_else(|_| format!("{}.{}", msg.ns.file(), msg.key))
    }

    /// `/home` in the chosen locale: the route core starts on.
    pub fn home_url(&self) -> String {
        if self.locale == "en" { "/home".into() } else { format!("/{}/home", self.locale) }
    }

    pub fn unavailable(&self) -> Failure {
        Failure::new(Exit::Network, self.say(&SESSION_UNAVAILABLE))
    }

    fn not_signed_in(&self) -> Failure {
        Failure::new(Exit::Auth, self.say(&common("Please sign in and try again.")))
    }

    /// Starts the presenter at `url`. `open` receives URLs core asks the user to visit.
    pub fn start(&self, url: &str, site_url: &str, open: OpenUrl) -> Outcome<Session> {
        let signal = Arc::new(Signal::default());
        let ports = Arc::new(TerminalPorts::new(self.vault.clone(), self.out_dir.clone(), open, signal.clone()));
        let config = self.host.presenter(site_url, USER_AGENT);
        let shell = Shell::start(config, ports.clone(), signal, url).map_err(|e| Failure::new(Exit::Refused, e.to_string()))?;
        Ok(Session { shell, ports })
    }

    /// A presenter at `/home` with nothing to open.
    pub fn start_home(&self) -> Outcome<Session> {
        self.start(&self.home_url(), IDLE_SITE, Box::new(|_| {}))
    }

    /// The first view that is not the session spinner.
    pub fn settled(&self, session: &Session) -> Outcome<AppVm> {
        session.shell.settled(self.timeout).ok_or_else(|| self.unavailable())
    }

    /// A signed-in presenter whose profile has loaded; otherwise the failure the exit code names.
    pub fn signed_in(&self) -> Outcome<(Session, Box<SignedInVm>)> {
        let session = self.start_home()?;
        let view = self.settled(&session)?;
        match view {
            AppVm::SignedIn(_) => {}
            AppVm::SignedOut(vm) => {
                return Err(match &vm.auth.error {
                    Some(error) => Failure::new(auth_exit(error), self.say(error)),
                    None => self.not_signed_in(),
                });
            }
            AppVm::Erased { message, .. } => return Err(Failure::new(Exit::Refused, self.say(&message))),
            AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } => return Err(self.unavailable()),
        }
        let vm = session
            .shell
            .wait(self.timeout, |view| match view {
                AppVm::SignedIn(vm) if vm.profile.is_some() || vm.session_error.is_some() => Some(vm.clone()),
                _ => None,
            })
            .ok_or_else(|| self.unavailable())?;
        if let Some(error) = &vm.session_error {
            return Err(Failure::new(Exit::Network, self.say(error)));
        }
        Ok((session, vm))
    }
}
