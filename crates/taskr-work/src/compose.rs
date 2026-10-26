//! Configuration and wiring for the audio-work sample. Each provider group is
//! all-or-none, the runtime login must be restricted, and nothing falls back to
//! a fake: an incomplete group refuses to start.
//!
//! Environment:
//! - `TASKR_WORK_ENABLED=1`: enables the sample. It runs on the billing sample's database and its
//!   verified `billing_runtime` pool (`TASKR_BILLING_DATABASE_URL`, and
//!   `TASKR_BILLING_MIGRATION_DATABASE_URL` for the owner migrations), so account erasure is one
//!   transaction over billing, the billing sample and this sample. `TASKR_WORK_DATABASE_URL` is
//!   refused: a second database cannot be atomic with billing.
//! - `TASKR_PUBLIC_URL`: the public HTTPS origin providers call back.
//! - Exactly one object store group: `MEDIA_R2_*` or `MEDIA_GCS_*`, each with
//!   `ENDPOINT`, `BUCKET`, `ACCESS_KEY`, `SECRET_KEY` and optional `REGION` (default `auto`).
//! - At least one provider group. Soniox: `SONIOX_API_KEY`, `SONIOX_PROJECT_ID`, optional
//!   `SONIOX_API_BASE_URL` (default `https://api.soniox.com`) and `SONIOX_MODEL` (only
//!   `stt-async-v5`). Deepgram: `DEEPGRAM_API_KEY`, `DEEPGRAM_PROJECT_ID`, optional
//!   `DEEPGRAM_API_BASE_URL` (default `https://api.deepgram.com`).
//! - `TASKR_WORK_TICK_SECONDS` (default 10) and `TASKR_WORK_RETENTION_DAYS` (default 30).
//! - Optional explicit summary: `TASKR_SUMMARY_ENABLED=1`, `TASKR_SUMMARY_PROVIDER` (`deepseek` or
//!   `openai`), `TASKR_SUMMARY_API_KEY`, `TASKR_SUMMARY_MODEL`, optional `TASKR_SUMMARY_BASE_URL`.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use billing_http::BillingIdentity;
use media_adapters::{Deepgram, DeepgramConfig, HttpConfig, S3Store, S3StoreConfig, Soniox, SonioxConfig};
use media_ports::{ObjectStore, SecretString, StoreKind, Transcriber};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;

use crate::app::{Config, Service, system_clock};
use crate::http::{WorkHttp, router};
use crate::store::{self, Store};
use crate::workers;

#[derive(Clone, Debug)]
pub struct ObjectSettings {
    pub kind: StoreKind,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Clone, Debug)]
pub struct SonioxSettings {
    pub base_url: String,
    pub api_key: String,
    pub project: String,
    pub model: String,
}

#[derive(Clone, Debug)]
pub struct DeepgramSettings {
    pub base_url: String,
    pub api_key: String,
    pub project: String,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub migration_database_url: Option<String>,
    pub public_url: String,
    pub objects: ObjectSettings,
    pub soniox: Option<SonioxSettings>,
    pub deepgram: Option<DeepgramSettings>,
    pub summary: Option<crate::summary::compose::SummarySettings>,
    pub tick: Duration,
    pub retention_days: i64,
}

type Get<'a> = &'a dyn Fn(&str) -> Option<String>;

fn object_group(get: Get<'_>, prefix: &str, kind: StoreKind) -> Result<Option<ObjectSettings>, String> {
    let name = |field: &str| format!("{prefix}_{field}");
    let fields = ["ENDPOINT", "BUCKET", "REGION", "ACCESS_KEY", "SECRET_KEY"];
    if !fields.iter().any(|f| get(&name(f)).is_some()) {
        return Ok(None);
    }
    let need = |field: &str| get(&name(field)).ok_or_else(|| format!("{} is required when another {prefix}_* variable is set", name(field)));
    Ok(Some(ObjectSettings {
        kind,
        endpoint: need("ENDPOINT")?,
        bucket: need("BUCKET")?,
        region: get(&name("REGION")).unwrap_or_else(|| "auto".into()),
        access_key: need("ACCESS_KEY")?,
        secret_key: need("SECRET_KEY")?,
    }))
}

fn provider_touched(get: Get<'_>, prefix: &str) -> bool {
    ["API_KEY", "PROJECT_ID", "API_BASE_URL"].iter().any(|f| get(&format!("{prefix}_{f}")).is_some())
}

/// `Ok(None)`: no work database, so taskr runs exactly as before. Otherwise every group that is
/// touched must be complete.
pub fn settings_from(get: Get<'_>) -> Result<Option<Settings>, String> {
    if get("TASKR_WORK_DATABASE_URL").is_some() {
        return Err("TASKR_WORK_DATABASE_URL is not supported: the audio-work sample runs on the billing sample's database (TASKR_BILLING_DATABASE_URL) so that billing erasure and its own rows commit in one transaction".into());
    }
    if get("TASKR_WORK_ENABLED").as_deref() != Some("1") {
        return Ok(None);
    }
    let public_url = get("TASKR_PUBLIC_URL").ok_or("TASKR_PUBLIC_URL is required when TASKR_WORK_ENABLED=1")?;
    if !public_url.starts_with("https://") {
        return Err("TASKR_PUBLIC_URL must be an https origin: providers only call back over HTTPS".into());
    }
    let r2 = object_group(get, "MEDIA_R2", StoreKind::R2)?;
    let gcs = object_group(get, "MEDIA_GCS", StoreKind::Gcs)?;
    let objects = match (r2, gcs) {
        (Some(_), Some(_)) => return Err("set exactly one of the MEDIA_R2_* and MEDIA_GCS_* object store groups".into()),
        (Some(group), None) | (None, Some(group)) => group,
        (None, None) => return Err("an object store is required: configure MEDIA_R2_* or MEDIA_GCS_*".into()),
    };
    let soniox = if provider_touched(get, "SONIOX") {
        let model = get("SONIOX_MODEL").unwrap_or_else(|| "stt-async-v5".into());
        if model != "stt-async-v5" {
            return Err("SONIOX_MODEL must be stt-async-v5".into());
        }
        Some(SonioxSettings {
            base_url: get("SONIOX_API_BASE_URL").unwrap_or_else(|| "https://api.soniox.com".into()),
            api_key: get("SONIOX_API_KEY").ok_or("SONIOX_API_KEY is required when another SONIOX_* variable is set")?,
            project: get("SONIOX_PROJECT_ID").ok_or("SONIOX_PROJECT_ID is required when another SONIOX_* variable is set")?,
            model,
        })
    } else {
        None
    };
    let deepgram = if provider_touched(get, "DEEPGRAM") {
        Some(DeepgramSettings {
            base_url: get("DEEPGRAM_API_BASE_URL").unwrap_or_else(|| "https://api.deepgram.com".into()),
            api_key: get("DEEPGRAM_API_KEY").ok_or("DEEPGRAM_API_KEY is required when another DEEPGRAM_* variable is set")?,
            project: get("DEEPGRAM_PROJECT_ID").ok_or("DEEPGRAM_PROJECT_ID is required when another DEEPGRAM_* variable is set")?,
        })
    } else {
        None
    };
    if soniox.is_none() && deepgram.is_none() {
        return Err("configure at least one provider: SONIOX_* or DEEPGRAM_*".into());
    }
    let number = |name: &str, default: u64| -> Result<u64, String> {
        match get(name) {
            None => Ok(default),
            Some(text) => text.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| format!("{name} must be a positive whole number")),
        }
    };
    Ok(Some(Settings {
        migration_database_url: get("TASKR_BILLING_MIGRATION_DATABASE_URL"),
        public_url,
        objects,
        soniox,
        deepgram,
        tick: Duration::from_secs(number("TASKR_WORK_TICK_SECONDS", 10)?),
        retention_days: number("TASKR_WORK_RETENTION_DAYS", 30)? as i64,
        summary: crate::summary::compose::settings_from(get)?,
    }))
}

pub struct TaskrWork {
    /// The audio-work routes, ready to merge into taskr's router.
    pub router: Router,
    pub service: Arc<Service>,
    /// The background tick; abort it on shutdown.
    pub workers: JoinHandle<()>,
}

/// The restricted group the billing sample's runtime login acts as.
const RUNTIME_ROLE: &str = "billing_runtime";

fn transport(e: media_ports::TransportError) -> String {
    e.to_string()
}

/// Wires the sample on the billing sample's verified runtime pool, builds the real adapters and
/// starts the background tick.
///
/// The composition invariant: the audio-work rows, billing's rows and the billing sample's rows
/// live in ONE database and one transaction (account erasure removes all of them atomically).
/// So there is no second database URL: the sample runs on `pool`, the pool the billing sample
/// already verified as the restricted `billing_runtime` login. A migration URL, if given, is
/// only checked by asking the connected database whether it holds every schema.
pub async fn compose(settings: Settings, pool: PgPool, identity: Arc<dyn BillingIdentity>) -> Result<TaskrWork, String> {
    if let Some(url) = &settings.migration_database_url {
        let owner = PgPoolOptions::new().max_connections(2).connect(url).await.map_err(|e| format!("TASKR_BILLING_MIGRATION_DATABASE_URL: {e}"))?;
        media_postgres::migrate(&owner).await.map_err(|e| e.to_string())?;
        store::migrate(&owner).await.map_err(|e| e.to_string())?;
        for role in [RUNTIME_ROLE] {
            store::grant_runtime(&owner, role).await.map_err(|e| e.to_string())?;
            media_postgres::grant_runtime(&owner, role).await.map_err(|e| e.to_string())?;
        }
        owner.close().await;
    }
    // The pool's own database must hold both this sample's schema and billing's: a migration
    // URL or a runtime URL that names another database fails here, not at the first erasure.
    store::verify_same_database(&pool).await.map_err(|e| e.to_string())?;
    let problems = media_postgres::runtime_authority_problems(&pool, RUNTIME_ROLE).await.map_err(|e| e.to_string())?;
    if !problems.is_empty() {
        return Err(format!("the audio-work runtime authority is not restricted: {}", problems.join("; ")));
    }

    let http = HttpConfig::default();
    let objects: Arc<dyn ObjectStore> = Arc::new(
        S3Store::new(S3StoreConfig {
            kind: settings.objects.kind,
            endpoint: settings.objects.endpoint.clone(),
            bucket: settings.objects.bucket.clone(),
            region: settings.objects.region.clone(),
            access_key: SecretString::new(settings.objects.access_key.clone()),
            secret_key: SecretString::new(settings.objects.secret_key.clone()),
            http: http.clone(),
        })
        .map_err(transport)?,
    );
    let mut transcribers: Vec<Arc<dyn Transcriber>> = Vec::new();
    if let Some(s) = &settings.soniox {
        transcribers.push(Arc::new(
            Soniox::new(SonioxConfig {
                base_url: s.base_url.clone(),
                api_key: SecretString::new(s.api_key.clone()),
                project: s.project.clone(),
                model: s.model.clone(),
                http: http.clone(),
            })
            .map_err(transport)?,
        ));
    }
    if let Some(d) = &settings.deepgram {
        transcribers.push(Arc::new(
            Deepgram::new(DeepgramConfig { base_url: d.base_url.clone(), api_key: SecretString::new(d.api_key.clone()), project: d.project.clone(), http })
                .map_err(transport)?,
        ));
    }

    let mut config = Config::new(settings.public_url.clone());
    config.retention_days = settings.retention_days;
    // taskr's sample has no external recurring subscriptions to cancel: LemonSqueezy is
    // pay-as-you-go packs, so no terminator is configured.
    let store = Store::new(pool);
    let clock = system_clock();
    let service = Arc::new(Service::new(store.clone(), objects, transcribers, Vec::new(), config, clock.clone()));
    let (llm, provider_name, model) = match &settings.summary {
        Some(s) => (Some(crate::summary::compose::provider(s, llm_http::DispatchCapacity::default())?), s.provider.clone(), s.model.clone()),
        None => (None, String::new(), String::new()),
    };
    let summary = Arc::new(crate::summary::app::SummaryService::new(store, llm, provider_name, model, clock));
    let workers = workers::spawn(service.clone(), settings.tick);
    let router = router(Arc::new(WorkHttp { service: service.clone(), identity: identity.clone() }))
        .merge(crate::summary::http::router(Arc::new(crate::summary::http::SummaryHttp { service: summary, identity })));
    Ok(TaskrWork { router, service, workers })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn from(pairs: &[(&str, &str)]) -> Result<Option<Settings>, String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        settings_from(&|name| map.get(name).cloned())
    }

    const BASE: [(&str, &str); 2] = [("TASKR_WORK_ENABLED", "1"), ("TASKR_PUBLIC_URL", "https://taskr.example.test")];
    const R2: [(&str, &str); 4] =
        [("MEDIA_R2_ENDPOINT", "https://r2.example.test"), ("MEDIA_R2_BUCKET", "audio"), ("MEDIA_R2_ACCESS_KEY", "a"), ("MEDIA_R2_SECRET_KEY", "s")];
    const SONIOX: [(&str, &str); 2] = [("SONIOX_API_KEY", "k"), ("SONIOX_PROJECT_ID", "p")];

    fn with<'a>(parts: &[&[(&'a str, &'a str)]]) -> Vec<(&'a str, &'a str)> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    #[test]
    fn the_sample_is_off_without_its_database_and_otherwise_every_group_must_be_complete() {
        assert_eq!(from(&[]).unwrap().map(|_| ()), None);
        let ok = with(&[&BASE, &R2, &SONIOX]);
        assert!(from(&ok).unwrap().is_some());

        let no_store = with(&[&BASE, &SONIOX]);
        assert!(from(&no_store).unwrap_err().contains("object store"));
        let no_provider = with(&[&BASE, &R2]);
        assert!(from(&no_provider).unwrap_err().contains("provider"));
        let half_provider = with(&[&BASE, &R2, &[("SONIOX_API_KEY", "k")]]);
        assert!(from(&half_provider).unwrap_err().contains("SONIOX_PROJECT_ID"));
        let half_store = with(&[&BASE, &SONIOX, &[("MEDIA_R2_BUCKET", "audio")]]);
        assert!(from(&half_store).unwrap_err().contains("MEDIA_R2_ENDPOINT"));
    }

    #[test]
    fn a_second_database_url_is_refused_because_it_could_not_be_atomic_with_billing() {
        let two_urls = with(&[&BASE, &R2, &SONIOX, &[("TASKR_WORK_DATABASE_URL", "postgres://work@localhost/other")]]);
        let why = from(&two_urls).unwrap_err();
        assert!(why.contains("TASKR_WORK_DATABASE_URL is not supported") && why.contains("one transaction"), "{why}");
        // Even without the enabling variable a leftover second URL fails loudly, never silently ignored.
        let only_url = [("TASKR_WORK_DATABASE_URL", "postgres://work@localhost/other")];
        assert!(from(&only_url).is_err());
        assert_eq!(from(&[]).unwrap().map(|_| ()), None, "off by default");
    }

    #[test]
    fn two_object_stores_an_insecure_callback_origin_and_another_soniox_model_are_refused() {
        let both = with(&[&BASE, &R2, &SONIOX, &[("MEDIA_GCS_ENDPOINT", "https://g"), ("MEDIA_GCS_BUCKET", "b"), ("MEDIA_GCS_ACCESS_KEY", "a"), ("MEDIA_GCS_SECRET_KEY", "s")]]);
        assert!(from(&both).unwrap_err().contains("exactly one"));
        let http = with(&[&[("TASKR_WORK_ENABLED", "1"), ("TASKR_PUBLIC_URL", "http://taskr.example.test")], &R2, &SONIOX]);
        assert!(from(&http).unwrap_err().contains("https"));
        let model = with(&[&BASE, &R2, &SONIOX, &[("SONIOX_MODEL", "stt-async-v4")]]);
        assert!(from(&model).unwrap_err().contains("stt-async-v5"));
    }
}
