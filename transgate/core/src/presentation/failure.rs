//! How a request the presenter asked for failed, as the client reports it.
//! Every result event carries `Result<T, ApiFailure>`; the app reducer alone
//! turns 401 into one refresh attempt and 410 `account_deletion_in_progress`
//! into the terminal erased state, so a screen never decides session rules.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiFailure {
    /// `None`: no HTTP answer (offline, DNS, aborted, timed out).
    pub status: Option<u16>,
    /// The host's machine-readable `code` (`transgate-api` error body), when present.
    pub code: Option<String>,
}

/// The host's 410 code for an account whose erasure started (P3 D13).
pub const ACCOUNT_ERASED: &str = "account_deletion_in_progress";

impl ApiFailure {
    pub fn network() -> ApiFailure {
        ApiFailure { status: None, code: None }
    }

    pub fn http(status: u16, code: Option<&str>) -> ApiFailure {
        ApiFailure { status: Some(status), code: code.map(str::to_string) }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status == Some(401)
    }

    pub fn is_account_erased(&self) -> bool {
        self.status == Some(410) && self.code.as_deref() == Some(ACCOUNT_ERASED)
    }

    pub fn is_network(&self) -> bool {
        self.status.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_erasure_code_on_410_is_the_erased_account() {
        assert!(ApiFailure::http(410, Some(ACCOUNT_ERASED)).is_account_erased());
        assert!(!ApiFailure::http(410, Some("gone")).is_account_erased());
        assert!(!ApiFailure::http(401, Some(ACCOUNT_ERASED)).is_account_erased());
        assert!(ApiFailure::http(401, None).is_unauthorized());
        assert!(ApiFailure::network().is_network() && !ApiFailure::network().is_unauthorized());
    }
}
