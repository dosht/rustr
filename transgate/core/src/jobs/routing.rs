//! One routing rule for every pipeline (PRD section 3): language and model
//! support, then duration, with the source's order of overrides.
//!
//! Source: `transcription.ts:505-558` (a stored provider is used as-is, then
//! `handleFailover` runs when the submit fails) and `provider-manager.ts:226-407,
//! 516-594`. Health scoring is retired (PRD section 2), so validity is
//! capability plus availability. Recorded corrections: a capability-incompatible
//! pin counts as a failed first attempt (then permitted fallback), and the
//! unconditional "Soniox anyway" last resort becomes a capacity wait or an
//! explicit compatibility failure.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::jobs::domain::{Job, Provider};

/// The source fallback chain (`soniox, deepgram, runpod`). RunPod keeps its slot
/// but is never available: it has no transport.
pub const FALLBACK_CHAIN: [Provider; 3] = [Provider::Soniox, Provider::Deepgram, Provider::Runpod];

/// Deepgram's provider limit and the slot the queue keeps free (JOB-018).
pub const DEEPGRAM_CONCURRENT_LIMIT: u32 = 5;
pub const DEEPGRAM_SLOT_BUFFER: u32 = 1;

/// Per-request routing preferences (API `options`, TRX-004/014).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub preferred: Option<Provider>,
    /// `options.fallbackEnabled !== false`.
    pub fallback_enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences { preferred: None, fallback_enabled: true }
    }
}

/// What the dispatcher knows at claim time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Availability {
    /// Providers with a configured transport (API key and project).
    pub configured: BTreeSet<Provider>,
    /// Claimed, uncertain and running Deepgram work (capacity accounting, C-5).
    pub deepgram_inflight: u32,
}

impl Availability {
    pub fn deepgram_free_slots(&self) -> u32 {
        (DEEPGRAM_CONCURRENT_LIMIT - DEEPGRAM_SLOT_BUFFER).saturating_sub(self.deepgram_inflight)
    }

    /// `isAvailable`: configured, and for Deepgram a free slot (TRX-012).
    pub fn is_available(&self, provider: Provider) -> bool {
        if provider.transport().is_none() || !self.configured.contains(&provider) {
            return false;
        }
        match provider {
            Provider::Deepgram => self.deepgram_free_slots() > 0,
            _ => true,
        }
    }

    /// Configured, ignoring Deepgram capacity.
    pub fn is_configured(&self, provider: Provider) -> bool {
        provider.transport().is_some() && self.configured.contains(&provider)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteReason {
    StoredProvider,
    DurationOverride,
    QtaOverride,
    Preferred,
    Soniox,
    FallbackChain,
    LastResort,
}

/// The first provider to try and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Submit { provider: Provider, reason: RouteReason },
    /// Only capacity (or nothing configured yet but a capable provider exists)
    /// blocks the job: it waits as pending.
    Wait,
    /// No configured provider can ever take this job.
    Fail { code: &'static str, message: String },
}

pub const TRANSLATION_UNAVAILABLE: &str = "Translation jobs require Soniox, but Soniox is unavailable";
pub const ALL_PROVIDERS_FAILED: &str = "All providers failed, no fallback available";

/// `canHandle` plus translation capability (`validateProviderChoice` minus health).
pub fn compatible(job: &Job, provider: Provider) -> bool {
    provider.can_handle(job.language) && (!job.is_translation() || provider.supports_translation())
}

fn valid(job: &Job, provider: Provider, availability: &Availability) -> bool {
    compatible(job, provider) && availability.is_available(provider)
}

/// The non-translation duration threshold (`SONIOX_MAX_DURATION_MINUTES`).
/// Required configuration: production observed 200, the code default is 250.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutingConfig {
    pub soniox_max_duration_minutes: i32,
}

/// First choice for a job (`processJobsWithProvider` layer 1, `selectProvider` layer 2).
pub fn first_choice(job: &Job, prefs: &Preferences, availability: &Availability, config: &RoutingConfig) -> Route {
    // Layer 1: a stored provider is the first attempt, as-is (TRX-001). Whether it
    // can execute is decided by the dispatcher; a non-executable or incompatible
    // pin becomes a failed first attempt followed by `failover`.
    if let Some(pin) = job.provider {
        return Route::Submit { provider: pin, reason: RouteReason::StoredProvider };
    }
    let translation = job.is_translation();
    // TRX-002: long non-translation audio goes to Deepgram with no availability check.
    if !translation && job.duration_minutes.is_some_and(|d| d > config.soniox_max_duration_minutes) {
        return Route::Submit { provider: Provider::Deepgram, reason: RouteReason::DurationOverride };
    }
    // TRX-003: Soniox cannot decode `.qta`.
    if !translation && job.file_extension().as_deref() == Some("qta") {
        return Route::Submit { provider: Provider::Deepgram, reason: RouteReason::QtaOverride };
    }
    // TRX-004
    if let Some(preferred) = prefs.preferred && valid(job, preferred, availability) {
        return Route::Submit { provider: preferred, reason: RouteReason::Preferred };
    }
    // TRX-005/006: the language map and the client type both prefer Soniox (LNG-D06).
    if valid(job, Provider::Soniox, availability) {
        return Route::Submit { provider: Provider::Soniox, reason: RouteReason::Soniox };
    }
    // TRX-007 without health sorting (LNG-D05), translation keeps capable providers only.
    if let Some(p) = FALLBACK_CHAIN
        .into_iter()
        .filter(|p| !translation || p.supports_translation())
        .find(|p| valid(job, *p, availability))
    {
        return Route::Submit { provider: p, reason: RouteReason::FallbackChain };
    }
    last_resort(job, availability)
}

/// TRX-008/009 corrected: a capable provider that is only out of capacity or
/// not yet configured means wait; nothing capable at all means fail.
fn last_resort(job: &Job, availability: &Availability) -> Route {
    let capable: Vec<Provider> = FALLBACK_CHAIN
        .into_iter()
        .filter(|p| p.transport().is_some() && compatible(job, *p))
        .collect();
    if capable.iter().any(|p| availability.is_configured(*p)) {
        return Route::Wait;
    }
    if job.is_translation() {
        Route::Fail { code: "provider_unavailable", message: TRANSLATION_UNAVAILABLE.into() }
    } else {
        Route::Fail { code: "provider_unavailable", message: ALL_PROVIDERS_FAILED.into() }
    }
}

/// What to do after an attempt with `tried` providers failed or was rate limited
/// (`handleFailover`): the next chain member that is compatible and available.
pub fn failover(job: &Job, tried: &[Provider], prefs: &Preferences, availability: &Availability) -> Option<Provider> {
    if !prefs.fallback_enabled {
        return None;
    }
    FALLBACK_CHAIN
        .into_iter()
        .filter(|p| !tried.contains(p))
        .filter(|p| !job.is_translation() || p.supports_translation())
        .find(|p| valid(job, *p, availability))
}

/// Whether the dispatcher may actually submit to `provider` now. A pin is
/// executed as-is; only a missing transport, missing configuration, Deepgram
/// capacity or (recorded correction) an incompatible pin stops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Executable {
    Yes,
    /// RunPod or an unconfigured provider: an unavailable attempt, then failover.
    Unavailable,
    /// Deepgram has no free slot: wait without submitting (no billed 429).
    NoCapacity,
    /// Translation pinned to a provider that cannot translate, or auto-detect
    /// pinned to Deepgram: a failed first attempt, then failover.
    Incompatible,
}

pub fn executable(job: &Job, provider: Provider, availability: &Availability) -> Executable {
    if !availability.is_configured(provider) {
        return Executable::Unavailable;
    }
    if !compatible(job, provider) {
        return Executable::Incompatible;
    }
    if provider == Provider::Deepgram && availability.deepgram_free_slots() == 0 {
        return Executable::NoCapacity;
    }
    Executable::Yes
}

/// The model sent for a product provider (D11; LNG-021/022). Deepgram keeps the
/// source's per-account Nova-2 exception as trusted host configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelConfig {
    /// `stt-async-v5` by default (D11).
    pub soniox: String,
    /// The stored `soniox_v5` pin's model.
    pub soniox_v5: String,
    pub deepgram: String,
    /// Accounts that get the alternate Deepgram model, and that model.
    pub deepgram_account_overrides: Vec<(crate::ids::AccountId, String)>,
}

impl ModelConfig {
    pub fn model_for(&self, provider: Provider, account: crate::ids::AccountId) -> Option<String> {
        match provider {
            Provider::Soniox => Some(self.soniox.clone()),
            Provider::SonioxV5 => Some(self.soniox_v5.clone()),
            Provider::Deepgram => Some(
                self.deepgram_account_overrides
                    .iter()
                    .find(|(a, _)| *a == account)
                    .map(|(_, m)| m.clone())
                    .unwrap_or_else(|| self.deepgram.clone()),
            ),
            Provider::Runpod => None,
        }
    }
}

#[cfg(test)]
mod tests;
