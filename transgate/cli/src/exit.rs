//! Exit codes (P8 D13): 0 success, 1 a domain refusal, 2 usage, 3 authentication,
//! 4 the host could not be reached. Messages go to stderr; results go to stdout.
use std::process::ExitCode;

use transgate_core::presentation::app::AuthFailure;
use transgate_core::presentation::msg::Msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Refused = 1,
    Usage = 2,
    Auth = 3,
    Network = 4,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> ExitCode {
        ExitCode::from(exit as u8)
    }
}

#[derive(Debug)]
pub struct Failure {
    pub exit: Exit,
    pub message: String,
}

impl Failure {
    pub fn new(exit: Exit, message: impl Into<String>) -> Failure {
        Failure { exit, message: message.into() }
    }

    pub fn usage(message: impl Into<String>) -> Failure {
        Failure::new(Exit::Usage, message)
    }
}

pub type Outcome<T = ()> = Result<T, Failure>;

/// A sign-in error Msg is the shell's only handle on its kind: network and an
/// unavailable identity service exit 4, everything else the person can fix exits 3.
pub fn auth_exit(error: &Msg) -> Exit {
    if *error == AuthFailure::Network.message() || *error == AuthFailure::Unavailable.message() { Exit::Network } else { Exit::Auth }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reachability_failures_exit_four() {
        for kind in [AuthFailure::Network, AuthFailure::Unavailable] {
            assert_eq!(auth_exit(&kind.message()), Exit::Network);
        }
        for kind in [AuthFailure::InvalidCredentials, AuthFailure::EmailNotConfirmed, AuthFailure::RateLimited, AuthFailure::CaptchaFailed] {
            assert_eq!(auth_exit(&kind.message()), Exit::Auth);
        }
    }
}
