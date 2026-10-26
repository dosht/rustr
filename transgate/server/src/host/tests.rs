use std::collections::HashMap;

use super::*;

/// Later pairs override earlier ones, so a case is its base plus what it changes.
fn from(pairs: &[(&str, &str)]) -> Result<Option<JobsSettings>, String> {
    let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    settings_from(&|name| map.get(name).cloned())
}

const BASE: [(&str, &str); 11] = [
    ("TRANSGATE_JOBS_ENABLED", "1"),
    ("TRANSGATE_PUBLIC_URL", "https://transgate.example.test"),
    ("STORAGE_PROVIDER", "cloudflare"),
    ("SONIOX_MAX_DURATION_MINUTES", "200"),
    ("TRANSGATE_ARCHIVE_BUCKET", "archive"),
    ("TRANSGATE_STAGING_BUCKET", "staging"),
    ("TRANSCRIPT_API_KEY", "t"),
    ("YOUTUBE_API_KEY", "y"),
    ("CRON_SECRET", "cron"),
    ("LEGACY_SUPABASE_URL", "https://old.supabase.example.test"),
    ("LEGACY_SUPABASE_SERVICE_ROLE_KEY", "legacy"),
];
const R2: [(&str, &str); 4] = [("MEDIA_R2_ENDPOINT", "https://r2.example.test"), ("MEDIA_R2_BUCKET", "audio"), ("MEDIA_R2_ACCESS_KEY", "a"), ("MEDIA_R2_SECRET_KEY", "s")];
const SONIOX: [(&str, &str); 2] = [("SONIOX_API_KEY", "k"), ("SONIOX_PROJECT_ID", "p")];

fn with<'a>(parts: &[&[(&'a str, &'a str)]]) -> Vec<(&'a str, &'a str)> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

fn without<'a>(all: &[(&'a str, &'a str)], name: &str) -> Vec<(&'a str, &'a str)> {
    all.iter().copied().filter(|(k, _)| *k != name).collect()
}

#[test]
fn the_job_host_is_off_by_default_and_a_complete_configuration_parses() {
    assert_eq!(from(&[]).unwrap().map(|_| ()), None);
    assert_eq!(from(&[("TRANSGATE_JOBS_ENABLED", "0")]).unwrap().map(|_| ()), None);
    let settings = from(&with(&[&BASE, &R2, &SONIOX])).unwrap().unwrap();
    assert_eq!((settings.storage, settings.soniox_max_duration_minutes, settings.soniox_auto_delete), (StorageProvider::Cloudflare, 200, true));
    assert_eq!(settings.cron_secret.as_deref(), Some("cron"));
    assert_eq!(settings.youtube_public_base, "https://www.youtube.com");
    assert_eq!(settings.legacy_supabase.base_url, "https://old.supabase.example.test");
    assert!(settings.deepgram.is_none());
}

#[test]
fn every_required_value_and_every_touched_group_must_be_complete() {
    let ok = with(&[&BASE, &R2, &SONIOX]);
    for name in [
        "TRANSGATE_PUBLIC_URL",
        "STORAGE_PROVIDER",
        "SONIOX_MAX_DURATION_MINUTES",
        "TRANSGATE_ARCHIVE_BUCKET",
        "TRANSCRIPT_API_KEY",
        "YOUTUBE_API_KEY",
        "LEGACY_SUPABASE_URL",
        "LEGACY_SUPABASE_SERVICE_ROLE_KEY",
    ] {
        let why = from(&without(&ok, name)).unwrap_err();
        assert!(why.contains(name), "{name}: {why}");
    }
    assert!(from(&with(&[&BASE, &SONIOX])).unwrap_err().contains("MEDIA_R2_*"), "cloudflare needs its group");
    assert!(from(&with(&[&BASE, &R2, &SONIOX, &[("STORAGE_PROVIDER", "gcs")]])).unwrap_err().contains("MEDIA_GCS_*"), "gcs needs its group even when the other is set");
    assert!(from(&with(&[&BASE, &R2])).unwrap_err().contains("provider"), "no provider");
    assert!(from(&with(&[&BASE, &R2, &[("SONIOX_API_KEY", "k")]])).unwrap_err().contains("SONIOX_PROJECT_ID"));
    assert!(from(&with(&[&BASE, &[("MEDIA_R2_BUCKET", "audio")], &SONIOX])).unwrap_err().contains("MEDIA_R2_ENDPOINT"));
}

#[test]
fn an_insecure_callback_origin_a_bad_number_and_a_wrong_store_name_are_refused() {
    let insecure = with(&[&BASE, &R2, &SONIOX, &[("TRANSGATE_PUBLIC_URL", "http://transgate.example.test")]]);
    assert!(from(&insecure).unwrap_err().contains("https"));
    let bad_minutes = with(&[&BASE, &R2, &SONIOX, &[("SONIOX_MAX_DURATION_MINUTES", "zero")]]);
    assert!(from(&bad_minutes).unwrap_err().contains("SONIOX_MAX_DURATION_MINUTES"));
    let wrong_store = with(&[&BASE, &R2, &SONIOX, &[("STORAGE_PROVIDER", "s3")]]);
    assert!(from(&wrong_store).unwrap_err().contains("gcs or cloudflare"));
}

fn buckets(pairs: &[(&str, &str)]) -> Vec<(StoreKind, String)> {
    let settings = from(pairs).unwrap().unwrap();
    bucket_stores(&settings).into_iter().map(|(kind, group)| (kind, group.bucket)).collect()
}

#[test]
fn the_upload_archive_and_staging_buckets_each_get_their_own_store_with_the_groups_credentials() {
    let settings = from(&with(&[&BASE, &R2, &SONIOX])).unwrap().unwrap();
    let stores = bucket_stores(&settings);
    let names: Vec<(StoreKind, &str)> = stores.iter().map(|(kind, group)| (*kind, group.bucket.as_str())).collect();
    assert_eq!(names, vec![(StoreKind::R2, "audio"), (StoreKind::R2, "archive"), (StoreKind::R2, "staging")]);
    assert!(stores.iter().all(|(_, g)| g.endpoint == "https://r2.example.test" && g.access_key == "a" && g.secret_key == "s"));
}

#[test]
fn a_bucket_shared_by_two_roles_or_a_second_store_is_instantiated_once_per_kind_and_bucket() {
    let shared = with(&[&BASE, &R2, &SONIOX, &[("TRANSGATE_ARCHIVE_BUCKET", "audio"), ("TRANSGATE_STAGING_BUCKET", "audio")]]);
    assert_eq!(buckets(&shared), vec![(StoreKind::R2, "audio".to_string())]);
    let both = with(&[
        &BASE,
        &R2,
        &SONIOX,
        &[("MEDIA_GCS_ENDPOINT", "https://storage.example.test"), ("MEDIA_GCS_BUCKET", "audio"), ("MEDIA_GCS_ACCESS_KEY", "g"), ("MEDIA_GCS_SECRET_KEY", "h")],
    ]);
    assert_eq!(
        buckets(&both),
        vec![(StoreKind::R2, "audio".to_string()), (StoreKind::R2, "archive".to_string()), (StoreKind::R2, "staging".to_string()), (StoreKind::Gcs, "audio".to_string())],
        "archive and staging follow the upload store only; the other store keeps its own bucket"
    );
}
