//! Settings "Manage Data" tab (PRV-002/010/011/027/028/036, PRV-D01 intended),
//! ported from `ManageDataView.tsx`. The copy is the source's `common` keys;
//! the erasure states, which the source never had, are new `app` keys.

use serde::{Deserialize, Serialize};

use crate::presentation::failure::ApiFailure;
use crate::presentation::msg::{Msg, app, common};
use crate::privacy::erasure::ErasureView;
use crate::privacy::settings::UserSettings;

pub const PRIVACY_INFO: [Msg; 4] = [
    common("Our system automatically deletes data every month."),
    common("You have the authority to delete your transcripts anytime."),
    common("You can delete all transcript files with one click."),
    common("Transcript cleanup runs daily and removes transcripts older than 30 days."),
];
pub const CLEANUP_POLICY: Msg = common(
    "Transcripts from jobs completed more than 30 days ago are automatically deleted to save storage space. Job metadata and settings are always preserved.",
);
pub const CLEANUP_WARNING: Msg = common("Deletion occurs daily at 2 AM UTC. Deleted transcripts cannot be recovered.");
pub const FILES_GONE_NOTE: Msg = common("Please note that once files are deleted, they cannot be recovered.");
const CANNOT_UNDO: Msg = common("This action cannot be undone. Are you sure you want to proceed?");

/// P3 D15 honest erasure states (meanings in `implementation/core.md`).
pub const ERASURE_REQUESTED: Msg = app("p5.erasure.requested");
pub const ERASURE_LOCAL_DONE: Msg = app("p5.erasure.local_data_erased");
pub const ERASURE_FULL: Msg = app("p5.erasure.fully_erased");
pub const ERASURE_EXCEPTIONS: Msg = app("p5.erasure.erased_with_exceptions");
pub const ERASURE_REQUEST_FAILED: Msg = app("p5.erasure.request_failed");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Confirm {
    /// PRV-027: turning cleanup off asks first.
    DisableCleanup,
    /// PRV-036: the legacy toggle asks before turning on.
    DeleteMonthly,
    /// PRV-011
    DeleteAll,
    /// PRV-002
    DeleteAccount,
}

impl Confirm {
    pub fn title(self) -> Msg {
        common(match self {
            Confirm::DisableCleanup => "Disable automatic transcript cleanup?",
            Confirm::DeleteMonthly => "Are you sure you want to delete all your data automatically?",
            Confirm::DeleteAll => "Are you sure you want to delete all of your data?",
            Confirm::DeleteAccount => "Are you sure you want to delete your account?",
        })
    }

    pub fn description(self) -> Msg {
        match self {
            Confirm::DisableCleanup => common(
                "By disabling automatic cleanup, your transcripts will be kept indefinitely. This may increase your storage usage over time. You can re-enable this setting anytime.",
            ),
            Confirm::DeleteMonthly => {
                common("By clicking YES, your data will delete automatically from the server totally once it passed one month!")
            }
            Confirm::DeleteAll | Confirm::DeleteAccount => CANNOT_UNDO,
        }
    }

    pub fn confirm_label(self) -> Msg {
        match self {
            Confirm::DisableCleanup => common("Disable Cleanup"),
            _ => common("Yes"),
        }
    }
}

/// The owner's erasure answer as copy: the typed state stays the authority,
/// the internal blocker and exception texts are never shown.
pub fn erasure_message(view: &ErasureView) -> Msg {
    match view {
        ErasureView::Requested { .. } => ERASURE_REQUESTED,
        ErasureView::LocalDataErased { pending, .. } => ERASURE_LOCAL_DONE.count(i64::from(*pending)),
        ErasureView::FullyErased => ERASURE_FULL,
        ErasureView::ErasedWithExceptions { .. } => ERASURE_EXCEPTIONS,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Loaded { settings: UserSettings },
    CleanupToggled { on: bool },
    DeleteMonthlyToggled { on: bool },
    DeleteAllPressed,
    DeleteAccountPressed,
    Confirmed,
    Canceled,
    Saved { settings: UserSettings },
    /// The honest erasure state from the server.
    ErasureState { view: ErasureView },
    ErasureFailed { failure: ApiFailure },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    SetAutoDelete { value: bool },
    SetDeletedMonthly { value: bool },
    DeleteAllJobs,
    RequestErasure,
    /// PRV-010 intended: sign out only once local data is gone, never on a failure.
    SignOut,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DataManagementState {
    pub settings: Option<UserSettings>,
    pub confirming: Option<Confirm>,
    pub erasure: Option<ErasureView>,
    pub failed: bool,
    signed_out: bool,
}

impl DataManagementState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Loaded { settings } | Event::Saved { settings } => {
                self.settings = Some(settings);
                vec![]
            }
            Event::CleanupToggled { on } => {
                if on {
                    // Turning cleanup on writes at once.
                    if let Some(s) = self.settings.as_mut() {
                        s.auto_delete_transcripts = true;
                    }
                    vec![Effect::SetAutoDelete { value: true }]
                } else {
                    self.confirming = Some(Confirm::DisableCleanup);
                    vec![]
                }
            }
            Event::DeleteMonthlyToggled { on } => {
                if on {
                    self.confirming = Some(Confirm::DeleteMonthly);
                    vec![]
                } else {
                    if let Some(s) = self.settings.as_mut() {
                        s.deleted_monthly = false;
                    }
                    vec![Effect::SetDeletedMonthly { value: false }]
                }
            }
            Event::DeleteAllPressed => {
                self.confirming = Some(Confirm::DeleteAll);
                vec![]
            }
            Event::DeleteAccountPressed => {
                self.confirming = Some(Confirm::DeleteAccount);
                vec![]
            }
            Event::Canceled => {
                self.confirming = None;
                vec![]
            }
            Event::Confirmed => match self.confirming.take() {
                Some(Confirm::DisableCleanup) => {
                    if let Some(s) = self.settings.as_mut() {
                        s.auto_delete_transcripts = false;
                    }
                    vec![Effect::SetAutoDelete { value: false }]
                }
                Some(Confirm::DeleteMonthly) => {
                    if let Some(s) = self.settings.as_mut() {
                        s.deleted_monthly = true;
                    }
                    vec![Effect::SetDeletedMonthly { value: true }]
                }
                Some(Confirm::DeleteAll) => vec![Effect::DeleteAllJobs],
                Some(Confirm::DeleteAccount) => {
                    self.failed = false;
                    vec![Effect::RequestErasure]
                }
                None => vec![],
            },
            Event::ErasureState { view } => {
                // The session ends once local data is gone; while billing still stops a
                // subscription the account is closed but the honest state is "requested".
                let local_gone = !matches!(view, ErasureView::Requested { .. });
                self.erasure = Some(view);
                if local_gone && !self.signed_out {
                    self.signed_out = true;
                    return vec![Effect::SignOut];
                }
                vec![]
            }
            Event::ErasureFailed { .. } => {
                self.failed = true;
                vec![]
            }
        }
    }

    pub fn view(&self) -> DataManagementVm {
        let cleanup_on = self.settings.as_ref().is_none_or(|s| s.auto_delete_transcripts);
        DataManagementVm {
            privacy_info: PRIVACY_INFO.to_vec(),
            cleanup_policy: CLEANUP_POLICY,
            cleanup_on,
            // PRV-028: the warning shows only while cleanup is on.
            cleanup_warning: cleanup_on.then_some(CLEANUP_WARNING),
            delete_monthly_on: self.settings.as_ref().is_none_or(|s| s.deleted_monthly),
            files_gone_note: FILES_GONE_NOTE,
            confirm: self.confirming.map(|c| ConfirmVm {
                kind: c,
                title: c.title(),
                description: c.description(),
                confirm_label: c.confirm_label(),
            }),
            erasure: self.erasure.as_ref().map(erasure_message),
            error: self.failed.then_some(ERASURE_REQUEST_FAILED),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "DataManagementConfirmVm"))]
pub struct ConfirmVm {
    pub kind: Confirm,
    pub title: Msg,
    pub description: Msg,
    pub confirm_label: Msg,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DataManagementVm {
    pub privacy_info: Vec<Msg>,
    pub cleanup_policy: Msg,
    pub cleanup_on: bool,
    pub cleanup_warning: Option<Msg>,
    pub delete_monthly_on: bool,
    pub files_gone_note: Msg,
    pub confirm: Option<ConfirmVm>,
    /// The honest erasure state; never "fully erased" before the server says so.
    pub erasure: Option<Msg>,
    pub error: Option<Msg>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::AccountId;

    fn loaded() -> DataManagementState {
        let mut s = DataManagementState::default();
        s.update(Event::Loaded { settings: UserSettings::defaults(AccountId(uuid::Uuid::nil())) });
        s
    }

    // PRV-027: off asks first, on writes at once; cancel keeps it on.
    #[test]
    fn disabling_cleanup_asks_first() {
        let mut s = loaded();
        assert!(s.update(Event::CleanupToggled { on: false }).is_empty());
        assert_eq!(s.view().confirm.unwrap().confirm_label, common("Disable Cleanup"));
        s.update(Event::Canceled);
        assert!(s.view().cleanup_on);
        s.update(Event::CleanupToggled { on: false });
        assert_eq!(s.update(Event::Confirmed), vec![Effect::SetAutoDelete { value: false }]);
        assert!(s.view().cleanup_warning.is_none());
        assert_eq!(s.update(Event::CleanupToggled { on: true }), vec![Effect::SetAutoDelete { value: true }]);
    }

    // PRV-002, PRV-D01: confirm, then sign out only after an honest erasure state.
    #[test]
    fn account_deletion_signs_out_after_the_server_answer() {
        let mut s = loaded();
        s.update(Event::DeleteAccountPressed);
        assert_eq!(s.view().confirm.unwrap().title, common("Are you sure you want to delete your account?"));
        assert_eq!(s.update(Event::Confirmed), vec![Effect::RequestErasure]);
        assert!(s.update(Event::ErasureFailed { failure: ApiFailure::http(503, None) }).is_empty());
        assert_eq!(s.view().error, Some(ERASURE_REQUEST_FAILED));
        let waiting = ErasureView::Requested { waiting_for: vec!["subscription s at iyzico: not yet confirmed stopped".into()] };
        assert!(s.update(Event::ErasureState { view: waiting }).is_empty());
        assert_eq!(s.view().erasure, Some(ERASURE_REQUESTED), "no internal blocker text reaches the screen");
        let effects = s.update(Event::ErasureState { view: ErasureView::LocalDataErased { pending: 2, exceptions: vec![] } });
        assert_eq!(effects, vec![Effect::SignOut]);
        assert_eq!(s.view().erasure, Some(ERASURE_LOCAL_DONE.count(2)));
    }

    // P3 D15: exceptions and remaining work are never presented as full erasure.
    #[test]
    fn only_a_fully_erased_answer_reads_as_fully_erased() {
        assert_eq!(erasure_message(&ErasureView::ErasedWithExceptions { exceptions: vec!["x".into()] }), ERASURE_EXCEPTIONS);
        assert_eq!(erasure_message(&ErasureView::FullyErased), ERASURE_FULL);
        assert_ne!(erasure_message(&ErasureView::LocalDataErased { pending: 0, exceptions: vec![] }), ERASURE_FULL);
    }
}
