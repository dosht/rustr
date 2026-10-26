//! Identifiers and instants. Job and line ids stay the source's signed database
//! ids; attempt, erasure and claim identities are independent UUIDs. Instants are
//! UTC microseconds (billing's `Micros`), provider durations are milliseconds.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use billing_core::types::{AccountId, Micros};

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "number"))]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> $name {
                $name(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0.hyphenated())
            }
        }
    };
}

numeric_id!(JobId);
numeric_id!(LineId);
numeric_id!(InboxId);
uuid_id!(AttemptId);
uuid_id!(ErasureId);

impl JobId {
    /// Billing's subject reference is the job id itself.
    pub fn subject(self) -> billing_core::types::SubjectRef {
        self.0
    }
}

pub const MICROS_PER_SECOND: i64 = 1_000_000;
pub const MICROS_PER_MINUTE: i64 = 60 * MICROS_PER_SECOND;
pub const MICROS_PER_DAY: i64 = 24 * 60 * MICROS_PER_MINUTE;

pub fn to_media(at: Micros) -> media_ports::Micros {
    media_ports::Micros(at.0)
}

pub fn from_media(at: media_ports::Micros) -> Micros {
    Micros(at.0)
}
