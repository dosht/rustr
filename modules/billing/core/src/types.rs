//! Identifiers, instants and the boxed future used by every port.
//!
//! The billing module owns its `BoxFuture` alias on purpose: importing the one
//! from `taskr-core` would be a dependency edge from a module to a host.

use std::fmt;
use std::pin::Pin;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Stable, UUID-shaped account identity. Hosts map their own subjects onto it
/// (Transgate users are UUIDs already; taskr dev subjects are hashed into one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountId(pub Uuid);

impl AccountId {
    pub fn parse(text: &str) -> Option<AccountId> {
        Uuid::parse_str(text).ok().map(AccountId)
    }

    /// A real UUID subject is kept as it is. Any other subject (taskr's
    /// `dev-<email>` tokens, for example) maps deterministically into the
    /// given namespace, so a retry reaches the same account.
    pub fn from_subject(subject: &str, namespace: &Uuid) -> AccountId {
        match Uuid::parse_str(subject) {
            Ok(id) => AccountId(id),
            Err(_) => AccountId(Uuid::new_v5(namespace, subject.as_bytes())),
        }
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl FromStr for AccountId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        AccountId::parse(s).ok_or(())
    }
}

/// Numeric ids are kept from the legacy tables, so imports retain them.
pub type PackageId = i64;
pub type PaymentId = i64;
/// The host's job (or sample work) id; billing never reads the host's tables.
pub type SubjectRef = i64;

/// A UTC instant in microseconds since the Unix epoch, the precision of
/// `timestamptz`. Milliseconds would merge instants that differ at an
/// inclusive window end or an allocation tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "number"))]
pub struct Micros(pub i64);

impl Micros {
    pub const PER_SECOND: i64 = 1_000_000;

    pub fn from_secs(secs: i64) -> Micros {
        Micros(secs * Self::PER_SECOND)
    }

    pub fn plus_secs(self, secs: i64) -> Micros {
        Micros(self.0 + secs * Self::PER_SECOND)
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Micros;
}

/// A clock that always answers the same instant, for tests and replays.
pub struct FixedClock(pub Micros);

impl Clock for FixedClock {
    fn now(&self) -> Micros {
        self.0
    }
}
