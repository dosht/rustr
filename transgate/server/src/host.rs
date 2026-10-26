//! Configuration and wiring of the job host on top of the composed billing runtime: the same
//! verified `billing_runtime` pool (a job's status change and its billing joins are one transaction
//! on one database), the real object stores and provider transports, the controlled external-URL
//! fetcher, the caption sources, the routes, and the background work. Every group is all-or-none and
//! nothing falls back to a fake.
//!
//! Environment (all of it needs `TRANSGATE_JOBS_ENABLED=1` and the billing configuration):
//! - `STORAGE_PROVIDER` (`gcs` or `cloudflare`) and that store's group: `MEDIA_GCS_*` or `MEDIA_R2_*`
//!   (`ENDPOINT`, `BUCKET`, `ACCESS_KEY`, `SECRET_KEY`, optional `REGION`, default `auto`). The other
//!   store's group may also be set: uploaded audio can live in either, and erasure reaches both.
//! - `TRANSGATE_ARCHIVE_BUCKET` and `TRANSGATE_STAGING_BUCKET`, in the upload store.
//! - Providers: `SONIOX_API_KEY`+`SONIOX_PROJECT_ID` (+`SONIOX_API_BASE_URL`) and/or
//!   `DEEPGRAM_API_KEY`+`DEEPGRAM_PROJECT_ID` (+`DEEPGRAM_API_BASE_URL`); `SONIOX_MAX_DURATION_MINUTES`.
//! - Captions: `TRANSCRIPT_API_KEY` (+`TRANSCRIPT_API_BASE_URL`), `YOUTUBE_API_KEY`
//!   (+`YOUTUBE_METADATA_BASE_URL`), optional `YOUTUBE_PUBLIC_BASE_URL`.
//! - Legacy storage erasure (required): `LEGACY_SUPABASE_URL` and `LEGACY_SUPABASE_SERVICE_ROLE_KEY`. The
//!   account's `jobs` bucket in the old Supabase project is inventoried and deleted by erasure; without
//!   it no erasure could finish, so the host refuses to start rather than leave one open forever.
//! - `CRON_SECRET` (the cron routes refuse every call while it is unset), `TRANSGATE_JOBS_TICK_SECONDS`
//!   (default 15), `TRANSGATE_SONIOX_AUTO_DELETE` (default on).
//! - The account's GoTrue deletion needs billing's `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY`.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use billing_core::freemium::FreemiumLists;
use billing_postgres::PgBillingStore;
use media_adapters::{
    ControlledFetcher, Deepgram, DeepgramConfig, FetchConfig, HttpConfig, LegacySupabaseConfig, LegacySupabaseStorage, PublicYouTube, PublicYouTubeConfig, S3Store,
    S3StoreConfig, Soniox, SonioxConfig, TranscriptApi, TranscriptApiConfig, YouTubeMetadata, YouTubeMetadataConfig,
};
use media_ports::{ExternalStager, ObjectInventory, ObjectStore, PaidCaptions, PublicCaptions, SecretString, StoreKind, Transcriber, VideoMetadataSource};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;
use transgate_billing::Transgate;
use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
use transgate_core::jobs::domain::StorageProvider;
use transgate_core::jobs::ports::{IdentityAdmin, JobQueries, JobStore, Locks, RateLimiter, Sleeper};
use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
use transgate_core::privacy::app::{Privacy, PrivacyServices};
use transgate_core::privacy::erasure::ExceptionPolicy;
use transgate_core::public_transcript::PublicTranscript;
use transgate_http::{DownloadEvents, TransgateHttp};
use transgate_jobs_postgres::{PgJobStore, PgLocks, PgRateLimiter};

use crate::ports::{DownloadAnalytics, GoTrueAdmin, TokioSleeper};
use crate::workers::{self, Workers};

type Get<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Clone, Debug)]
pub struct ObjectGroup {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Clone, Debug)]
pub struct ProviderGroup {
    pub base_url: String,
    pub api_key: String,
    pub project: String,
}

#[derive(Clone, Debug)]
pub struct KeyedBase {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct JobsSettings {
    pub callback_base: String,
    pub storage: StorageProvider,
    pub r2: Option<ObjectGroup>,
    pub gcs: Option<ObjectGroup>,
    pub archive_bucket: String,
    pub staging_bucket: String,
    pub soniox: Option<ProviderGroup>,
    pub deepgram: Option<ProviderGroup>,
    pub soniox_max_duration_minutes: i32,
    pub soniox_auto_delete: bool,
    pub transcript_api: KeyedBase,
    pub youtube_metadata: KeyedBase,
    pub youtube_public_base: String,
    pub legacy_supabase: KeyedBase,
    pub cron_secret: Option<String>,
    pub ai: crate::ai::Settings,
    /// Private deployment allow-lists; only effective capabilities reach the session DTO.
    pub flags: transgate_core::flags::FlagConfig,
    pub tick: Duration,
}

fn object_group(get: Get<'_>, prefix: &str) -> Result<Option<ObjectGroup>, String> {
    let name = |field: &str| format!("{prefix}_{field}");
    if !["ENDPOINT", "BUCKET", "REGION", "ACCESS_KEY", "SECRET_KEY"].iter().any(|f| get(&name(f)).is_some()) {
        return Ok(None);
    }
    let need = |field: &str| get(&name(field)).ok_or_else(|| format!("{} is required when another {prefix}_* variable is set", name(field)));
    Ok(Some(ObjectGroup {
        endpoint: need("ENDPOINT")?,
        bucket: need("BUCKET")?,
        region: get(&name("REGION")).unwrap_or_else(|| "auto".into()),
        access_key: need("ACCESS_KEY")?,
        secret_key: need("SECRET_KEY")?,
    }))
}

fn provider_group(get: Get<'_>, prefix: &str, default_base: &str) -> Result<Option<ProviderGroup>, String> {
    if !["API_KEY", "PROJECT_ID", "API_BASE_URL"].iter().any(|f| get(&format!("{prefix}_{f}")).is_some()) {
        return Ok(None);
    }
    let need = |field: &str| get(&format!("{prefix}_{field}")).ok_or_else(|| format!("{prefix}_{field} is required when another {prefix}_* variable is set"));
    Ok(Some(ProviderGroup { base_url: get(&format!("{prefix}_API_BASE_URL")).unwrap_or_else(|| default_base.into()), api_key: need("API_KEY")?, project: need("PROJECT_ID")? }))
}

fn required(get: Get<'_>, name: &str) -> Result<String, String> {
    get(name).ok_or_else(|| format!("{name} is required when TRANSGATE_JOBS_ENABLED=1"))
}

/// `Ok(None)`: the job host is off and Transgate serves what it did before. Otherwise every group
/// that is touched must be complete.
pub fn settings_from(get: Get<'_>) -> Result<Option<JobsSettings>, String> {
    if get("TRANSGATE_JOBS_ENABLED").as_deref() != Some("1") {
        return Ok(None);
    }
    let callback_base = required(get, "TRANSGATE_PUBLIC_URL")?;
    if !callback_base.starts_with("https://") {
        return Err("TRANSGATE_PUBLIC_URL must be an https origin: providers only call back over HTTPS".into());
    }
    let r2 = object_group(get, "MEDIA_R2")?;
    let gcs = object_group(get, "MEDIA_GCS")?;
    let storage = match required(get, "STORAGE_PROVIDER")?.as_str() {
        "gcs" => StorageProvider::Gcs,
        "cloudflare" => StorageProvider::Cloudflare,
        other => return Err(format!("STORAGE_PROVIDER must be gcs or cloudflare, not {other:?}")),
    };
    match (storage, &r2, &gcs) {
        (StorageProvider::Gcs, _, None) => return Err("STORAGE_PROVIDER=gcs needs the MEDIA_GCS_* group".into()),
        (StorageProvider::Cloudflare, None, _) => return Err("STORAGE_PROVIDER=cloudflare needs the MEDIA_R2_* group".into()),
        _ => {}
    }
    let soniox = provider_group(get, "SONIOX", "https://api.soniox.com")?;
    let deepgram = provider_group(get, "DEEPGRAM", "https://api.deepgram.com")?;
    if soniox.is_none() && deepgram.is_none() {
        return Err("configure at least one provider: SONIOX_* or DEEPGRAM_*".into());
    }
    let max_minutes = required(get, "SONIOX_MAX_DURATION_MINUTES")?
        .parse::<i32>()
        .ok()
        .filter(|m| *m > 0)
        .ok_or("SONIOX_MAX_DURATION_MINUTES must be a positive whole number")?;
    let legacy_supabase = KeyedBase { base_url: required(get, "LEGACY_SUPABASE_URL")?, api_key: required(get, "LEGACY_SUPABASE_SERVICE_ROLE_KEY")? };
    let tick = match get("TRANSGATE_JOBS_TICK_SECONDS") {
        None => 15,
        Some(text) => text.parse::<u64>().ok().filter(|n| *n > 0).ok_or("TRANSGATE_JOBS_TICK_SECONDS must be a positive whole number")?,
    };
    Ok(Some(JobsSettings {
        callback_base,
        storage,
        r2,
        gcs,
        archive_bucket: required(get, "TRANSGATE_ARCHIVE_BUCKET")?,
        staging_bucket: required(get, "TRANSGATE_STAGING_BUCKET")?,
        soniox,
        deepgram,
        soniox_max_duration_minutes: max_minutes,
        soniox_auto_delete: get("TRANSGATE_SONIOX_AUTO_DELETE").as_deref() != Some("0"),
        transcript_api: KeyedBase {
            base_url: get("TRANSCRIPT_API_BASE_URL").unwrap_or_else(|| "https://transcriptapi.com".into()),
            api_key: required(get, "TRANSCRIPT_API_KEY")?,
        },
        youtube_metadata: KeyedBase {
            base_url: get("YOUTUBE_METADATA_BASE_URL").unwrap_or_else(|| "https://www.googleapis.com".into()),
            api_key: required(get, "YOUTUBE_API_KEY")?,
        },
        youtube_public_base: get("YOUTUBE_PUBLIC_BASE_URL").unwrap_or_else(|| "https://www.youtube.com".into()),
        legacy_supabase,
        cron_secret: get("CRON_SECRET"),
        ai: crate::ai::Settings::from_env(get)?,
        flags: crate::account::flags_from(get)?,
        tick: Duration::from_secs(tick),
    }))
}

pub struct JobHost {
    pub router: Router,
    /// The background tick; abort it on shutdown.
    pub workers: JoinHandle<()>,
}

fn transport(e: media_ports::TransportError) -> String {
    e.to_string()
}

fn s3(kind: StoreKind, group: &ObjectGroup, http: &HttpConfig) -> Result<Arc<dyn ObjectStore>, String> {
    S3Store::new(S3StoreConfig {
        kind,
        endpoint: group.endpoint.clone(),
        bucket: group.bucket.clone(),
        region: group.region.clone(),
        access_key: SecretString::new(group.access_key.clone()),
        secret_key: SecretString::new(group.secret_key.clone()),
        http: http.clone(),
    })
    .map(|store| Arc::new(store) as Arc<dyn ObjectStore>)
    .map_err(transport)
}

/// Every (store kind, bucket) the host reads or deletes, each once, with the credentials of its group.
/// An `S3Store` is bound to one bucket, so the upload bucket, the archive bucket and the staging
/// bucket (which live in the upload store) and each configured group's own bucket each need an adapter.
fn bucket_stores(settings: &JobsSettings) -> Vec<(StoreKind, ObjectGroup)> {
    let upload_kind = settings.storage.store_kind();
    let mut stores: Vec<(StoreKind, ObjectGroup)> = Vec::new();
    for (kind, group) in [(StoreKind::R2, &settings.r2), (StoreKind::Gcs, &settings.gcs)] {
        let Some(group) = group else { continue };
        let extra = if Some(kind) == upload_kind { vec![settings.archive_bucket.clone(), settings.staging_bucket.clone()] } else { vec![] };
        for bucket in std::iter::once(group.bucket.clone()).chain(extra) {
            if !stores.iter().any(|(k, g)| *k == kind && g.bucket == bucket) {
                stores.push((kind, ObjectGroup { bucket, ..group.clone() }));
            }
        }
    }
    stores
}

/// Builds the host on the pool billing already verified. `owner_url` (the billing owner migration URL)
/// migrates the job schema when given; `freemium` is the billing configuration's own exception lists.
pub async fn compose(settings: JobsSettings, billing: &Transgate, owner_url: Option<&str>, freemium: FreemiumLists) -> Result<JobHost, String> {
    if let Some(url) = owner_url {
        let owner = PgPoolOptions::new().max_connections(2).connect(url).await.map_err(|e| format!("TRANSGATE_MIGRATION_DATABASE_URL: {e}"))?;
        transgate_jobs_postgres::migrate(&owner).await.map_err(|e| e.to_string())?;
        owner.close().await;
    }
    let pool = billing.pool.clone();
    transgate_jobs_postgres::verify_runtime(&pool).await.map_err(|e| e.to_string())?;
    let auth_admin = billing.auth_admin.clone().ok_or("account erasure deletes the identity: set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY")?;

    let http = HttpConfig::default();
    let mut objects: Vec<Arc<dyn ObjectStore>> = Vec::new();
    for (kind, group) in bucket_stores(&settings) {
        objects.push(s3(kind, &group, &http)?);
    }
    // One adapter is the object store and, once core takes inventories, the owner inventory too.
    let legacy = LegacySupabaseStorage::new(LegacySupabaseConfig {
        base_url: settings.legacy_supabase.base_url.clone(),
        service_key: SecretString::new(settings.legacy_supabase.api_key.clone()),
        http: http.clone(),
    })
    .map(Arc::new)
    .map_err(transport)?;
    objects.push(legacy.clone());
    // The same adapter lists an owner's legacy objects so erasure can prove that none remain.
    let inventories: Vec<Arc<dyn ObjectInventory>> = vec![legacy];
    let mut transcribers: Vec<Arc<dyn Transcriber>> = Vec::new();
    if let Some(s) = &settings.soniox {
        let soniox = Soniox::new(SonioxConfig {
            base_url: s.base_url.clone(),
            api_key: SecretString::new(s.api_key.clone()),
            project: s.project.clone(),
            model: "stt-async-v5".into(),
            http: http.clone(),
        })
        .map_err(transport)?;
        transcribers.push(Arc::new(soniox));
    }
    if let Some(d) = &settings.deepgram {
        let deepgram = Deepgram::new(DeepgramConfig { base_url: d.base_url.clone(), api_key: SecretString::new(d.api_key.clone()), project: d.project.clone(), http: http.clone() }).map_err(transport)?;
        transcribers.push(Arc::new(deepgram));
    }
    let stager: Arc<dyn ExternalStager> = Arc::new(ControlledFetcher::new(FetchConfig { http: http.clone(), max_redirects: 5 }).map_err(transport)?);
    let paid: Arc<dyn PaidCaptions> =
        Arc::new(TranscriptApi::new(TranscriptApiConfig { base_url: settings.transcript_api.base_url.clone(), api_key: SecretString::new(settings.transcript_api.api_key.clone()), http: http.clone() }).map_err(transport)?);
    let metadata: Arc<dyn VideoMetadataSource> = Arc::new(
        YouTubeMetadata::new(YouTubeMetadataConfig { base_url: settings.youtube_metadata.base_url.clone(), api_key: SecretString::new(settings.youtube_metadata.api_key.clone()), http: http.clone() }).map_err(transport)?,
    );
    let public_captions: Arc<dyn PublicCaptions> = Arc::new(PublicYouTube::new(PublicYouTubeConfig { base_url: settings.youtube_public_base.clone(), http: http.clone() }).map_err(transport)?);

    let clock: Arc<dyn billing_core::types::Clock> = Arc::new(transgate_billing::SystemClock);
    let store = PgJobStore::new(pool.clone());
    let job_store: Arc<dyn JobStore> = Arc::new(store.clone());
    let queries: Arc<dyn JobQueries> = Arc::new(store.clone());
    let locks: Arc<dyn Locks> = Arc::new(PgLocks::new(pool.clone()));
    let limiter: Arc<dyn RateLimiter> = Arc::new(PgRateLimiter::new(pool.clone()));
    let sleeper: Arc<dyn Sleeper> = Arc::new(TokioSleeper);
    let upload_kind = settings.storage.store_kind().ok_or("the upload storage has no object store")?;
    let upload_bucket = match settings.storage {
        StorageProvider::Gcs => settings.gcs.as_ref().map(|g| g.bucket.clone()),
        _ => settings.r2.as_ref().map(|g| g.bucket.clone()),
    }
    .ok_or("the upload storage group is not configured")?;
    let ai_chat = settings.ai.policy.chat_enabled;
    let ai = crate::ai::compose(settings.ai, job_store.clone(), clock.clone(), sleeper.clone())?;

    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: job_store.clone(),
            clock: clock.clone(),
            objects: objects.clone(),
            transcribers: transcribers.clone(),
            stager: Some(stager),
            sleeper: sleeper.clone(),
            rate_limiter: limiter.clone(),
            paid_captions: Some(paid),
            video_metadata: Some(metadata),
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: settings.soniox_max_duration_minutes },
            models: ModelConfig { soniox: "stt-async-v5".into(), soniox_v5: "stt-async-v5".into(), deepgram: "whisper-large".into(), deepgram_account_overrides: vec![] },
            upload_storage: settings.storage,
            upload_bucket,
            archive_store: upload_kind,
            archive_bucket: settings.archive_bucket.clone(),
            staging_store: upload_kind,
            staging_bucket: settings.staging_bucket.clone(),
            callback_base_url: settings.callback_base.clone(),
            soniox_auto_delete: settings.soniox_auto_delete,
            ssrf_protection: true,
            freemium_lists: freemium,
        },
    ));
    let identity_admin: Arc<dyn IdentityAdmin> = Arc::new(GoTrueAdmin(auth_admin));
    let privacy = Arc::new(Privacy::new(PrivacyServices {
        store: job_store.clone(),
        clock: clock.clone(),
        locks: locks.clone(),
        objects,
        inventories,
        transcribers: transcribers.clone(),
        identity: identity_admin,
        billing: Arc::new(PgBillingStore::new(pool.clone())),
        terminators: billing.iyzico.iter().map(|c| c.clone() as Arc<dyn billing_core::erasure::SubscriptionTermination>).collect(),
        exceptions: ExceptionPolicy::default(),
    }));
    let account_router = crate::account::router(
        billing, transgate_core::account::AccountServices { store: job_store, clock: clock.clone() },
        privacy.clone(), settings.flags, ai_chat,
    );
    let downloads: Option<Arc<dyn DownloadEvents>> =
        billing.analytics.clone().map(|sink| Arc::new(DownloadAnalytics { sink, clock: clock.clone() }) as Arc<dyn DownloadEvents>);
    let http_state = Arc::new(TransgateHttp {
        jobs: jobs.clone(),
        privacy: privacy.clone(),
        ai: Some(ai.clone()),
        public_transcript: Arc::new(PublicTranscript::new(public_captions, limiter, clock.clone())),
        queries,
        transcribers,
        identity: billing.identity.clone(),
        clock: clock.clone(),
        cron_secret: settings.cron_secret.clone().map(SecretString::new),
        downloads,
    });
    let background = Arc::new(Workers {
        jobs,
        privacy,
        ai,
        store,
        pool: pool.clone(),
        mailer: billing.mailer.clone(),
        directory: transgate_billing::directory::PgCustomerDirectory::new(pool),
        clock,
        locks,
    });
    Ok(JobHost { router: transgate_http::router(http_state).merge(account_router), workers: workers::spawn(background, settings.tick) })
}

#[cfg(test)]
mod tests;
