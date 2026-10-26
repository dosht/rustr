//! The AI consent gate and the settings AI tab (AI-F01/F02, WEB-PRV-009/010,
//! PRV-038..044, AI-022/023). One per session: every AI surface reads the same
//! gate, as the source's single `AIConsentContext` did.
//!
//! The server answer is authoritative (D3): a grant is `POST {hasConsented:
//! true}`; the learn-more refusal is the nondestructive `POST false`, which
//! fences new work and keeps completed results. Settings "turn off AI and
//! delete saved AI data" is the revoking `DELETE` (P4 D3): refusal plus the
//! account-side purge, never proof of provider deletion, with a pending purge
//! reported as pending (D22). A failed write never shows consent as changed
//! and stays an inline action error (D21). Confirmation dialogs and the
//! intro's learn-more modal stay view mechanics.

use serde::{Deserialize, Serialize};

use crate::ai::ConsentView;
use crate::ids::Micros;
use crate::presentation::ai::ApiFailure;
use crate::presentation::msg::{Msg, app};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Enable,
    Decline,
    Disable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The session began (or the settings tab opened before any read).
    Opened,
    Loaded { result: Result<ConsentView, ApiFailure> },
    /// "Start Summarizing"/"Start Chatting" on the introduction, "I agree" in
    /// its modal, or "Enable AI Features" in settings.
    EnableRequested,
    /// Declining in the introduction's learn-more modal: `POST false`.
    DeclineRequested,
    /// The settings turn-off confirmation: the revoking, purging `DELETE`.
    DisableConfirmed,
    DecisionSaved { result: Result<ConsentView, ApiFailure> },
    ErrorDismissed,
    SessionEnded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// `GET /api/ai/consent`.
    LoadConsent,
    /// `POST /api/ai/consent {"hasConsented": granted}`: grant, or the
    /// nondestructive refusal that keeps completed results.
    RecordConsent { granted: bool },
    /// `DELETE /api/ai/consent`: refusal plus the account-side purge (P4 D3);
    /// provider deletion is not verified.
    RevokeConsent,
}

/// What the AI tabs render instead of (or as) their content (`AIFeatureWrapper`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Spinner until the first answer.
    Loading,
    /// The introduction with its consent call to action.
    Required,
    Granted,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConsentPanelState {
    consent: Option<ConsentView>,
    loading: bool,
    answered: bool,
    busy: Option<Decision>,
    /// The consent READ failure (D21: distinct from a failed write).
    error: Option<Msg>,
    /// A failed write, shown inline beside the control that issued it.
    action_error: Option<Msg>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConsentPanelVm {
    pub gate: Gate,
    pub has_consented: bool,
    /// "Turn off AI and delete saved AI data" is offered for a valid grant only.
    pub can_revoke: bool,
    /// D3/D22: a DELETE whose account-side purge has not completed yet.
    pub cleanup_pending: bool,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub consent_date: Option<Micros>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub revoked_date: Option<Micros>,
    pub provider_name: Option<String>,
    pub consent_version: Option<String>,
    pub privacy_policy_url: String,
    pub busy: Option<Decision>,
    /// The consent read failed: settings renders it in place of the tab.
    pub load_error: Option<Msg>,
    /// A failed enable, decline or turn-off (D21): inline, controls kept.
    pub action_error: Option<Msg>,
}

/// The source fallback when no record names a policy page.
pub const DEFAULT_PRIVACY_POLICY_URL: &str = "/legal/privacy-policy-en";

impl ConsentPanelState {
    pub fn granted(&self) -> bool {
        self.consent.as_ref().is_some_and(|c| c.has_consented)
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Opened => {
                if self.loading || self.answered {
                    return vec![];
                }
                self.loading = true;
                vec![Effect::LoadConsent]
            }
            Event::Loaded { result } => {
                self.loading = false;
                self.answered = true;
                match result {
                    Ok(view) => {
                        self.consent = Some(view);
                        self.error = None;
                    }
                    Err(_) => self.error = Some(app("Failed to load consent preferences")),
                }
                vec![]
            }
            Event::EnableRequested => self.decide(Decision::Enable),
            Event::DeclineRequested => self.decide(Decision::Decline),
            Event::DisableConfirmed => {
                if !self.consent.as_ref().is_some_and(|c| c.can_revoke) {
                    return vec![];
                }
                self.decide(Decision::Disable)
            }
            Event::DecisionSaved { result } => {
                let Some(decision) = self.busy.take() else { return vec![] };
                match result {
                    Ok(view) => {
                        self.consent = Some(view);
                        self.answered = true;
                        self.error = None;
                        self.action_error = None;
                    }
                    // D21: a failed write keeps the controls and the last
                    // server-confirmed state; the error is the action's own.
                    Err(_) => {
                        self.action_error = Some(app(match decision {
                            Decision::Enable => "Failed to enable AI features. Please try again.",
                            Decision::Disable => "Failed to disable AI features. Please try again.",
                            Decision::Decline => "Failed to record consent decision",
                        }));
                    }
                }
                vec![]
            }
            Event::ErrorDismissed => {
                self.error = None;
                self.action_error = None;
                vec![]
            }
            Event::SessionEnded => {
                *self = ConsentPanelState::default();
                vec![]
            }
        }
    }

    fn decide(&mut self, decision: Decision) -> Vec<Effect> {
        if self.busy.is_some() {
            return vec![];
        }
        self.busy = Some(decision);
        self.error = None;
        self.action_error = None;
        vec![match decision {
            Decision::Enable => Effect::RecordConsent { granted: true },
            Decision::Decline => Effect::RecordConsent { granted: false },
            Decision::Disable => Effect::RevokeConsent,
        }]
    }

    pub fn view(&self) -> ConsentPanelVm {
        let c = self.consent.as_ref();
        let gate = if !self.answered && c.is_none() {
            Gate::Loading
        } else if self.granted() {
            Gate::Granted
        } else {
            Gate::Required
        };
        ConsentPanelVm {
            gate,
            has_consented: self.granted(),
            can_revoke: c.is_some_and(|c| c.can_revoke),
            cleanup_pending: c.is_some_and(|c| c.cleanup_pending),
            consent_date: c.and_then(|c| c.consent_date),
            revoked_date: c.and_then(|c| c.revoked_date),
            provider_name: c.map(|c| c.provider_name.clone()),
            consent_version: c.map(|c| c.consent_version.clone()),
            privacy_policy_url: c
                .map(|c| c.privacy_policy_url.as_str())
                .filter(|u| !u.is_empty())
                .unwrap_or(DEFAULT_PRIVACY_POLICY_URL)
                .to_string(),
            busy: self.busy,
            load_error: self.error.clone(),
            action_error: self.action_error.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
