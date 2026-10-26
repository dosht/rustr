//! Concrete completion HTTP and portable application-stream codecs.
pub mod sse;
pub mod application;
pub use application::{ApplicationDecoder, decode_application_json, encode_application_json, encode_frame};
pub use sse::{CodecError, SseDecoder, SseEvent};

#[cfg(not(target_arch = "wasm32"))]
pub mod provider;
#[cfg(not(target_arch = "wasm32"))]
pub mod client;
#[cfg(not(target_arch = "wasm32"))]
pub use provider::{DispatchCapacity, HttpLlm, ProviderConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use client::{ApplicationClient, ApplicationStream, ClientError};
