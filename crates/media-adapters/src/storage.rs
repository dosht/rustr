use std::collections::HashSet;
use std::time::Duration;
use jiff::Timestamp;
use media_ports::*;
use reqwest::{Client, Method, Response};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use crate::http::{self, HttpConfig};

#[derive(Clone, Debug)]
pub struct S3StoreConfig {
    /// Gcs uses HMAC XML interoperability; R2 uses its native S3 endpoint.
    pub kind: StoreKind,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: SecretString,
    pub secret_key: SecretString,
    pub http: HttpConfig,
}
pub struct S3Store { client: Client, bucket: Bucket, credentials: Credentials, kind: StoreKind, http: HttpConfig }

fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.starts_with('/') && !key.contains('\0')
        && !key.split('/').any(|part| part == "." || part == ".." || part.is_empty())
}
fn bound(object: &ObjectRef, kind: StoreKind, bucket: &str) -> Result<(), TransportError> {
    if object.store != kind || object.bucket != bucket || !valid_key(&object.key)
        || object.version.as_deref().is_some_and(str::is_empty) {
        Err(TransportError::Rejected { status:None, code:"object_binding".into() })
    } else { Ok(()) }
}
fn timestamp(now: Micros) -> Result<Timestamp, TransportError> {
    Timestamp::from_microsecond(now.0).map_err(|_| TransportError::Configuration("signing instant".into()))
}
impl S3Store {
    pub fn new(config: S3StoreConfig) -> Result<Self, TransportError> {
        if !matches!(config.kind, StoreKind::Gcs | StoreKind::R2) || config.bucket.is_empty() || config.region.is_empty()
            || config.access_key.expose().is_empty() || config.secret_key.expose().is_empty() {
            return Err(TransportError::Configuration("GCS/R2 bucket, region and HMAC credentials required".into()));
        }
        let endpoint = http::configured_url(&config.endpoint)?;
        let bucket = Bucket::new(endpoint, UrlStyle::Path, config.bucket, config.region)
            .map_err(|_| TransportError::Configuration("storage bucket".into()))?;
        Ok(Self { client:http::client(&config.http)?, bucket,
            credentials:Credentials::new(config.access_key.expose().to_owned(), config.secret_key.expose().to_owned()),
            kind:config.kind, http:config.http })
    }
    fn sign_action<'a, A: S3Action<'a>>(&self, mut action: A, object: &'a ObjectRef, now: Timestamp, ttl: u32) -> Url {
        if let Some(version) = &object.version {
            action.query_mut().insert(if self.kind == StoreKind::Gcs { "generation" } else { "versionId" }, version.as_str());
        }
        action.sign_with_time(Duration::from_secs(ttl.into()), &now)
    }
    fn url(&self, object: &ObjectRef, method: Method, now: Timestamp, ttl: u32) -> Result<Url, TransportError> {
        bound(object, self.kind, self.bucket.name())?;
        let credentials = Some(&self.credentials);
        Ok(match method {
            Method::GET => self.sign_action(self.bucket.get_object(credentials, &object.key), object, now, ttl),
            Method::HEAD => self.sign_action(self.bucket.head_object(credentials, &object.key), object, now, ttl),
            Method::DELETE => self.sign_action(self.bucket.delete_object(credentials, &object.key), object, now, ttl),
            Method::PUT if object.version.is_none() => self.sign_action(self.bucket.put_object(credentials, &object.key), object, now, ttl),
            _ => return Err(TransportError::Rejected { status:None, code:"immutable_version_write".into() }),
        })
    }
    fn metadata(&self, response: &Response, object: &ObjectRef, size: u64, content_type: Option<String>) -> ObjectMetadata {
        let header = |name: &str| response.headers().get(name).and_then(|h| h.to_str().ok()).map(str::to_owned);
        let version = header(if self.kind == StoreKind::Gcs { "x-goog-generation" } else { "x-amz-version-id" });
        let mut object = object.clone();
        if let Some(version) = version { object.version = Some(version); }
        ObjectMetadata { object, size, content_type, etag:header("etag") }
    }
}
impl ObjectStore for S3Store {
    fn kind(&self) -> StoreKind { self.kind }
    fn bucket(&self) -> &str { self.bucket.name() }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: Micros, ttl_seconds: u32) -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async move {
            if ttl_seconds == 0 || ttl_seconds > 604800 {
                return Err(TransportError::Rejected { status:None, code:"signed_url_lifetime".into() });
            }
            let expires_at = now.0.checked_add(i64::from(ttl_seconds) * 1_000_000)
                .ok_or_else(|| TransportError::Configuration("signed_url_expiry".into()))?;
            let url = self.url(object, match method { AccessMethod::Get => Method::GET, AccessMethod::Put => Method::PUT }, timestamp(now)?, ttl_seconds)?;
            // No content-type pin and no resumable-upload header: ordinary single PUT.
            Ok(TemporaryAccess { object:object.clone(), method, url:SensitiveUrl::new(url.to_string()), expires_at:Micros(expires_at), headers:vec![] })
        })
    }
    fn head<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async move {
            let url = self.url(object, Method::HEAD, Timestamp::now(), 60)?;
            let response = self.client.head(url).send().await.map_err(|_| http::unavailable("storage head", None))?;
            if response.status().as_u16() == 404 { return Ok(None); }
            if !response.status().is_success() { return Err(http::unavailable("storage head", Some(response.status().as_u16()))); }
            let size = response.headers().get("content-length").and_then(|h| h.to_str().ok()).and_then(|v| v.parse().ok())
                .ok_or_else(|| TransportError::Malformed("storage content-length".into()))?;
            let content_type = response.headers().get("content-type").and_then(|h| h.to_str().ok()).map(str::to_owned);
            Ok(Some(self.metadata(&response, object, size, content_type)))
        })
    }
    fn put(&self, request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        Box::pin(async move {
            let url = self.url(&request.object, Method::PUT, Timestamp::now(), 3600)?;
            let response = self.client.put(url).header("content-length", request.content_length)
                .header("content-type", &request.content_type).body(reqwest::Body::wrap_stream(request.body))
                .send().await.map_err(|_| http::unavailable("storage put", None))?;
            if !response.status().is_success() { return Err(http::unavailable("storage put", Some(response.status().as_u16()))); }
            Ok(self.metadata(&response, &request.object, request.content_length, Some(request.content_type)))
        })
    }
    fn delete<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            let url = self.url(object, Method::DELETE, Timestamp::now(), 60)?;
            let response = self.client.delete(url).send().await.map_err(|_| http::unavailable("storage delete", None))?;
            let status = response.status().as_u16();
            if matches!(status, 200 | 204) { return Ok(DeleteOutcome::Deleted); }
            if status == 404 { return Ok(DeleteOutcome::Absent); }
            let body = http::bytes(response, self.http.max_response_bytes, "storage delete").await?;
            #[derive(Deserialize)]
            struct StorageError { #[serde(rename="Code")] code:String, #[serde(rename="Message", default)] message:String }
            let error: Result<StorageError, _> = quick_xml::de::from_reader(body.as_slice());
            if let Ok(error) = error {
                let retained = matches!(error.code.as_str(), "ObjectUnderActiveHold" | "ObjectUnderRetention" | "RetentionPolicyNotMet")
                    || (error.code == "AccessDenied" && (error.message.to_lowercase().contains("retention") || error.message.to_lowercase().contains("hold")));
                if retained { return Ok(DeleteOutcome::Retained { until:None, reason:"provider retention or hold".into() }); }
            }
            Err(http::unavailable("storage delete", Some(status)))
        })
    }
}

#[derive(Clone, Debug)]
pub struct LegacySupabaseConfig {
    /// Project origin, without /storage/v1. Authority restricted to the jobs bucket.
    pub base_url: String,
    pub service_key: SecretString,
    pub http: HttpConfig,
}
pub struct LegacySupabaseStorage { client:Client, base:Url, config:LegacySupabaseConfig }
#[derive(Clone, Debug, Deserialize)]
pub struct LegacyEntry {
    pub name:String,
    /// None means a directory, not an object. Caller recurses using its account prefix.
    pub id:Option<String>,
    pub metadata:Option<Value>,
}
impl LegacySupabaseStorage {
    pub fn new(config: LegacySupabaseConfig) -> Result<Self, TransportError> {
        if config.service_key.expose().is_empty() { return Err(TransportError::Configuration("legacy Supabase service key required".into())); }
        Ok(Self { client:http::client(&config.http)?, base:http::configured_url(&config.base_url)?, config })
    }
    fn request(&self, method:Method, url:Url) -> reqwest::RequestBuilder {
        self.client.request(method, url).bearer_auth(self.config.service_key.expose()).header("apikey", self.config.service_key.expose())
    }
    fn object_url(&self, object:&ObjectRef, operation:&str) -> Result<Url, TransportError> {
        bound(object, StoreKind::Supabase, "jobs")?;
        if object.version.is_some() { return Err(TransportError::Unsupported("Supabase object versions".into())); }
        let mut parts = vec!["storage", "v1", "object"];
        if !operation.is_empty() { parts.push(operation); }
        parts.push("jobs");
        parts.extend(object.key.split('/'));
        http::endpoint(&self.base, &parts)
    }
    pub async fn list_prefix(&self, prefix:&str, offset:u64, limit:u32) -> Result<Vec<LegacyEntry>, TransportError> {
        if !valid_key(prefix.trim_end_matches('/')) || limit == 0 || limit > 1000 {
            return Err(TransportError::Rejected { status:None, code:"legacy_inventory_scope".into() });
        }
        let url = http::endpoint(&self.base, &["storage", "v1", "object", "list", "jobs"])?;
        let response = self.request(Method::POST, url).json(&json!({"prefix":prefix,"offset":offset,"limit":limit,"sortBy":{"column":"name","order":"asc"}}))
            .send().await.map_err(|_| http::unavailable("legacy list", None))?;
        let entries:Vec<LegacyEntry> = http::json(response, &self.config.http, "legacy list").await?;
        if entries.iter().any(|e| e.name.contains('/') || !valid_key(&e.name)) {
            return Err(TransportError::Malformed("legacy inventory path".into()));
        }
        Ok(entries)
    }
}
impl ObjectStore for LegacySupabaseStorage {
    fn kind(&self) -> StoreKind { StoreKind::Supabase }
    fn bucket(&self) -> &str { "jobs" }
    fn sign<'a>(&'a self, _object:&'a ObjectRef, _method:AccessMethod, _now:Micros, _ttl_seconds:u32) -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("legacy Supabase adapter is erasure-only".into())) })
    }
    fn put(&self, _request:PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("legacy Supabase adapter is erasure-only".into())) })
    }
    fn head<'a>(&'a self, object:&'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async move {
            let url = self.object_url(object, "info")?;
            let response = self.request(Method::GET, url).send().await.map_err(|_| http::unavailable("legacy info", None))?;
            if response.status().as_u16() == 404 { return Ok(None); }
            let value:Value = http::json(response, &self.config.http, "legacy info").await?;
            // InfoRenderer's metadata is user metadata, not the object's storage properties.
            let size = value.get("size").and_then(Value::as_u64).ok_or_else(|| TransportError::Malformed("legacy object size".into()))?;
            Ok(Some(ObjectMetadata { object:object.clone(), size,
                content_type:value.get("content_type").and_then(Value::as_str).map(str::to_owned),
                etag:value.get("etag").and_then(Value::as_str).map(str::to_owned) }))
        })
    }
    fn delete<'a>(&'a self, object:&'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            self.object_url(object, "")?;
            let url = http::endpoint(&self.base, &["storage", "v1", "object", "jobs"])?;
            let response = self.request(Method::DELETE, url).json(&json!({"prefixes":[object.key]})).send().await
                .map_err(|_| http::unavailable("legacy delete", None))?;
            let deleted:Vec<Value> = http::json(response, &self.config.http, "legacy delete").await?;
            if deleted.iter().any(|row| row.get("name").and_then(Value::as_str) == Some(&object.key)) { return Ok(DeleteOutcome::Deleted); }
            // Empty success arrays do not prove deletion; confirm authenticated absence.
            match self.head(object).await? {
                None => Ok(DeleteOutcome::Absent),
                Some(_) => Err(http::unavailable("legacy delete not confirmed", None)),
            }
        })
    }
}

/// One inventory page; within the 1..=1000 `list_prefix` accepts.
const LEGACY_PAGE: u32 = 1000;
/// An owner tree with more entries (objects and folders) than this is refused, not truncated.
/// Every queued folder is an entry, so this also bounds the folders waiting to be listed.
pub const LEGACY_INVENTORY_LIMIT: usize = 100_000;
/// List requests one inventory may make, so a tree of empty folders is bounded too.
pub const LEGACY_INVENTORY_REQUESTS: usize = 20_000;
/// The legacy layout is `{account}/{job}/{file}`; a deeper tree is refused, not guessed.
pub const LEGACY_INVENTORY_DEPTH: usize = 8;

impl ObjectInventory for LegacySupabaseStorage {
    fn kind(&self) -> StoreKind { StoreKind::Supabase }
    fn bucket(&self) -> &str { "jobs" }
    fn inventory<'a>(&'a self, owner:&'a str) -> BoxFuture<'a, Result<Vec<ObjectRef>, TransportError>> {
        Box::pin(async move {
            if owner.contains('/') || !valid_key(owner) {
                return Err(TransportError::Rejected { status:None, code:"legacy_inventory_scope".into() });
            }
            let mut objects = Vec::new();
            let mut entries = 0usize;
            let mut requests = 0usize;
            // Folders still to list, with their depth below the bucket root.
            let mut folders = vec![(owner.to_owned(), 1usize)];
            while let Some((prefix, depth)) = folders.pop() {
                let mut offset = 0u64;
                let mut names = HashSet::new();
                loop {
                    requests += 1;
                    if requests > LEGACY_INVENTORY_REQUESTS { return Err(TransportError::LimitExceeded("legacy inventory requests".into())); }
                    let page = self.list_prefix(&prefix, offset, LEGACY_PAGE).await?;
                    if page.len() > LEGACY_PAGE as usize { return Err(TransportError::Malformed("legacy list page size".into())); }
                    for entry in &page {
                        // Offset pages over a changing folder repeat or skip names: start again later.
                        if !names.insert(entry.name.clone()) { return Err(http::unavailable("legacy list changed during inventory", None)); }
                        entries += 1;
                        if entries > LEGACY_INVENTORY_LIMIT { return Err(TransportError::LimitExceeded("legacy inventory size".into())); }
                        let key = format!("{prefix}/{}", entry.name);
                        match entry.id {
                            Some(_) => objects.push(ObjectRef { store:StoreKind::Supabase, bucket:"jobs".into(), key, version:None }),
                            None if depth >= LEGACY_INVENTORY_DEPTH => return Err(TransportError::LimitExceeded("legacy inventory depth".into())),
                            None => folders.push((key, depth + 1)),
                        }
                    }
                    if page.len() < LEGACY_PAGE as usize { break; }
                    offset += page.len() as u64;
                }
            }
            objects.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(objects)
        })
    }
}
