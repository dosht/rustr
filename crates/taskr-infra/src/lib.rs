//! Infrastructure adapters. `snapshot_json` is target independent and is reused
//! by the web build; everything behind `native` needs a real filesystem.

pub mod snapshot_json;

#[cfg(feature = "native")]
pub mod clock;
#[cfg(feature = "native")]
pub mod json_file;
#[cfg(feature = "native")]
pub mod paths;
#[cfg(feature = "native")]
pub mod session_file;
#[cfg(feature = "native")]
pub mod telemetry;
