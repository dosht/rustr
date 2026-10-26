//! Provider minutes and the duration decision applied at result ingestion
//! (TRX-034, BIL-034..037, D6).
//!
//! Soniox keeps its observation origin: on the webhook path a positive
//! `audio_duration_ms` exactly as the webhook carried it, otherwise the largest
//! token end; on the recovery path the polled status observation, otherwise the
//! largest token end. A verification GET never fills the webhook value.

use media_ports::{ProviderObservation, ProviderPayload};

use crate::jobs::domain::{DeferredCharge, Job};
use crate::transcript::soniox::max_token_end_ms;

/// Whole provider minutes for this observation, or `None` when the provider
/// reported no usable length.
pub fn provider_minutes(observation: &ProviderObservation) -> Option<i64> {
    match &observation.payload {
        ProviderPayload::Soniox { reported, transcript } => {
            let tokens = transcript.as_ref().map(|t| t.tokens.as_slice()).unwrap_or(&[]);
            billing_core::charge::provider_minutes_soniox(reported.audio_duration_ms, max_token_end_ms(tokens))
        }
        ProviderPayload::Deepgram(result) => billing_core::charge::provider_minutes_deepgram(result.metadata.duration),
    }
}

/// What ingestion does with the job's duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationDecision {
    /// Nothing changes (equal or higher provider length on a known duration).
    Keep,
    /// The job started without a duration: set it and charge strictly (BIL-034).
    SetAndCharge { minutes: i64 },
    /// A shorter provider length lowers the stored minutes only; no refund (BIL-037).
    Lower { minutes: i64 },
    /// No usable provider length: no update and no charge (BIL-035).
    NoDuration,
}

pub fn decide(job: &Job, minutes: Option<i64>) -> DurationDecision {
    let Some(minutes) = minutes else { return DurationDecision::NoDuration };
    match job.duration_minutes {
        None => DurationDecision::SetAndCharge { minutes },
        Some(current) => match billing_core::charge::duration_after_provider(i64::from(current), minutes) {
            Some(lower) => DurationDecision::Lower { minutes: lower },
            None => DurationDecision::Keep,
        },
    }
}

/// The deferred charge state after a decision whose debit outcome is known. The
/// stored duration itself fences a second charge: once set, a replay sees a
/// known duration and only `Keep`/`Lower` remain.
pub fn deferred_after(job: &Job, decision: DurationDecision, charged: Option<bool>) -> DeferredCharge {
    match (decision, charged) {
        (DurationDecision::SetAndCharge { .. }, Some(true)) => DeferredCharge::Charged,
        (DurationDecision::SetAndCharge { .. }, _) => DeferredCharge::Refused,
        (DurationDecision::NoDuration, _) if job.deferred_charge == DeferredCharge::Pending => DeferredCharge::NoDuration,
        _ => job.deferred_charge,
    }
}

#[cfg(test)]
mod tests;
