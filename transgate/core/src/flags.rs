//! Feature flags (FLG-001..009). The source evaluated a static table in the
//! browser and shipped its allow-lists (emails and user ids) to every visitor.
//! Here the host holds the table as configuration and sends only the effective
//! [`Capabilities`] of the verified caller (P5 D7). The voice demo flag is not
//! carried: the demo is retired (P4 D6, P5 D14).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Feature {
    AiFeatures,
    TranscriptionSourceMicrophone,
    TranscriptionSourceYoutube,
    TranscriptionSourceGoogleDrive,
    TranscriptionSourceOnedrive,
    ShowOriginalTranscript,
    MobileUxJobs,
    MobileExpandedAudioPlayer,
    MobileUxAi,
    BillingHistory,
    BillingCustomerLookup,
}

impl Feature {
    pub const ALL: [Feature; 11] = [
        Feature::AiFeatures,
        Feature::TranscriptionSourceMicrophone,
        Feature::TranscriptionSourceYoutube,
        Feature::TranscriptionSourceGoogleDrive,
        Feature::TranscriptionSourceOnedrive,
        Feature::ShowOriginalTranscript,
        Feature::MobileUxJobs,
        Feature::MobileExpandedAudioPlayer,
        Feature::MobileUxAi,
        Feature::BillingHistory,
        Feature::BillingCustomerLookup,
    ];
}

/// One flag: a global switch, else any matching email, user id or locale.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagRule {
    pub enabled: bool,
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    #[serde(default)]
    pub allowed_user_ids: Vec<String>,
    #[serde(default)]
    pub allowed_locales: Vec<String>,
}

impl FlagRule {
    pub const fn on() -> FlagRule {
        FlagRule { enabled: true, allowed_emails: Vec::new(), allowed_user_ids: Vec::new(), allowed_locales: Vec::new() }
    }

    pub const fn off() -> FlagRule {
        FlagRule { enabled: false, allowed_emails: Vec::new(), allowed_user_ids: Vec::new(), allowed_locales: Vec::new() }
    }

    /// FLG-001 (`checkFeatureFlag`): exact, case-sensitive matches.
    pub fn enabled_for(&self, subject: &Subject<'_>) -> bool {
        let listed = |list: &[String], value: Option<&str>| value.is_some_and(|v| list.iter().any(|x| x == v));
        self.enabled
            || listed(&self.allowed_emails, subject.email)
            || listed(&self.allowed_user_ids, subject.user_id)
            || listed(&self.allowed_locales, subject.locale)
    }
}

/// Who a flag is evaluated for: the verified caller and the page locale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subject<'a> {
    pub email: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub locale: Option<&'a str>,
}

/// The host's flag table. Allow-lists are deployment configuration, never code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagConfig {
    pub ai_features: FlagRule,
    pub transcription_source_microphone: FlagRule,
    pub transcription_source_youtube: FlagRule,
    pub transcription_source_google_drive: FlagRule,
    pub transcription_source_onedrive: FlagRule,
    pub show_original_transcript: FlagRule,
    pub mobile_ux_jobs: FlagRule,
    pub mobile_expanded_audio_player: FlagRule,
    pub mobile_ux_ai: FlagRule,
    pub billing_history: FlagRule,
    pub billing_customer_lookup: FlagRule,
}

impl Default for FlagConfig {
    /// The source's global switches (FLG-003/004/005) with empty allow-lists.
    fn default() -> FlagConfig {
        FlagConfig {
            ai_features: FlagRule::on(),
            transcription_source_microphone: FlagRule::off(),
            transcription_source_youtube: FlagRule::on(),
            transcription_source_google_drive: FlagRule::off(),
            transcription_source_onedrive: FlagRule::off(),
            show_original_transcript: FlagRule::off(),
            mobile_ux_jobs: FlagRule::on(),
            mobile_expanded_audio_player: FlagRule::off(),
            mobile_ux_ai: FlagRule::on(),
            billing_history: FlagRule::on(),
            billing_customer_lookup: FlagRule::off(),
        }
    }
}

impl FlagConfig {
    pub fn rule(&self, feature: Feature) -> &FlagRule {
        match feature {
            Feature::AiFeatures => &self.ai_features,
            Feature::TranscriptionSourceMicrophone => &self.transcription_source_microphone,
            Feature::TranscriptionSourceYoutube => &self.transcription_source_youtube,
            Feature::TranscriptionSourceGoogleDrive => &self.transcription_source_google_drive,
            Feature::TranscriptionSourceOnedrive => &self.transcription_source_onedrive,
            Feature::ShowOriginalTranscript => &self.show_original_transcript,
            Feature::MobileUxJobs => &self.mobile_ux_jobs,
            Feature::MobileExpandedAudioPlayer => &self.mobile_expanded_audio_player,
            Feature::MobileUxAi => &self.mobile_ux_ai,
            Feature::BillingHistory => &self.billing_history,
            Feature::BillingCustomerLookup => &self.billing_customer_lookup,
        }
    }

    pub fn enabled(&self, feature: Feature, subject: &Subject<'_>) -> bool {
        self.rule(feature).enabled_for(subject)
    }

    /// The verified caller's effective capabilities. `ai_chat` is the AI host's
    /// own switch (FLG-009: chat only on exact true), not a flag-table row.
    pub fn capabilities(&self, subject: &Subject<'_>, ai_chat: bool) -> Capabilities {
        let on = |f| self.enabled(f, subject);
        let ai = on(Feature::AiFeatures);
        Capabilities {
            ai,
            ai_chat: ai && ai_chat,
            youtube: on(Feature::TranscriptionSourceYoutube),
            microphone: on(Feature::TranscriptionSourceMicrophone),
            google_drive: on(Feature::TranscriptionSourceGoogleDrive),
            onedrive: on(Feature::TranscriptionSourceOnedrive),
            show_original: on(Feature::ShowOriginalTranscript),
            mobile_ux_jobs: on(Feature::MobileUxJobs),
            mobile_expanded_player: on(Feature::MobileExpandedAudioPlayer),
            mobile_ux_ai: on(Feature::MobileUxAi),
            billing_history: on(Feature::BillingHistory),
            customer_lookup: on(Feature::BillingCustomerLookup),
        }
    }
}

/// What the signed-in shell may show. FLG-002: a shell without a verified
/// caller has none of these (`Capabilities::default()`, every field false).
/// Server routes keep enforcing their own checks (FLG-007 customer lookup).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Capabilities {
    pub ai: bool,
    pub ai_chat: bool,
    pub youtube: bool,
    pub microphone: bool,
    pub google_drive: bool,
    pub onedrive: bool,
    pub show_original: bool,
    pub mobile_ux_jobs: bool,
    pub mobile_expanded_player: bool,
    pub mobile_ux_ai: bool,
    pub billing_history: bool,
    pub customer_lookup: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject<'a>(email: &'a str, user: &'a str, locale: &'a str) -> Subject<'a> {
        Subject { email: Some(email), user_id: Some(user), locale: Some(locale) }
    }

    #[test]
    fn a_global_switch_enables_everyone_and_otherwise_any_listed_identity_does() {
        let rule = FlagRule {
            enabled: false,
            allowed_emails: vec!["ada@example.test".into()],
            allowed_user_ids: vec!["u-7".into()],
            allowed_locales: vec!["id".into()],
        };
        assert!(rule.enabled_for(&subject("ada@example.test", "u-1", "en")));
        assert!(rule.enabled_for(&subject("bob@example.test", "u-7", "en")));
        assert!(rule.enabled_for(&subject("bob@example.test", "u-1", "id")));
        assert!(!rule.enabled_for(&subject("bob@example.test", "u-1", "en")));
        assert!(!rule.enabled_for(&subject("Ada@example.test", "u-1", "en")), "matches are exact");
        assert!(FlagRule::on().enabled_for(&Subject::default()));
        assert!(!rule.enabled_for(&Subject::default()));
    }

    #[test]
    fn source_defaults_open_ai_youtube_mobile_and_history_and_close_the_rest() {
        let caps = FlagConfig::default().capabilities(&subject("bob@example.test", "u-1", "en"), true);
        assert_eq!(
            caps,
            Capabilities {
                ai: true,
                ai_chat: true,
                youtube: true,
                microphone: false,
                google_drive: false,
                onedrive: false,
                show_original: false,
                mobile_ux_jobs: true,
                mobile_expanded_player: false,
                mobile_ux_ai: true,
                billing_history: true,
                customer_lookup: false,
            }
        );
    }

    #[test]
    fn chat_needs_both_the_ai_flag_and_the_chat_switch() {
        let mut config = FlagConfig::default();
        let who = subject("bob@example.test", "u-1", "en");
        assert!(!config.capabilities(&who, false).ai_chat);
        config.ai_features = FlagRule::off();
        assert!(!config.capabilities(&who, true).ai_chat);
    }

    #[test]
    fn an_allow_listed_admin_alone_sees_customer_lookup() {
        let mut config = FlagConfig::default();
        config.billing_customer_lookup.allowed_emails = vec!["admin@example.test".into()];
        assert!(config.capabilities(&subject("admin@example.test", "u-9", "en"), true).customer_lookup);
        assert!(!config.capabilities(&subject("bob@example.test", "u-1", "en"), true).customer_lookup);
    }
}
