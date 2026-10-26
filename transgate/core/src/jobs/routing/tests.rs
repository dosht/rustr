use super::*;
use crate::jobs::domain::JobStatus;
use crate::languages::Language;
use crate::presentation::fixtures;

const CONFIG: RoutingConfig = RoutingConfig { soniox_max_duration_minutes: 200 };

fn all() -> Availability {
    Availability { configured: [Provider::Soniox, Provider::SonioxV5, Provider::Deepgram].into_iter().collect(), deepgram_inflight: 0 }
}

fn job(language: Option<&str>, minutes: i32, name: &str) -> Job {
    let mut j = fixtures::job(1, JobStatus::NotStarted, name);
    j.language = language.map(|l| Language::from_name(l).unwrap());
    j.duration_minutes = Some(minutes);
    j
}

fn first(j: &Job, prefs: &Preferences, a: &Availability) -> Route {
    first_choice(j, prefs, a, &CONFIG)
}

fn submit(p: Provider, r: RouteReason) -> Route {
    Route::Submit { provider: p, reason: r }
}

// TRX-005/006, LNG-D06: every Soniox language and auto-detect goes to Soniox.
#[test]
fn soniox_takes_its_languages_and_auto_detect() {
    let prefs = Preferences::default();
    for l in Language::all().filter(|l| l.soniox()) {
        let j = job(Some(l.name()), 10, "a.mp3");
        assert_eq!(first(&j, &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox), "{l}");
    }
    assert_eq!(first(&job(None, 10, "a.mp3"), &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox));
}

// TRX-007/011/012, LNG-D05: the eight non-Soniox languages go to Deepgram deterministically.
#[test]
fn eight_languages_go_to_deepgram() {
    for l in Language::all().filter(|l| !l.soniox()) {
        let j = job(Some(l.name()), 10, "a.mp3");
        assert_eq!(first(&j, &Preferences::default(), &all()), submit(Provider::Deepgram, RouteReason::FallbackChain), "{l}");
    }
}

// TRX-002: strictly longer than the threshold, non-translation, no availability check.
#[test]
fn long_audio_goes_to_deepgram() {
    let prefs = Preferences::default();
    assert_eq!(first(&job(Some("English"), 200, "a.mp3"), &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox));
    let none = Availability { configured: [Provider::Soniox].into_iter().collect(), deepgram_inflight: 9 };
    assert_eq!(first(&job(Some("English"), 201, "a.mp3"), &prefs, &none), submit(Provider::Deepgram, RouteReason::DurationOverride));
    let mut t = job(Some("English"), 500, "a.mp3");
    t.translate_to = Language::from_name("German").into_iter().collect();
    assert_eq!(first(&t, &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox));
}

// TRX-003
#[test]
fn qta_goes_to_deepgram_unless_translation() {
    let prefs = Preferences::default();
    assert_eq!(first(&job(Some("English"), 5, "x.QTA"), &prefs, &all()), submit(Provider::Deepgram, RouteReason::QtaOverride));
    let mut t = job(Some("English"), 5, "x.qta");
    t.translate_to = Language::from_name("German").into_iter().collect();
    assert_eq!(first(&t, &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox));
}

// TRX-004: a valid preference wins after the overrides; an invalid one is ignored.
#[test]
fn preferred_provider_needs_to_be_valid() {
    let prefs = Preferences { preferred: Some(Provider::Deepgram), fallback_enabled: true };
    assert_eq!(first(&job(Some("English"), 5, "a.mp3"), &prefs, &all()), submit(Provider::Deepgram, RouteReason::Preferred));
    assert_eq!(first(&job(None, 5, "a.mp3"), &prefs, &all()), submit(Provider::Soniox, RouteReason::Soniox));
    let full = Availability { deepgram_inflight: 4, ..all() };
    assert_eq!(first(&job(Some("English"), 5, "a.mp3"), &prefs, &full), submit(Provider::Soniox, RouteReason::Soniox));
}

// TRX-001: a stored provider is the first attempt as-is, including RunPod.
#[test]
fn stored_provider_comes_first() {
    for pin in Provider::ALL {
        let mut j = job(Some("English"), 500, "a.qta");
        j.provider = Some(pin);
        assert_eq!(first(&j, &Preferences::default(), &all()), submit(pin, RouteReason::StoredProvider));
    }
}

// Recorded corrections: a removed RunPod pin is unavailable, an incompatible pin
// is a failed first attempt, both followed by compatible failover.
#[test]
fn pins_that_cannot_execute_fail_over() {
    let mut j = job(Some("English"), 5, "a.mp3");
    j.provider = Some(Provider::Runpod);
    assert_eq!(executable(&j, Provider::Runpod, &all()), Executable::Unavailable);
    assert_eq!(failover(&j, &[Provider::Runpod], &Preferences::default(), &all()), Some(Provider::Soniox));
    let mut t = job(Some("English"), 5, "a.mp3");
    t.translate_to = Language::from_name("German").into_iter().collect();
    t.provider = Some(Provider::Deepgram);
    assert_eq!(executable(&t, Provider::Deepgram, &all()), Executable::Incompatible);
    assert_eq!(failover(&t, &[Provider::Deepgram], &Preferences::default(), &all()), Some(Provider::Soniox));
    let off = Preferences { preferred: None, fallback_enabled: false };
    assert_eq!(failover(&j, &[Provider::Runpod], &off, &all()), None);
}

// TRX-009, LNG-014: translation never falls back to a non-translation provider.
#[test]
fn translation_requires_soniox() {
    let mut t = job(Some("English"), 5, "a.mp3");
    t.translate_to = Language::from_name("German").into_iter().collect();
    assert_eq!(failover(&t, &[Provider::Soniox], &Preferences::default(), &all()), None);
    let deepgram_only = Availability { configured: [Provider::Deepgram].into_iter().collect(), deepgram_inflight: 0 };
    assert_eq!(
        first(&t, &Preferences::default(), &deepgram_only),
        Route::Fail { code: "provider_unavailable", message: TRANSLATION_UNAVAILABLE.into() }
    );
}

// TRX-008 corrected: capacity only means wait; nothing capable configured means fail.
#[test]
fn last_resort_waits_or_fails_explicitly() {
    let full = Availability { deepgram_inflight: 4, ..all() };
    assert_eq!(first(&job(Some("Armenian"), 5, "a.mp3"), &Preferences::default(), &full), Route::Wait);
    let soniox_only = Availability { configured: [Provider::Soniox].into_iter().collect(), deepgram_inflight: 0 };
    assert_eq!(
        first(&job(Some("Armenian"), 5, "a.mp3"), &Preferences::default(), &soniox_only),
        Route::Fail { code: "provider_unavailable", message: ALL_PROVIDERS_FAILED.into() }
    );
}

// JOB-018: five slots minus one buffer; claimed and uncertain attempts count.
#[test]
fn deepgram_capacity_keeps_one_slot() {
    let mut a = all();
    for (inflight, free) in [(0, 4), (3, 1), (4, 0), (7, 0)] {
        a.deepgram_inflight = inflight;
        assert_eq!(a.deepgram_free_slots(), free);
    }
    a.deepgram_inflight = 4;
    assert_eq!(executable(&job(Some("Armenian"), 5, "a.mp3"), Provider::Deepgram, &a), Executable::NoCapacity);
}

// D11 and LNG-022: configured models, the per-account Deepgram exception as host data.
#[test]
fn models_come_from_configuration() {
    let special = crate::ids::AccountId(uuid::Uuid::from_u128(7));
    let config = ModelConfig {
        soniox: "stt-async-v5".into(),
        soniox_v5: "stt-async-v5".into(),
        deepgram: "whisper-large".into(),
        deepgram_account_overrides: vec![(special, "nova-2".into())],
    };
    assert_eq!(config.model_for(Provider::Soniox, fixtures::account()).as_deref(), Some("stt-async-v5"));
    assert_eq!(config.model_for(Provider::Deepgram, special).as_deref(), Some("nova-2"));
    assert_eq!(config.model_for(Provider::Deepgram, fixtures::account()).as_deref(), Some("whisper-large"));
    assert_eq!(config.model_for(Provider::Runpod, special), None);
}
