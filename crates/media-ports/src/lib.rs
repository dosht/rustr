//! Shared wire contracts. No routing, billing, normalization, retry or retention policy.
use std::{fmt, future::Future, pin::Pin};
use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send + 'static>>;
pub type Headers = Vec<(String, String)>;
pub type Extra = Map<String, Value>;

/// Explicit capability access only; Debug never reveals credentials or signed URLs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);
impl SecretString {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
    pub fn expose(&self) -> &str { &self.0 }
}
impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("[REDACTED]") }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SensitiveUrl(String);
impl SensitiveUrl {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
    pub fn expose(&self) -> &str { &self.0 }
}
impl fmt::Debug for SensitiveUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("[REDACTED URL]") }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Micros(pub i64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider { Soniox, Deepgram }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind { Gcs, R2, Supabase }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    pub store: StoreKind,
    pub bucket: String,
    pub key: String,
    /// GCS generation or S3 version id; never an ETag masquerading as a version.
    pub version: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessMethod { Get, Put }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryAccess {
    pub object: ObjectRef,
    pub method: AccessMethod,
    pub url: SensitiveUrl,
    pub expires_at: Micros,
    pub headers: Headers,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMetadata {
    pub object: ObjectRef,
    pub size: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteOutcome {
    Deleted,
    Absent,
    Retained { until: Option<Micros>, reason: String },
    Unsupported { reason: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportError {
    Configuration(String),
    Rejected { status: Option<u16>, code: String },
    Unavailable { status: Option<u16>, operation: String },
    Malformed(String),
    Unsupported(String),
    UnsafeDestination,
    LimitExceeded(String),
}
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for TransportError {}

pub struct PutObject {
    pub object: ObjectRef,
    pub body: ByteStream,
    pub content_length: u64,
    pub content_type: String,
}
pub trait ObjectStore: Send + Sync {
    fn kind(&self) -> StoreKind;
    fn bucket(&self) -> &str;
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: Micros, ttl_seconds: u32)
        -> BoxFuture<'a, Result<TemporaryAccess, TransportError>>;
    fn head<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>>;
    fn put(&self, request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>>;
    fn delete<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>>;
}
/// A full, bounded, paginated traversal of one owner prefix. A store whose objects may
/// exist without a row elsewhere (the legacy Supabase `jobs` bucket) offers this so erasure
/// can find them; stores that only hold recorded objects do not implement it.
pub trait ObjectInventory: Send + Sync {
    fn kind(&self) -> StoreKind;
    fn bucket(&self) -> &str;
    /// Objects at any depth under `{owner}/`, where `owner` is one key segment. A failed,
    /// unauthorized, too deep or too large traversal, and every detected listing anomaly, is
    /// an error rather than a partial list. This is not an atomic snapshot: offset pages can
    /// miss objects moved by concurrent writes or deletes. An empty answer may close work
    /// only after the relevant writers are fenced or drained; a later traversal repairs
    /// names skipped because of deletions, never objects written after it.
    fn inventory<'a>(&'a self, owner: &'a str) -> BoxFuture<'a, Result<Vec<ObjectRef>, TransportError>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackSpec {
    /// Host supplies job_id and attempt query parameters before submission.
    pub url: SensitiveUrl,
    /// Soniox uses X-Webhook-Secret; Deepgram uses Basic username=attempt UUID, password=secret.
    pub secret: SecretString,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionRequest {
    pub attempt: Uuid,
    pub provider: Provider,
    pub project: String,
    /// Server-selected. New Soniox submissions use explicit stt-async-v5 (D11).
    pub model: String,
    pub language: Option<String>,
    pub translation_target: Option<String>,
    pub diarize: bool,
    pub audio: TemporaryAccess,
    /// Preserve the product's job_N reference; attempt identity is separate.
    pub client_reference: String,
    pub callback: CallbackSpec,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIdentity {
    pub provider: Provider,
    pub project: String,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Submission {
    Accepted { remote: RemoteIdentity },
    Rejected { status: Option<u16>, code: String },
    /// Includes lost/malformed answers after POST: nonacceptance is not proved.
    Uncertain { status: Option<u16>, operation: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationOrigin { Webhook, RecoveryPoll }

/// Lossless provider wire payloads. Unknown fields survive durable intake too.
/// Provider timestamps stay in their original units; only product core normalizes them.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SonioxToken {
    pub text: String,
    #[serde(default)] pub start_ms: Option<f64>,
    #[serde(default)] pub end_ms: Option<f64>,
    #[serde(default)] pub speaker: Option<Value>,
    #[serde(default)] pub language: Option<String>,
    #[serde(default)] pub translation_status: Option<String>,
    #[serde(default)] pub is_audio_event: Option<bool>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SonioxTranscript {
    pub id: String,
    pub tokens: Vec<SonioxToken>,
    #[serde(default)] pub text: Option<String>,
    #[serde(default)] pub metadata: Value,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SonioxReported {
    pub id: String,
    pub status: String,
    #[serde(default)] pub audio_duration_ms: Option<f64>,
    #[serde(default)] pub error: Option<Value>,
    #[serde(default)] pub error_type: Option<String>,
    #[serde(default)] pub error_message: Option<String>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)] pub punctuated_word: Option<String>,
    #[serde(default)] pub speaker: Option<i64>,
    #[serde(default)] pub confidence: Option<f64>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramSentence {
    pub text: String,
    pub start: f64,
    pub end: f64,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramParagraph {
    pub sentences: Vec<DeepgramSentence>,
    #[serde(default)] pub speaker: Option<i64>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramParagraphs {
    pub paragraphs: Vec<DeepgramParagraph>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramAlternative {
    #[serde(default)] pub transcript: String,
    #[serde(default)] pub words: Vec<DeepgramWord>,
    #[serde(default)] pub paragraphs: Option<DeepgramParagraphs>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramChannel {
    pub alternatives: Vec<DeepgramAlternative>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramResults {
    pub channels: Vec<DeepgramChannel>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramMetadata {
    pub request_id: String,
    #[serde(default)] pub duration: Option<f64>,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepgramTranscript {
    pub metadata: DeepgramMetadata,
    pub results: DeepgramResults,
    #[serde(flatten)] pub extra: Extra,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProviderPayload {
    Soniox { reported: SonioxReported, transcript: Option<SonioxTranscript> },
    Deepgram(DeepgramTranscript),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderObservation {
    pub remote: RemoteIdentity,
    pub origin: ObservationOrigin,
    pub payload: ProviderPayload,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRecord {
    pub remote: RemoteIdentity,
    pub status: String,
    pub model: String,
    pub client_reference: Option<String>,
    pub audio_url: Option<SensitiveUrl>,
    pub callback_url: Option<SensitiveUrl>,
    pub created_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackBinding {
    pub attempt: Uuid,
    pub provider: Provider,
    pub project: String,
    /// None while submit acknowledgement is uncommitted/uncertain; core owns remote-id adoption.
    pub expected_remote_id: Option<String>,
    pub secret: SecretString,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallbackError { Unauthorized, IdentityMismatch, Malformed }

pub trait Transcriber: Send + Sync {
    fn provider(&self) -> Provider;
    fn project(&self) -> &str;
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>>;
    /// Status-only observation. Deepgram has no transcript/status recovery API: Unsupported.
    fn status<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>>;
    fn result<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>>;
    fn delete_remote<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>>;
    /// All available matching records, not an acceptance/nonacceptance decision.
    fn find_by_reference<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>>;
    /// Host selects the stored binding by query attempt, before this call. No I/O.
    fn verify_callback(&self, binding: &CallbackBinding, headers: &Headers, body: &[u8])
        -> Result<ProviderObservation, CallbackError>;
    /// Fetch Soniox tokens while preserving reported webhook duration verbatim (D6).
    /// Deepgram's complete callback body already contains the result.
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionCue { pub text: String, pub start: f64, pub duration: f64 }
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionDocument {
    pub video_id: String,
    pub language: Option<String>,
    /// Seconds for both protocols; no product line segmentation here.
    pub cues: Vec<CaptionCue>,
    pub metadata: Value,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptionError {
    InvalidVideo, NoCaptions, LanguageUnavailable, VideoUnavailable,
    AgeRestricted, RateLimited, Network, Unknown,
}
pub trait PaidCaptions: Send + Sync {
    fn fetch<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<CaptionDocument, CaptionError>>;
}
pub trait PublicCaptions: Send + Sync {
    fn fetch<'a>(&'a self, video_id: &'a str, language: Option<&'a str>) -> BoxFuture<'a, Result<CaptionDocument, CaptionError>>;
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub video_id: String,
    pub title: String,
    pub duration_seconds: Option<f64>,
    pub language: Option<String>,
    pub raw: Value,
}
pub trait VideoMetadataSource: Send + Sync {
    fn fetch<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<VideoMetadata, CaptionError>>;
}

pub trait ExternalStager: Send + Sync {
    /// Host records target in the erasure manifest before invoking this operation.
    /// Every DNS result and redirect is checked, socket addresses pinned, proxies disabled.
    fn stage<'a>(&'a self, source: &'a SensitiveUrl, target: &'a ObjectRef, store: &'a dyn ObjectStore)
        -> BoxFuture<'a, Result<ObjectMetadata, TransportError>>;
}

// Full provider metadata can contain signed source/callback URLs and customer text.
// It remains serializable for durable intake, but is never printable via Debug.
macro_rules! private_payload_debug {
    ($($name:ty),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED PAYLOAD])"))
            }
        }
    )+};
}
private_payload_debug!(
    SonioxToken, SonioxTranscript, SonioxReported, DeepgramWord, DeepgramSentence,
    DeepgramParagraph, DeepgramParagraphs, DeepgramAlternative, DeepgramChannel,
    DeepgramResults, DeepgramMetadata, DeepgramTranscript, CaptionDocument, VideoMetadata,
);
