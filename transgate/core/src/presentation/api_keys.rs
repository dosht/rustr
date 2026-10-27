//! The API key screen (WEB-045 desktop, WEB-046 phone; `dashboard/api-keys.tsx`),
//! one state machine for both layouts. Create with a name, a scope choice and an
//! expiry policy; the new key is shown once with a copy action; the list shows
//! scopes, created time, expiry and status; revoke asks first (API-019). The
//! account's callback signing secret is shown to its owner (P7 D5, lead
//! 2026-10-27). Everything the views show is decided here.

use serde::{Deserialize, Serialize};

use crate::api::KeyId;
use crate::api::keys::{CreatedKey, ExpirationPolicy, KeyView, NewKey};
use crate::api::Scope;
use crate::ids::{MICROS_PER_DAY, Micros};
use crate::presentation::failure::ApiFailure;
use crate::presentation::job_list::Tone;
use crate::presentation::msg::{Msg, app, common};

/// The permission choices in the source's order (`api-keys.tsx:50-69`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScopeChoice {
    Read,
    Create,
    /// The source's default.
    #[default]
    ReadAndCreate,
}

impl ScopeChoice {
    pub const ALL: [ScopeChoice; 3] = [ScopeChoice::Read, ScopeChoice::Create, ScopeChoice::ReadAndCreate];

    pub fn scopes(self) -> Vec<Scope> {
        match self {
            ScopeChoice::Read => vec![Scope::TranscriptionsRead],
            ScopeChoice::Create => vec![Scope::TranscriptionsCreate],
            ScopeChoice::ReadAndCreate => vec![Scope::TranscriptionsRead, Scope::TranscriptionsCreate],
        }
    }

    pub fn label(self) -> Msg {
        common(match self {
            ScopeChoice::Read => "Read Only",
            ScopeChoice::Create => "Create Only",
            ScopeChoice::ReadAndCreate => "Read & Create",
        })
    }
}

/// The policy choices in the source's fallback order (`api-keys.tsx:120-126`).
pub const POLICIES: [ExpirationPolicy; 5] =
    [ExpirationPolicy::Never, ExpirationPolicy::Days30, ExpirationPolicy::Days90, ExpirationPolicy::OneYear, ExpirationPolicy::Custom];
/// The custom day input's range (`api-keys.tsx:430`, "1-3650").
pub const CUSTOM_DAYS: std::ops::RangeInclusive<i32> = 1..=3650;
/// The source's custom default.
pub const DEFAULT_CUSTOM_DAYS: i32 = 30;
/// Expiry within this many days shows the warning tone (`api-keys.tsx:263`).
pub const EXPIRY_WARNING_DAYS: i64 = 7;

pub fn policy_label(policy: ExpirationPolicy) -> Msg {
    match policy {
        ExpirationPolicy::Never => common("Never Expire"),
        ExpirationPolicy::Days30 => common("30 Days"),
        ExpirationPolicy::Days90 => common("90 Days"),
        // The source passed `t('1 Year')`, but that common key reads as the annual
        // plan in some catalogs (pt "Plano Anual"); the expiry gets its own key.
        ExpirationPolicy::OneYear => EXPIRY_ONE_YEAR,
        ExpirationPolicy::Custom => common("Custom"),
    }
}

/// "1 Year" as a key expiry choice (one year after creation).
pub const EXPIRY_ONE_YEAR: Msg = app("p7.keys.expiry_one_year");

pub const LOAD_FAILED: Msg = app("p7.keys.load_failed");
pub const SECRET_FAILED: Msg = app("p7.keys.secret_load_failed");
/// The signing-secret section (P7 D5). The source had no such section, so its
/// copy is new `app` text (D10); English meanings in `p7-api/implementation/core.md`.
pub const SECRET_TITLE: Msg = app("p7.keys.secret_title");
pub const SECRET_DESCRIPTION: Msg = app("p7.keys.secret_description");
pub const SECRET_COPY: Msg = app("p7.keys.secret_copy");
pub const SECRET_COPIED: Msg = app("p7.keys.secret_copied");
pub const SECRET_COPY_FAILED: Msg = app("p7.keys.secret_copy_failed");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Copied {
    Key,
    Secret,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Opened,
    KeysLoaded { result: Result<Vec<KeyView>, ApiFailure> },
    SecretLoaded { result: Result<String, ApiFailure> },
    /// "Generate New Key" / "Cancel".
    FormToggled,
    NameChanged { text: String },
    ScopeChosen { scope: ScopeChoice },
    PolicyChosen { policy: ExpirationPolicy },
    CustomDaysChanged { text: String },
    CreateRequested,
    Created { result: Result<CreatedKey, ApiFailure> },
    CopyRequested { what: Copied },
    CopyAnswered { what: Copied, ok: bool },
    RevokeRequested { id: KeyId },
    RevokeConfirmed,
    RevokeCanceled,
    Revoked { id: KeyId, result: Result<KeyView, ApiFailure> },
    NoticeDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// `GET /api/keys`.
    LoadKeys,
    /// `GET /api/keys/signing-secret` -> `{secret}`.
    LoadSigningSecret,
    /// `POST /api/keys`.
    CreateKey { key: NewKey },
    /// `DELETE /api/keys/{id}`.
    RevokeKey { id: KeyId },
    CopyText { text: String, what: Copied },
}

/// What the key screen's views send; results arrive as [`Event`]s from the driver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ApiKeysIntent"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Intent {
    FormToggled,
    NameChanged { text: String },
    ScopeChosen { scope: ScopeChoice },
    PolicyChosen { policy: ExpirationPolicy },
    CustomDaysChanged { text: String },
    CreateRequested,
    CopyRequested { what: Copied },
    RevokeRequested { #[cfg_attr(feature = "ts", ts(type = "string"))] id: KeyId },
    RevokeConfirmed,
    RevokeCanceled,
    NoticeDismissed,
}

impl From<Intent> for Event {
    fn from(intent: Intent) -> Event {
        match intent {
            Intent::FormToggled => Event::FormToggled,
            Intent::NameChanged { text } => Event::NameChanged { text },
            Intent::ScopeChosen { scope } => Event::ScopeChosen { scope },
            Intent::PolicyChosen { policy } => Event::PolicyChosen { policy },
            Intent::CustomDaysChanged { text } => Event::CustomDaysChanged { text },
            Intent::CreateRequested => Event::CreateRequested,
            Intent::CopyRequested { what } => Event::CopyRequested { what },
            Intent::RevokeRequested { id } => Event::RevokeRequested { id },
            Intent::RevokeConfirmed => Event::RevokeConfirmed,
            Intent::RevokeCanceled => Event::RevokeCanceled,
            Intent::NoticeDismissed => Event::NoticeDismissed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiKeysState {
    keys: Vec<KeyView>,
    loading: bool,
    load_failed: bool,
    secret: Option<String>,
    secret_failed: bool,
    form_open: bool,
    name: String,
    scope: ScopeChoice,
    policy: ExpirationPolicy,
    custom_days: String,
    creating: bool,
    created: Option<String>,
    copied: Option<Copied>,
    confirm_revoke: Option<KeyId>,
    revoking: Option<KeyId>,
    notice: Option<Msg>,
}

impl Default for ApiKeysState {
    fn default() -> Self {
        ApiKeysState {
            keys: vec![],
            loading: false,
            load_failed: false,
            secret: None,
            secret_failed: false,
            form_open: false,
            name: String::new(),
            scope: ScopeChoice::default(),
            policy: ExpirationPolicy::Never,
            custom_days: DEFAULT_CUSTOM_DAYS.to_string(),
            creating: false,
            created: None,
            copied: None,
            confirm_revoke: None,
            revoking: None,
            notice: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeLabel {
    Known { label: Msg },
    /// A stored string the source did not offer (migrated rows), shown as is.
    Raw { value: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpiryLabel {
    Never,
    Expired,
    /// `{days} {Days}`.
    Days { days: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct KeyRowVm {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub id: KeyId,
    pub name: String,
    /// Empty shows "No Permissions".
    pub scopes: Vec<ScopeLabel>,
    /// Shown with the platform's `toLocaleString`.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at_ms: i64,
    pub expiry: ExpiryLabel,
    pub expiry_tone: Tone,
    pub active: bool,
    pub status: Msg,
    /// The status badge (`api-keys.tsx:495-500`): success when active, danger when revoked.
    pub status_tone: Tone,
    pub can_revoke: bool,
    pub revoking: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct OptionVm<T> {
    pub value: T,
    pub label: Msg,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct KeyFormVm {
    pub name: String,
    pub scopes: Vec<OptionVm<ScopeChoice>>,
    pub policies: Vec<OptionVm<ExpirationPolicy>>,
    /// The custom day input, present only for the custom policy.
    pub custom_days: Option<String>,
    pub custom_days_invalid: bool,
    pub can_create: bool,
    pub creating: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ApiKeysVm {
    pub loading: bool,
    pub error: Option<Msg>,
    pub rows: Vec<KeyRowVm>,
    /// "No API keys found."
    pub empty: bool,
    /// "Generate New Key" or "Cancel".
    pub form_toggle: Msg,
    pub form: Option<KeyFormVm>,
    /// The new key, shown once until the next create (API-002).
    pub created_key: Option<String>,
    pub key_copied: bool,
    /// The revoke confirmation ("Are you sure you want to revoke this API key?").
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub confirm_revoke: Option<KeyId>,
    /// The signing-secret section: heading, what the secret is for, copy action.
    pub secret_title: Msg,
    pub secret_description: Msg,
    pub secret_copy_label: Msg,
    pub signing_secret: Option<String>,
    /// After a successful copy the notice says `p7.keys.secret_copied`.
    pub secret_copied: bool,
    pub secret_error: Option<Msg>,
    pub notice: Option<Msg>,
}

/// `getExpirationStatus` (`api-keys.tsx:248-268`): no expiry is "Never"
/// (neutral); before now is "Expired" (danger); else the whole days left,
/// rounded up, warning within seven days, success otherwise.
pub fn expiry(expires_at: Option<Micros>, now: Micros) -> (ExpiryLabel, Tone) {
    let Some(at) = expires_at else { return (ExpiryLabel::Never, Tone::Neutral) };
    if at < now {
        return (ExpiryLabel::Expired, Tone::Danger);
    }
    let days = (at.0 - now.0 + MICROS_PER_DAY - 1).div_euclid(MICROS_PER_DAY);
    (ExpiryLabel::Days { days }, if days <= EXPIRY_WARNING_DAYS { Tone::Progress } else { Tone::Success })
}

fn scope_label(scope: &str) -> ScopeLabel {
    match scope {
        "transcriptions:read" => ScopeLabel::Known { label: ScopeChoice::Read.label() },
        "transcriptions:create" => ScopeLabel::Known { label: ScopeChoice::Create.label() },
        other => ScopeLabel::Raw { value: other.to_string() },
    }
}

impl ApiKeysState {
    fn custom_days(&self) -> Option<i32> {
        self.custom_days.trim().parse::<i32>().ok().filter(|d| CUSTOM_DAYS.contains(d))
    }

    fn can_create(&self) -> bool {
        !self.creating && !self.name.trim().is_empty() && (self.policy != ExpirationPolicy::Custom || self.custom_days().is_some())
    }

    fn reset_form(&mut self) {
        let defaults = ApiKeysState::default();
        self.name = defaults.name;
        self.scope = defaults.scope;
        self.policy = defaults.policy;
        self.custom_days = defaults.custom_days;
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Opened => {
                self.loading = true;
                vec![Effect::LoadKeys, Effect::LoadSigningSecret]
            }
            Event::KeysLoaded { result } => {
                self.loading = false;
                match result {
                    Ok(keys) => {
                        self.keys = keys;
                        self.load_failed = false;
                    }
                    Err(_) => self.load_failed = true,
                }
                vec![]
            }
            Event::SecretLoaded { result } => {
                match result {
                    Ok(secret) => {
                        self.secret = Some(secret);
                        self.secret_failed = false;
                    }
                    Err(_) => self.secret_failed = true,
                }
                vec![]
            }
            Event::FormToggled => {
                self.form_open = !self.form_open;
                vec![]
            }
            Event::NameChanged { text } => {
                self.name = text;
                vec![]
            }
            Event::ScopeChosen { scope } => {
                self.scope = scope;
                vec![]
            }
            Event::PolicyChosen { policy } => {
                self.policy = policy;
                vec![]
            }
            Event::CustomDaysChanged { text } => {
                self.custom_days = text;
                vec![]
            }
            Event::CreateRequested => {
                if !self.form_open || !self.can_create() {
                    return vec![];
                }
                self.creating = true;
                self.created = None;
                self.copied = None;
                self.notice = None;
                let custom_days = (self.policy == ExpirationPolicy::Custom).then(|| self.custom_days()).flatten();
                let key = NewKey { name: self.name.trim().to_string(), scopes: self.scope.scopes(), expiration_policy: self.policy, custom_days };
                vec![Effect::CreateKey { key }]
            }
            Event::Created { result } => {
                if !self.creating {
                    return vec![];
                }
                self.creating = false;
                match result {
                    Ok(created) => {
                        self.created = Some(created.api_key);
                        self.form_open = false;
                        self.reset_form();
                        self.notice = Some(common("API key created successfully"));
                        vec![Effect::LoadKeys]
                    }
                    Err(_) => {
                        self.notice = Some(common("Failed to create API key"));
                        vec![]
                    }
                }
            }
            Event::CopyRequested { what } => {
                let text = match what {
                    Copied::Key => self.created.clone(),
                    Copied::Secret => self.secret.clone(),
                };
                text.map(|text| Effect::CopyText { text, what }).into_iter().collect()
            }
            Event::CopyAnswered { what, ok } => {
                self.copied = ok.then_some(what);
                self.notice = Some(match (what, ok) {
                    (Copied::Key, true) => common("API key copied to clipboard"),
                    (Copied::Key, false) => common("Failed to copy API key"),
                    (Copied::Secret, true) => SECRET_COPIED,
                    (Copied::Secret, false) => SECRET_COPY_FAILED,
                });
                vec![]
            }
            Event::RevokeRequested { id } => {
                if self.revoking.is_none() && self.keys.iter().any(|k| k.id == id && k.active) {
                    self.confirm_revoke = Some(id);
                }
                vec![]
            }
            Event::RevokeCanceled => {
                self.confirm_revoke = None;
                vec![]
            }
            Event::RevokeConfirmed => match self.confirm_revoke.take() {
                Some(id) if self.revoking.is_none() => {
                    self.revoking = Some(id);
                    vec![Effect::RevokeKey { id }]
                }
                _ => vec![],
            },
            Event::Revoked { id, result } => {
                if self.revoking != Some(id) {
                    return vec![];
                }
                self.revoking = None;
                match result {
                    Ok(view) => {
                        if let Some(row) = self.keys.iter_mut().find(|k| k.id == id) {
                            *row = view;
                        }
                        self.notice = Some(common("API key revoked successfully"));
                        vec![Effect::LoadKeys]
                    }
                    Err(_) => {
                        self.notice = Some(common("Failed to revoke API key"));
                        vec![]
                    }
                }
            }
            Event::NoticeDismissed => {
                self.notice = None;
                vec![]
            }
        }
    }

    pub fn view(&self, now: Micros) -> ApiKeysVm {
        let rows = self
            .keys
            .iter()
            .map(|k| {
                let (expiry, expiry_tone) = expiry(k.expires_at, now);
                KeyRowVm {
                    id: k.id,
                    name: k.name.clone(),
                    scopes: k.scopes.iter().map(|s| scope_label(s)).collect(),
                    created_at_ms: k.created_at.0.div_euclid(1000),
                    expiry,
                    expiry_tone,
                    active: k.active,
                    status: common(if k.active { "Active" } else { "Revoked" }),
                    status_tone: if k.active { Tone::Success } else { Tone::Danger },
                    can_revoke: k.active && self.revoking.is_none(),
                    revoking: self.revoking == Some(k.id),
                }
            })
            .collect::<Vec<_>>();
        let form = self.form_open.then(|| KeyFormVm {
            name: self.name.clone(),
            scopes: ScopeChoice::ALL.iter().map(|&value| OptionVm { value, label: value.label(), selected: value == self.scope }).collect(),
            policies: POLICIES.iter().map(|&value| OptionVm { value, label: policy_label(value), selected: value == self.policy }).collect(),
            custom_days: (self.policy == ExpirationPolicy::Custom).then(|| self.custom_days.clone()),
            custom_days_invalid: self.policy == ExpirationPolicy::Custom && self.custom_days().is_none(),
            can_create: self.can_create(),
            creating: self.creating,
        });
        ApiKeysVm {
            loading: self.loading,
            error: self.load_failed.then_some(LOAD_FAILED),
            // The source shows "No API keys found." for `keys == []` even after a failed load.
            empty: !self.loading && rows.is_empty(),
            rows,
            form_toggle: common(if self.form_open { "Cancel" } else { "Generate New Key" }),
            form,
            created_key: self.created.clone(),
            key_copied: self.copied == Some(Copied::Key),
            confirm_revoke: self.confirm_revoke,
            secret_title: SECRET_TITLE,
            secret_description: SECRET_DESCRIPTION,
            secret_copy_label: SECRET_COPY,
            signing_secret: self.secret.clone(),
            secret_copied: self.copied == Some(Copied::Secret),
            secret_error: self.secret_failed.then_some(SECRET_FAILED),
            notice: self.notice.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
