//! Real provider transports. Product routing, authorization, retry scheduling and erasure
//! completion decisions belong to their cores/hosts; this crate reports wire evidence only.
pub mod http;
pub mod transcription;
pub mod storage;
pub mod fetch;
pub mod captions;
pub mod callback;

/// The loopback test server shared by the unit tests (once per crate).
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;

pub use http::HttpConfig;
pub use transcription::{Soniox, SonioxConfig, Deepgram, DeepgramConfig};
pub use storage::{S3Store, S3StoreConfig, LegacySupabaseStorage, LegacySupabaseConfig, LegacyEntry};
pub use fetch::{ControlledFetcher, FetchConfig, is_public_address};
pub use captions::{TranscriptApi, TranscriptApiConfig, PublicYouTube, PublicYouTubeConfig, YouTubeMetadata, YouTubeMetadataConfig};
pub use callback::{CallbackConfig, CallbackPoster, PostOutcome};
