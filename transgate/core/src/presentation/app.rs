//! The composed signed-in app (P5 D1): one presenter per browser tab owns one
//! [`AppState`]. It holds the session, the route, the layouts and every screen
//! state machine, and fences every asynchronous answer by [`Epoch`]: a late
//! answer for an earlier session, an earlier consent decision or another job is
//! dropped, never shown (the taskr `Epoch` pattern).
//!
//! The driver (transgate-wasm) executes each [`AppEffect`] with the Rust client
//! and feeds its outcome back as an [`AppEvent`] carrying the same epoch; it
//! checks [`AppState::is_current`] after every await and right before saving,
//! sharing, copying or submitting a checkout. Shells send only [`AppIntent`]s
//! and render [`AppVm`]. Request keys are minted here, never by a shell.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use billing_core::presentation::BALANCE_REFRESH_SECONDS;
use billing_core::presentation::purchase::{CatalogView, PurchaseEffect, PurchaseEvent, PurchaseState, PurchaseVm};
use billing_core::presentation::subscription::{SubscriptionEffect, SubscriptionEvent, SubscriptionState, SubscriptionVm};
use billing_core::presentation::confirm::{ConfirmEffect, ConfirmEvent, ConfirmPageState, ConfirmPayload, ConfirmVm};
use billing_core::presentation::invoices::{InvoicesEffect, InvoicesEvent, InvoicesState, InvoicesVm};
use billing_core::pricing::SpecialClientAlert;
use super::billing::{self, BillingMessages, ConfirmMessages};

use crate::account::{FeedbackRating, SessionProfile};
use crate::documents::DocumentFormat;
use crate::flags::Capabilities;
use crate::ids::{JobId, LineId, Micros};
use crate::jobs::app::JobDetail;
use crate::jobs::domain::JobStatus;
use crate::languages::Language;
use crate::presentation::ai::section_copy::SectionId;
use crate::presentation::ai::{analysis, chat, consent};
use crate::presentation::failure::ApiFailure;
use crate::presentation::layout::{self, Frame, Layouts, Viewport};
use crate::presentation::msg::{Msg, app, auth, common};
use crate::presentation::route::{self, HomeView, Route};
use crate::presentation::telemetry::TelemetryEvent;
use crate::presentation::upload_queue::{Alert, Choice, Mode, PickedFile};
use crate::presentation::{data_management, dates, feedback, job_list, settings, support, transcript, upload_queue};
use crate::transcript::LanguageTab;

/// How long the "Password updated successfully!" notice stays before the page
/// goes to the public root (IDN-017).
pub const PASSWORD_UPDATED_REDIRECT_MS: u64 = 2000;

/// D11 owned-list poll while nothing listed is in progress: the draft poll's
/// 30-second ceiling (`upload_queue::poll_interval_ms`). While a listed job is
/// in progress the transcript's 2-second running poll applies
/// (`transcript::RUNNING_POLL_MS`). Both replace the source realtime channel
/// (`JobRepository.ts:433-464`).
pub const JOBS_IDLE_POLL_MS: u64 = 30_000;

/// The tab's configuration from the host (`tg-config`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    /// `NEXT_PUBLIC_SITE_URL`: the origin auth redirects return to.
    pub site_url: String,
    /// Turnstile is configured: sign-in forms need a captcha token (D12).
    pub captcha: bool,
}

/// Fences an asynchronous answer. `session` advances on every sign-in
/// (including the same owner), sign-out, account switch and erasure.
/// `consent` is set only on AI effects and advances whenever the AI consent
/// decision changes (granted, revoked, cleanup), so a held AI answer, copy
/// or export is dropped after a refusal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Epoch {
    pub session: u64,
    pub consent: Option<u64>,
}

/// What the identity adapter says about the signed-in user. Tokens never
/// enter core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFacts {
    pub user_id: Uuid,
    pub email: String,
    pub expires_at: Micros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AuthFailure {
    InvalidCredentials,
    EmailNotConfirmed,
    RateLimited,
    WeakPassword,
    UserExists,
    InvalidOrExpiredCode,
    CaptchaFailed,
    Network,
    Unavailable,
}

impl AuthFailure {
    pub fn message(self) -> Msg {
        app(match self {
            AuthFailure::InvalidCredentials => "p5.auth.invalid_credentials",
            AuthFailure::EmailNotConfirmed => "p5.auth.email_not_confirmed",
            AuthFailure::RateLimited => "p5.auth.rate_limited",
            AuthFailure::WeakPassword => "p5.auth.weak_password",
            AuthFailure::UserExists => "p5.auth.user_exists",
            AuthFailure::InvalidOrExpiredCode => "p5.auth.invalid_or_expired_code",
            AuthFailure::CaptchaFailed => "p5.auth.captcha_failed",
            AuthFailure::Network => "p5.auth.network",
            AuthFailure::Unavailable => "p5.auth.unavailable",
        })
    }
}

/// GoTrue work the driver performs (PKCE, storage, cross-tab refresh).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthEffect {
    /// Read the stored session (refreshing it if due).
    RestoreSession,
    PasswordSignIn { email: String, password: String, captcha: Option<String> },
    SignUp { email: String, password: String, redirect: String, captcha: Option<String> },
    MagicLink { email: String, redirect: String, captcha: Option<String> },
    Recover { email: String, redirect: String, captcha: Option<String> },
    GoogleRedirect { redirect: String },
    ExchangeCallback { code: String },
    UpdatePassword { password: String },
    /// One refresh after a 401.
    Refresh,
    SignOut,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthEvent {
    Restored { facts: Option<SessionFacts> },
    SignedIn { facts: SessionFacts },
    /// A recovery link was exchanged: the update-password screen follows.
    PasswordRecovery { facts: SessionFacts },
    /// A refresh or another tab replaced the session.
    SessionAdopted { facts: SessionFacts },
    SignedOut,
    EmailSent,
    PasswordUpdated,
    Failed { kind: AuthFailure },
}

/// `GET /api/billing/overview`: what the header shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceView {
    /// Minutes; `None` when unknown.
    pub balance: Option<i64>,
    /// FRE app definition 1 (the transcript display cut).
    pub is_freemium: bool,
    pub special_client_alert: Option<SpecialClientAlert>,
}

/// WEB-BIL-001: the source `Balance.tsx` text shapes.
pub fn balance_text(balance: Option<i64>) -> Msg {
    match balance {
        None => app("p5.balance.none"),
        Some(m) if m < 60 => app("p5.balance.minutes").number("minutes", m),
        Some(m) => app("p5.balance.hours_minutes").number("hours", m / 60).number("minutes", m % 60),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timer {
    Balance,
    PasswordUpdated,
    Confirmation { sequence: u64 },
    /// D11 owned-list poll; a stale `sequence` (hidden tab, new chain) is ignored.
    JobsPoll { sequence: u64 },
}

// ---- intents (the shell's only input) --------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    SignIn,
    SignUp,
    MagicLink,
    ForgottenPassword,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthIntent {
    ModeChosen { mode: AuthMode },
    EmailChanged { text: String },
    PasswordChanged { text: String },
    CaptchaSolved { token: String },
    CaptchaExpired,
    Submitted,
    GoogleChosen,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewPasswordIntent {
    PasswordChanged { text: String },
    Submitted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UploadIntent {
    Picked { files: Vec<PickedFile> },
    Dropped { files: Vec<PickedFile> },
    Removed {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        file: u64,
    },
    LanguageChosen { choice: Option<Choice> },
    SpeakersToggled { enabled: bool },
    TabChanged { mode: Mode },
    SourceLanguageChosen { language: Option<Language> },
    TargetLanguageChosen { language: Option<Language> },
    YouTubeDialogOpened,
    YouTubeDialogClosed,
    YouTubeUrlChanged { url: String },
    YouTubeSubmitted,
    StartPressed,
    DismissAlert { alert: Alert },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptIntent {
    TabSelected { tab: LanguageTab },
    EditStarted { line: LineId },
    EditCanceled,
    EditSubmitted { text: String },
    SpeakerChosen { line: LineId, speaker: String },
    SpeakerRenamed { from: String, to: String },
    ShowTimeToggled { on: bool },
    ShowSpeakersToggled { on: bool },
    TimeClicked { line: LineId },
    DownloadPressed { all_formats: bool },
    CopyPressed,
    PlayerState { playing: bool },
    NoAudioAlertDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobsIntent {
    LoadMore,
    Refresh,
    DeleteRequested { job: JobId },
    DeleteConfirmed,
    DeleteCanceled,
    DownloadRequested { job: JobId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManageDataIntent {
    CleanupToggled { on: bool },
    DeleteMonthlyToggled { on: bool },
    DeleteAllPressed,
    DeleteAccountPressed,
    Confirmed,
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedbackIntent {
    Opened { job: JobId },
    Closed,
    Selected { rating: FeedbackRating },
    CommentChanged { text: String },
    Submitted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SupportIntent {
    Opened,
    Closed,
    MessageChanged { text: String },
    /// Sent with the route's job, as the source read `router.query.jobId`.
    Submitted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsentIntent {
    EnableRequested,
    DeclineRequested,
    DisableConfirmed,
    ErrorDismissed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnalysisIntent {
    Shown,
    Hidden,
    RetryRequested,
    CopyRequested,
    CopySectionRequested { section: SectionId },
    ExportRequested { format: DocumentFormat },
    ExportErrorDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatIntent {
    SendRequested { text: String },
    RetryRequested {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        index: usize,
    },
    CopyMessageRequested {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        index: usize,
    },
    RateRequested {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        index: usize,
        rating: chat::Rating,
    },
    CopyChatRequested,
    DownloadRequested,
    ErrorDismissed,
    NoticeDismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PurchaseIntent {
    SelectPayAsYouGo,
    SelectPlan {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        package_id: usize,
    },
    PlanSectionClicked,
    IntervalChosen { yearly: bool },
    PresetHours {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        hours: i64,
    },
    HoursTyped { text: String },
    StepHours {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        delta: i64,
    },
    CouponTyped { text: String },
    CouponApplied,
    TermsAccepted { accepted: bool },
    PayRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubscriptionIntent {
    Refresh,
    UpgradeClicked,
    CancelClicked,
    CancelDismissed,
    CancelConfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvoicesIntent {
    Preview { #[cfg_attr(feature = "ts", ts(type = "number"))] payment_id: i64 },
    ClosePreview,
    Download { #[cfg_attr(feature = "ts", ts(type = "number"))] payment_id: i64 },
    LookupEmailTyped { text: String },
    LookupRequested,
    LookupCleared,
    TemporaryBillToTyped { text: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", content = "intent", rename_all = "snake_case")]
pub enum AppIntent {
    /// The tab started at `url`; `utc_offset_minutes` is `-getTimezoneOffset()`.
    Started { url: String, viewport: Viewport, utc_offset_minutes: i16 },
    /// History navigation (popstate).
    RouteChanged { url: String },
    ViewportChanged { viewport: Viewport },
    /// `document.visibilityState`; D11 polls the owned list only while visible.
    VisibilityChanged { visible: bool },
    /// An in-app link: core pushes the history entry.
    Navigate { route: Route },
    /// The language switcher: a full page load of `/{code}/home`, no cookie (LNG-002).
    LocaleChosen { code: String },
    SignOutRequested,
    SessionRetried,
    Auth(AuthIntent),
    NewPassword(NewPasswordIntent),
    Upload(UploadIntent),
    Transcript(TranscriptIntent),
    Jobs(JobsIntent),
    Settings(settings::Event),
    ManageData(ManageDataIntent),
    Feedback(FeedbackIntent),
    Support(SupportIntent),
    Consent(ConsentIntent),
    Analysis(AnalysisIntent),
    Chat(ChatIntent),
    PurchaseOpened,
    PurchaseClosed,
    Purchase(PurchaseIntent),
    Subscription(SubscriptionIntent),
    Invoices(InvoicesIntent),
}

// ---- driver events and effects -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    User(AppIntent),
    Auth { epoch: Epoch, event: AuthEvent },
    SessionLoaded { epoch: Epoch, result: Result<SessionProfile, ApiFailure> },
    /// Any request of this epoch was answered 401.
    Unauthorized { epoch: Epoch },
    /// Any request of this epoch was answered 410 `account_deletion_in_progress`.
    AccountErased { epoch: Epoch },
    Balance { epoch: Epoch, result: Result<BalanceView, ApiFailure> },
    Timer { epoch: Epoch, timer: Timer },
    JobLoaded { epoch: Epoch, job: JobId, result: Result<JobDetail, ApiFailure> },
    Upload { epoch: Epoch, event: upload_queue::Event },
    Transcript { epoch: Epoch, job: JobId, event: transcript::Event },
    Jobs { epoch: Epoch, event: job_list::Event },
    ManageData { epoch: Epoch, event: data_management::Event },
    Feedback { epoch: Epoch, event: feedback::Event },
    Support { epoch: Epoch, event: support::Event },
    Consent { epoch: Epoch, event: consent::Event },
    Analysis { epoch: Epoch, job: JobId, event: analysis::Event },
    Chat { epoch: Epoch, job: JobId, event: chat::Event },
    Catalog { epoch: Epoch, result: Result<CatalogView, ApiFailure> },
    Purchase { epoch: Epoch, event: PurchaseEvent },
    Subscription { epoch: Epoch, event: SubscriptionEvent },
    Invoices { epoch: Epoch, ticket: u64, event: InvoicesEvent },
    ConfirmationLoaded { epoch: Epoch, sequence: u64, payload: Option<ConfirmPayload> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppEffect {
    Auth { epoch: Epoch, effect: AuthEffect },
    /// `GET /api/session?locale=`.
    LoadSession { epoch: Epoch, locale: String },
    /// History push or replace of an app path.
    Navigate { path: String, replace: bool },
    /// A full page load (the public root, another locale).
    Leave { url: String },
    LoadBalance { epoch: Epoch },
    Timer { epoch: Epoch, timer: Timer, after_ms: u64 },
    /// `GET /api/jobs/{id}`.
    LoadJob { epoch: Epoch, job: JobId },
    /// `GET /api/jobs/drafts` and `GET /api/languages/usage`, answered as `Upload{Restored}`.
    LoadQueue { epoch: Epoch },
    /// `GET /api/settings`, answered as `ManageData{Loaded}`.
    LoadSettings { epoch: Epoch },
    /// `GET /api/billing/catalog`.
    LoadCatalog { epoch: Epoch },
    Upload { epoch: Epoch, effect: upload_queue::Effect },
    Transcript { epoch: Epoch, job: JobId, effect: transcript::Effect },
    Jobs { epoch: Epoch, effect: job_list::Effect },
    ManageData { epoch: Epoch, effect: data_management::Effect },
    Feedback { epoch: Epoch, effect: feedback::Effect },
    Support { epoch: Epoch, effect: support::Effect },
    Consent { epoch: Epoch, effect: consent::Effect },
    Analysis { epoch: Epoch, job: JobId, effect: analysis::Effect },
    Chat { epoch: Epoch, job: JobId, effect: chat::Effect },
    Purchase { epoch: Epoch, effect: PurchaseEffect },
    Subscription { epoch: Epoch, effect: SubscriptionEffect },
    Invoices { epoch: Epoch, ticket: u64, effect: InvoicesEffect },
    /// Read `#tg-confirm` once. No HTTP confirmation or checkout request.
    ReadConfirmation { epoch: Epoch, sequence: u64 },
    Confirmation { epoch: Epoch, effect: ConfirmEffect },
    /// Product analytics; the shell sends it only with cookie consent (D12).
    Telemetry { event: TelemetryEvent },
    /// PostHog `reset()` after sign-out (IDN-024).
    TelemetryReset,
}

// ---- view ------------------------------------------------------------------------------

/// The language switcher's options in its source order, with its flags.
pub const LOCALE_SWITCHER: [(&str, &str, &str); 16] = [
    ("en", "English", "🇺🇸"),
    ("tr", "Turkish", "🇹🇷"),
    ("pt", "Portuguese", "🇧🇷"),
    ("es", "Spanish", "🇪🇸"),
    ("id", "Indonesian", "🇮🇩"),
    ("el", "Greek", "🇬🇷"),
    ("ru", "Russian", "🇷🇺"),
    ("fr", "French", "🇫🇷"),
    ("de", "German", "🇩🇪"),
    ("it", "Italian", "🇮🇹"),
    ("da", "Danish", "🇩🇰"),
    ("no", "Norwegian", "🇳🇴"),
    ("sv", "Swedish", "🇸🇪"),
    ("fi", "Finnish", "🇫🇮"),
    ("az", "Azerbaijani", "🇦🇿"),
    ("kk", "Kazakh", "🇰🇿"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LocaleOption {
    pub code: String,
    pub name: Msg,
    pub flag: String,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuthVm {
    pub mode: AuthMode,
    pub email: String,
    pub password: String,
    /// Turnstile is shown and its token is required (D12).
    pub captcha_required: bool,
    pub busy: bool,
    pub can_submit: bool,
    pub error: Option<Msg>,
    /// "Check your email ..." after a sign-up, magic link or recovery request.
    pub notice: Option<Msg>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewPasswordVm {
    pub password: String,
    pub busy: bool,
    pub error: Option<Msg>,
    pub success: Option<Msg>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SignedOutVm {
    pub locale: String,
    pub route: Route,
    pub locales: Vec<LocaleOption>,
    pub auth: AuthVm,
    pub new_password: NewPasswordVm,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SignedInVm {
    pub locale: String,
    pub route: Route,
    pub layouts: Layouts,
    pub locales: Vec<LocaleOption>,
    /// `None` while `GET /api/session` is answering.
    pub profile: Option<SessionProfile>,
    /// WEB-IDN-021: "Joined on" as the source's local `d-m-yyyy`.
    pub joined_on_text: Option<String>,
    pub session_error: Option<Msg>,
    pub capabilities: Capabilities,
    pub balance: Msg,
    /// The phone header (`home/[[...jobId]].tsx:375`): the open job's raw name
    /// when it has one, else `header_title`.
    pub header_title_text: Option<String>,
    pub header_title: Msg,
    /// Where "New Transcript" goes: `/home` on desktop, `/home?view=new` on the phone.
    pub new_transcript_route: Route,
    pub signing_out: bool,
    pub queue: upload_queue::UploadQueueVm,
    pub jobs: job_list::JobListVm,
    /// The open job's transcript; `None` off a job route.
    pub transcript: Option<transcript::TranscriptVm>,
    pub consent: consent::ConsentPanelVm,
    pub analysis: Option<analysis::AnalysisPanelVm>,
    pub chat: Option<chat::ChatPanelVm>,
    /// `None` until the profile is known.
    pub settings: Option<settings::SettingsVm>,
    pub manage_data: data_management::DataManagementVm,
    pub feedback: feedback::FeedbackVm,
    pub support: support::SupportVm,
    pub new_password: NewPasswordVm,
    pub subscription: SubscriptionVm,
    /// The purchase dialog while open.
    pub purchase: Option<PurchaseVm>,
    pub invoices: Option<InvoicesVm>,
    pub billing_messages: BillingMessages,
    /// Configured host data, not translated or recomputed (DEF-023).
    pub special_client_alert: Option<SpecialClientAlert>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AppVm {
    /// The source spinner while the session is read.
    Restoring { locale: String },
    SignedOut(SignedOutVm),
    SignedIn(Box<SignedInVm>),
    CheckoutConfirm { locale: String, vm: Option<ConfirmVm>, messages: Option<ConfirmMessages> },
    /// P3 D13: erasure started; terminal for this tab.
    Erased { locale: String, message: Msg },
}

pub const ACCOUNT_ERASED: Msg = app("p5.session.account_erased");
pub const SESSION_UNAVAILABLE: Msg = app("p5.session.unavailable");
pub const PASSWORD_UPDATED: Msg = common("Password updated successfully!");

// ---- state ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Starting,
    Restoring,
    SignedOut,
    SignedIn,
    Erased,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuthForm {
    mode: AuthMode,
    email: String,
    password: String,
    captcha: Option<String>,
    busy: bool,
    failure: Option<AuthFailure>,
    sent: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NewPasswordForm {
    password: String,
    busy: bool,
    failure: Option<AuthFailure>,
    done: bool,
}

pub struct AppState {
    config: AppConfig,
    phase: Phase,
    session: u64,
    consent_epoch: u64,
    locale: &'static str,
    route: Route,
    viewport: Viewport,
    utc_offset_minutes: i16,
    facts: Option<SessionFacts>,
    profile: Option<SessionProfile>,
    session_failed: bool,
    refreshing: bool,
    signing_out: bool,
    auth: AuthForm,
    new_password: NewPasswordForm,
    balance: Option<BalanceView>,
    selected: Option<JobId>,
    /// A loaded job waiting for the freemium answer before it is displayed.
    pending_job: Option<JobDetail>,
    has_lines: bool,
    queue: upload_queue::UploadQueueState,
    jobs: job_list::JobListState,
    transcript: transcript::TranscriptState,
    manage_data: data_management::DataManagementState,
    settings: settings::SettingsState,
    feedback: feedback::FeedbackState,
    support: support::SupportState,
    consent: consent::ConsentPanelState,
    analysis: analysis::AnalysisPanelState,
    chat: chat::ChatPanelState,
    subscription: SubscriptionState,
    purchase: Option<PurchaseState>,
    invoices: Option<InvoicesState>,
    invoice_lookup_failed: bool,
    invoice_preview_ticket: u64,
    confirmation: Option<ConfirmPageState>,
    confirmation_sequence: u64,
    /// `document.visibilityState` (D11); assumed visible until told otherwise.
    visible: bool,
    /// Cancels a pending owned-list poll timer when bumped.
    jobs_poll_sequence: u64,
    jobs_poll_timer: bool,
    jobs_poll_in_flight: bool,
}

fn locale_of(code: &str) -> &'static str {
    LOCALE_SWITCHER.iter().find(|(c, _, _)| *c == code).map_or("en", |(c, _, _)| c)
}

impl AppState {
    pub fn new(config: AppConfig) -> AppState {
        AppState {
            config,
            phase: Phase::Starting,
            session: 0,
            consent_epoch: 0,
            locale: "en",
            route: Route::Home(HomeView::Default),
            viewport: Viewport::default(),
            utc_offset_minutes: 0,
            facts: None,
            profile: None,
            session_failed: false,
            refreshing: false,
            signing_out: false,
            auth: AuthForm::default(),
            new_password: NewPasswordForm::default(),
            balance: None,
            selected: None,
            pending_job: None,
            has_lines: false,
            queue: upload_queue::UploadQueueState::default(),
            jobs: job_list::JobListState::default(),
            transcript: transcript::TranscriptState::default(),
            manage_data: data_management::DataManagementState::default(),
            settings: settings::SettingsState::default(),
            feedback: feedback::FeedbackState::default(),
            support: support::SupportState::default(),
            consent: consent::ConsentPanelState::default(),
            analysis: analysis::AnalysisPanelState::default(),
            chat: chat::ChatPanelState::default(),
            subscription: SubscriptionState::default(),
            purchase: None,
            invoices: None,
            invoice_lookup_failed: false,
            invoice_preview_ticket: 0,
            confirmation: None,
            confirmation_sequence: 0,
            visible: true,
            jobs_poll_sequence: 0,
            jobs_poll_timer: false,
            jobs_poll_in_flight: false,
        }
    }

    pub fn invoice_preview_ticket(&self) -> u64 {
        self.invoice_preview_ticket
    }

    /// A held renderer must check this immediately before opening a preview,
    /// in addition to the session epoch and the driver's disposal guard.
    pub fn is_current_invoice_preview(&self, ticket: u64, payment_id: i64) -> bool {
        self.signed_in() && self.route == Route::Billing && ticket == self.invoice_preview_ticket
            && self.invoices.as_ref().is_some_and(|state| state.is_previewing(payment_id))
    }

    fn invalidate_invoice_preview(&mut self) {
        self.invoice_preview_ticket += 1;
        if let Some(state) = self.invoices.as_mut() {
            state.update(InvoicesEvent::ClosePreview);
        }
    }

    /// The fence of session work.
    pub fn epoch(&self) -> Epoch {
        Epoch { session: self.session, consent: None }
    }

    /// The fence of AI work: the session and the current consent decision.
    pub fn ai_epoch(&self) -> Epoch {
        Epoch { session: self.session, consent: Some(self.consent_epoch) }
    }

    /// Whether an effect's answer still belongs here: same session and, for
    /// AI work, the same consent decision. The driver also checks it right
    /// before saving, sharing, copying or submitting.
    pub fn is_current(&self, epoch: Epoch) -> bool {
        epoch.session == self.session && epoch.consent.is_none_or(|c| c == self.consent_epoch) && self.phase != Phase::Erased
    }

    fn signed_in(&self) -> bool {
        self.phase == Phase::SignedIn
    }

    fn capabilities(&self) -> Capabilities {
        self.profile.as_ref().map(|p| p.capabilities).unwrap_or_default()
    }

    pub fn layouts(&self) -> Layouts {
        layout::layouts(self.viewport, &self.capabilities())
    }

    fn redirect_url(&self, route: Route) -> String {
        format!("{}{}", self.config.site_url.trim_end_matches('/'), route::href(route, self.locale))
    }

    fn navigate(&mut self, route: Route, replace: bool) -> Vec<AppEffect> {
        let path = route::href(route, self.locale);
        let mut effects = vec![AppEffect::Navigate { path, replace }];
        effects.extend(self.enter_route(route));
        effects
    }

    // ---- session ------------------------------------------------------------------------

    fn begin_session(&mut self, facts: SessionFacts) -> Vec<AppEffect> {
        self.reset_session_state();
        self.session += 1;
        self.phase = Phase::SignedIn;
        self.facts = Some(facts);
        let mut effects = vec![AppEffect::LoadSession { epoch: self.epoch(), locale: self.locale.to_string() }];
        if self.route == Route::SignIn {
            // `signin.tsx`: a signed-in visitor is replaced to /home.
            effects.extend(self.navigate(Route::Home(HomeView::Default), true));
        }
        effects
    }

    fn reset_session_state(&mut self) {
        self.facts = None;
        self.profile = None;
        self.session_failed = false;
        self.refreshing = false;
        self.signing_out = false;
        self.auth = AuthForm::default();
        self.balance = None;
        self.selected = None;
        self.pending_job = None;
        self.has_lines = false;
        self.queue = upload_queue::UploadQueueState::default();
        self.jobs = job_list::JobListState::default();
        self.cancel_jobs_poll();
        self.transcript = transcript::TranscriptState::default();
        self.manage_data = data_management::DataManagementState::default();
        self.settings = settings::SettingsState::default();
        self.feedback = feedback::FeedbackState::default();
        self.support = support::SupportState::default();
        self.consent = consent::ConsentPanelState::default();
        self.analysis = analysis::AnalysisPanelState::default();
        self.chat = chat::ChatPanelState::default();
        self.subscription = SubscriptionState::default();
        self.purchase = None;
        self.invoices = None;
        self.invoice_lookup_failed = false;
        self.invoice_preview_ticket += 1;
        self.confirmation = None;
        self.confirmation_sequence += 1;
    }

    /// IDN-022: a protected page without a user is replaced by /signin.
    fn end_session(&mut self) -> Vec<AppEffect> {
        self.reset_session_state();
        self.session += 1;
        self.consent_epoch += 1;
        self.phase = Phase::SignedOut;
        if self.route.protected() { self.navigate(Route::SignIn, true) } else { vec![] }
    }

    /// The profile arrived: every screen starts loading.
    fn start_screens(&mut self) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let mut effects = vec![
            AppEffect::LoadBalance { epoch },
            AppEffect::LoadQueue { epoch },
            AppEffect::LoadSettings { epoch },
        ];
        let list = self.jobs.update(job_list::Event::Refresh);
        effects.extend(list.into_iter().map(|effect| AppEffect::Jobs { epoch, effect }));
        if self.capabilities().ai {
            let consent = self.consent.update(consent::Event::Opened);
            effects.extend(consent.into_iter().map(|effect| AppEffect::Consent { epoch, effect }));
        }
        let (subscription, load) = SubscriptionState::new();
        self.subscription = subscription;
        effects.extend(load.into_iter().map(|effect| AppEffect::Subscription { epoch, effect }));
        let route = self.route;
        effects.extend(self.enter_route(route));
        effects
    }

    // ---- D11 owned-list polling ---------------------------------------------------------

    fn jobs_poll_due(&self) -> bool {
        self.signed_in() && self.visible && self.profile.is_some()
    }

    /// One poll now, unless one is already in flight (or a page load answers first).
    fn poll_jobs_now(&mut self) -> Vec<AppEffect> {
        if !self.jobs_poll_due() || self.jobs_poll_in_flight {
            return vec![];
        }
        let epoch = self.epoch();
        let effects: Vec<AppEffect> = self.jobs.update(job_list::Event::PollRequested).into_iter().map(|effect| AppEffect::Jobs { epoch, effect }).collect();
        self.jobs_poll_in_flight = !effects.is_empty();
        effects
    }

    /// The next tick of the single chain: fast while a listed job is in progress.
    fn arm_jobs_poll(&mut self) -> Vec<AppEffect> {
        if !self.jobs_poll_due() || self.jobs_poll_timer || self.jobs_poll_in_flight {
            return vec![];
        }
        self.jobs_poll_timer = true;
        let after_ms = if self.jobs.has_active() { transcript::RUNNING_POLL_MS } else { JOBS_IDLE_POLL_MS };
        vec![AppEffect::Timer { epoch: self.epoch(), timer: Timer::JobsPoll { sequence: self.jobs_poll_sequence }, after_ms }]
    }

    /// A pending tick becomes stale; an answer in flight is dropped by its epoch or ignored.
    fn cancel_jobs_poll(&mut self) {
        self.jobs_poll_sequence += 1;
        self.jobs_poll_timer = false;
        self.jobs_poll_in_flight = false;
    }

    fn sign_out(&mut self) -> Vec<AppEffect> {
        if self.signing_out || !self.signed_in() {
            return vec![];
        }
        self.signing_out = true;
        vec![
            AppEffect::Telemetry { event: TelemetryEvent::new("loggedOut") },
            AppEffect::TelemetryReset,
            AppEffect::Auth { epoch: self.epoch(), effect: AuthEffect::SignOut },
        ]
    }

    // ---- routes ---------------------------------------------------------------------------

    /// Applies a route already in the address bar: protection, the phone
    /// new-transcript redirect (MOBL-116) and job selection.
    fn enter_route(&mut self, route: Route) -> Vec<AppEffect> {
        let changed = self.route != route;
        self.route = route;
        if changed {
            self.invalidate_invoice_preview();
            self.confirmation = None;
            self.confirmation_sequence += 1;
        }
        if route == Route::CheckoutConfirm && self.phase != Phase::Erased {
            return if changed { vec![AppEffect::ReadConfirmation { epoch: self.epoch(), sequence: self.confirmation_sequence }] } else { vec![] };
        }
        match self.phase {
            Phase::SignedOut if route.protected() => return self.navigate(Route::SignIn, true),
            Phase::SignedIn if route == Route::SignIn => return self.navigate(Route::Home(HomeView::Default), true),
            Phase::SignedIn => {}
            _ => return vec![],
        }
        if self.profile.is_none() {
            return vec![];
        }
        if route == Route::Home(HomeView::Default) && self.layouts().frame == Frame::Phone {
            return self.navigate(Route::Home(HomeView::New), true);
        }
        if route == Route::Billing && self.invoices.is_none() {
            self.deselect_job();
            let (state, effects) = InvoicesState::new(self.capabilities().customer_lookup);
            self.invoices = Some(state);
            return self.invoice_effects(effects);
        }
        match route.job() {
            Some(job) if self.selected != Some(job) => self.select_job(job),
            Some(_) => vec![],
            None => {
                self.deselect_job();
                vec![]
            }
        }
    }

    fn deselect_job(&mut self) {
        self.selected = None;
        self.pending_job = None;
        self.has_lines = false;
        self.transcript = transcript::TranscriptState::default();
        self.analysis = analysis::AnalysisPanelState::default();
        self.chat = chat::ChatPanelState::default();
    }

    fn select_job(&mut self, job: JobId) -> Vec<AppEffect> {
        self.deselect_job();
        self.selected = Some(job);
        let mut effects = vec![AppEffect::LoadJob { epoch: self.epoch(), job }];
        let granted = self.consent.granted();
        let chat = [chat::Event::Selected { job }, chat::Event::ConsentChanged { granted }]
            .into_iter()
            .flat_map(|e| self.chat.update(e))
            .collect::<Vec<_>>();
        effects.extend(self.chat_effects(job, chat));
        effects
    }

    fn display_job(&mut self, detail: JobDetail) -> Vec<AppEffect> {
        let Some(balance) = self.balance.as_ref() else {
            self.pending_job = Some(detail);
            return vec![];
        };
        let job = detail.job.id;
        let name = detail.job.name.clone();
        let has_lines = !detail.lines.is_empty();
        let show_original = self.capabilities().show_original;
        let loaded = self.transcript.update(transcript::Event::Loaded { job: detail.job, lines: detail.lines, show_original, is_freemium: balance.is_freemium });
        let epoch = self.epoch();
        let mut effects: Vec<AppEffect> = loaded.into_iter().map(|effect| AppEffect::Transcript { epoch, job, effect }).collect();
        self.has_lines = has_lines;
        let granted = self.consent.granted();
        let analysis = [
            analysis::Event::Selected { job, job_name: name.clone(), file_name: name, has_lines },
            analysis::Event::ConsentChanged { granted },
        ]
        .into_iter()
        .flat_map(|e| self.analysis.update(e))
        .collect::<Vec<_>>();
        effects.extend(self.analysis_effects(job, analysis));
        effects
    }

    // ---- child effects ---------------------------------------------------------------------

    fn upload_effects(&mut self, effects: Vec<upload_queue::Effect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let mut out = Vec::new();
        for effect in effects {
            match effect {
                upload_queue::Effect::Navigate { job } => out.extend(self.navigate(Route::Home(HomeView::Job(job)), false)),
                upload_queue::Effect::Telemetry { event } => out.push(AppEffect::Telemetry { event }),
                effect => out.push(AppEffect::Upload { epoch, effect }),
            }
        }
        out
    }

    fn manage_data_effects(&mut self, effects: Vec<data_management::Effect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let mut out = Vec::new();
        for effect in effects {
            match effect {
                data_management::Effect::SignOut => out.extend(self.sign_out()),
                effect => out.push(AppEffect::ManageData { epoch, effect }),
            }
        }
        out
    }

    fn analysis_effects(&self, job: JobId, effects: Vec<analysis::Effect>) -> Vec<AppEffect> {
        let epoch = self.ai_epoch();
        effects.into_iter().map(|effect| AppEffect::Analysis { epoch, job, effect }).collect()
    }

    fn chat_effects(&self, job: JobId, effects: Vec<chat::Effect>) -> Vec<AppEffect> {
        let epoch = self.ai_epoch();
        effects
            .into_iter()
            .map(|effect| match effect {
                chat::Effect::Telemetry { event } => AppEffect::Telemetry { event },
                effect => AppEffect::Chat { epoch, job, effect },
            })
            .collect()
    }

    fn subscription_effects(&mut self, effects: Vec<SubscriptionEffect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let mut out = Vec::new();
        for effect in effects {
            match effect {
                SubscriptionEffect::OpenPurchase => out.extend(self.open_purchase()),
                effect => out.push(AppEffect::Subscription { epoch, effect }),
            }
        }
        out
    }

    fn purchase_effects(&self, effects: Vec<PurchaseEffect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        effects.into_iter().map(|effect| AppEffect::Purchase { epoch, effect }).collect()
    }

    fn invoice_effects(&self, effects: Vec<InvoicesEffect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let ticket = self.invoice_preview_ticket;
        effects.into_iter().map(|effect| AppEffect::Invoices { epoch, ticket, effect }).collect()
    }

    fn confirmation_effects(&self, effects: Vec<ConfirmEffect>) -> Vec<AppEffect> {
        let epoch = self.epoch();
        let mut out: Vec<_> = effects.into_iter().map(|effect| match effect {
            ConfirmEffect::NavigateHome => AppEffect::Leave { url: route::public_root(self.locale) },
            effect => AppEffect::Confirmation { epoch, effect },
        }).collect();
        if self.confirmation.as_ref().and_then(|c| c.view().countdown).is_some_and(|n| n > 0) {
            out.push(AppEffect::Timer { epoch, timer: Timer::Confirmation { sequence: self.confirmation_sequence }, after_ms: 1000 });
        }
        out
    }

    fn open_purchase(&mut self) -> Vec<AppEffect> {
        if self.purchase.is_some() {
            return vec![];
        }
        vec![AppEffect::LoadCatalog { epoch: self.epoch() }]
    }

    /// Runs a consent transition and, when the decision changed, advances the
    /// consent fence and tells both panels.
    fn consent_update(&mut self, event: consent::Event) -> Vec<AppEffect> {
        let before = self.consent.granted();
        let epoch = self.epoch();
        let mut effects: Vec<AppEffect> = self.consent.update(event).into_iter().map(|effect| AppEffect::Consent { epoch, effect }).collect();
        let granted = self.consent.granted();
        if granted != before {
            self.consent_epoch += 1;
            if let Some(job) = self.selected {
                let analysis = self.analysis.update(analysis::Event::ConsentChanged { granted });
                effects.extend(self.analysis_effects(job, analysis));
                let chat = self.chat.update(chat::Event::ConsentChanged { granted });
                effects.extend(self.chat_effects(job, chat));
            }
        }
        effects
    }

    /// ANLZ-1-03 (`[[...jobId]].tsx:245-331`): the open job reaching
    /// completed or failed while shown.
    fn status_telemetry(before: Option<JobStatus>, after: &crate::jobs::domain::Job) -> Option<AppEffect> {
        let name = match after.status {
            JobStatus::Completed => "transcription_completed",
            JobStatus::Failed => "transcription_failed",
            _ => return None,
        };
        if before == Some(after.status) || before.is_none() {
            return None;
        }
        let event = TelemetryEvent::new(name)
            .number("job_id", after.id.0 as f64)
            .text("provider", after.provider.map_or("unknown", |p| p.as_str()))
            .text("language", after.language.map_or("unknown", Language::name));
        Some(AppEffect::Telemetry { event })
    }

    // ---- update ------------------------------------------------------------------------------

    pub fn update(&mut self, event: AppEvent, now: Micros) -> Vec<AppEffect> {
        match event {
            AppEvent::User(intent) => self.intent(intent, now),
            AppEvent::Auth { epoch, event } if epoch.session == self.session => self.auth_event(event),
            AppEvent::SessionLoaded { epoch, result } if self.is_current(epoch) && self.signed_in() => match result {
                Ok(profile) => {
                    self.profile = Some(profile);
                    self.session_failed = false;
                    self.start_screens()
                }
                Err(_) => {
                    self.session_failed = true;
                    vec![]
                }
            },
            AppEvent::Unauthorized { epoch } if self.is_current(epoch) && self.signed_in() && !self.refreshing => {
                self.refreshing = true;
                vec![AppEffect::Auth { epoch: self.epoch(), effect: AuthEffect::Refresh }]
            }
            AppEvent::AccountErased { epoch } if self.is_current(epoch) && self.signed_in() => {
                // P3 D13: terminal; the local session is cleared, nothing retries.
                self.reset_session_state();
                self.session += 1;
                self.consent_epoch += 1;
                self.phase = Phase::Erased;
                vec![AppEffect::Auth { epoch: self.epoch(), effect: AuthEffect::SignOut }]
            }
            AppEvent::Balance { epoch, result } if self.is_current(epoch) && self.signed_in() => {
                let mut effects = vec![AppEffect::Timer { epoch, timer: Timer::Balance, after_ms: u64::from(BALANCE_REFRESH_SECONDS) * 1000 }];
                if let Ok(view) = result {
                    self.balance = Some(view);
                    if let Some(detail) = self.pending_job.take() {
                        effects.extend(self.display_job(detail));
                    }
                }
                effects
            }
            AppEvent::Timer { epoch, timer } if self.is_current(epoch) => match timer {
                Timer::Balance if self.signed_in() => vec![AppEffect::LoadBalance { epoch }],
                Timer::PasswordUpdated => vec![AppEffect::Leave { url: route::public_root(self.locale) }],
                Timer::Confirmation { sequence } if self.route == Route::CheckoutConfirm && sequence == self.confirmation_sequence => {
                    let Some(state) = self.confirmation.as_mut() else { return vec![] };
                    let effects = state.update(ConfirmEvent::Tick);
                    self.confirmation_effects(effects)
                }
                Timer::Confirmation { .. } => vec![],
                Timer::Balance => vec![],
                Timer::JobsPoll { sequence } if sequence == self.jobs_poll_sequence => {
                    self.jobs_poll_timer = false;
                    self.poll_jobs_now()
                }
                Timer::JobsPoll { .. } => vec![],
            },
            AppEvent::JobLoaded { epoch, job, result } if self.is_current(epoch) && self.selected == Some(job) => match result {
                Ok(detail) => self.display_job(detail),
                // The source showed an empty transcript for an unknown job.
                Err(_) => vec![],
            },
            AppEvent::Upload { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                let started = matches!(event, upload_queue::Event::StartSucceeded { .. });
                let effects = self.queue.update(event);
                let mut effects = self.upload_effects(effects);
                // D11: the started jobs appear in the list without a reload.
                if started {
                    effects.extend(self.poll_jobs_now());
                }
                effects
            }
            AppEvent::Transcript { epoch, job, event } if self.is_current(epoch) && self.selected == Some(job) => self.transcript_event(job, event),
            AppEvent::Jobs { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                let answer = matches!(event, job_list::Event::Polled { .. } | job_list::Event::PollFailed);
                let settles = answer || matches!(event, job_list::Event::PageLoaded { .. } | job_list::Event::LoadFailed { .. });
                if answer {
                    self.jobs_poll_in_flight = false;
                }
                let mut effects: Vec<AppEffect> = self.jobs.update(event).into_iter().map(|effect| AppEffect::Jobs { epoch, effect }).collect();
                if settles {
                    effects.extend(self.arm_jobs_poll());
                }
                effects
            }
            AppEvent::ManageData { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                let effects = self.manage_data.update(event);
                self.manage_data_effects(effects)
            }
            AppEvent::Feedback { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                self.feedback.update(event).into_iter().map(|effect| AppEffect::Feedback { epoch, effect }).collect()
            }
            AppEvent::Support { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                self.support.update(event).into_iter().map(|effect| AppEffect::Support { epoch, effect }).collect()
            }
            AppEvent::Consent { epoch, event } if self.is_current(epoch) && self.signed_in() => self.consent_update(event),
            AppEvent::Analysis { epoch, job, event } if self.is_current(epoch) && epoch.consent.is_some() && self.selected == Some(job) => {
                let effects = self.analysis.update(event);
                self.analysis_effects(job, effects)
            }
            AppEvent::Chat { epoch, job, event } if self.is_current(epoch) && epoch.consent.is_some() && self.selected == Some(job) => {
                let effects = self.chat.update(event);
                self.chat_effects(job, effects)
            }
            AppEvent::Catalog { epoch, result } if self.is_current(epoch) && self.signed_in() && self.purchase.is_none() => match result {
                Ok(catalog) => {
                    let (purchase, effects) = PurchaseState::new(catalog, self.locale);
                    self.purchase = Some(purchase);
                    self.purchase_effects(effects)
                }
                Err(_) => vec![],
            },
            AppEvent::Purchase { epoch, event } if self.is_current(epoch) => match self.purchase.as_mut() {
                Some(purchase) => {
                    let effects = purchase.update(event, now);
                    self.purchase_effects(effects)
                }
                None => vec![],
            },
            AppEvent::Subscription { epoch, event } if self.is_current(epoch) && self.signed_in() => {
                let effects = self.subscription.update(event, now);
                self.subscription_effects(effects)
            }
            AppEvent::Invoices { epoch, ticket, event } if self.is_current(epoch) && self.signed_in() => {
                if matches!(&event, InvoicesEvent::DocumentFailed { .. })
                    && (ticket != self.invoice_preview_ticket || self.route != Route::Billing)
                {
                    return vec![];
                }
                if matches!(&event, InvoicesEvent::Loaded(_) | InvoicesEvent::LookupAnswered(_)) {
                    self.invalidate_invoice_preview();
                }
                let Some(state) = self.invoices.as_mut() else { return vec![] };
                match &event {
                    InvoicesEvent::Loaded(_) => self.invoice_lookup_failed = false,
                    InvoicesEvent::LookupAnswered(result) => self.invoice_lookup_failed = result.is_err(),
                    _ => {}
                }
                let effects = state.update(event);
                self.invoice_effects(effects)
            }
            AppEvent::ConfirmationLoaded { epoch, sequence, payload }
                if self.is_current(epoch) && sequence == self.confirmation_sequence
                    && self.route == Route::CheckoutConfirm && self.confirmation.is_none() =>
            {
                let (state, effects) = ConfirmPageState::new(payload);
                self.confirmation = Some(state);
                self.confirmation_effects(effects)
            }
            _ => vec![],
        }
    }

    fn transcript_event(&mut self, job: JobId, event: transcript::Event) -> Vec<AppEffect> {
        let before = self.transcript.job.as_ref().map(|j| j.status);
        let effects = self.transcript.update(event);
        let epoch = self.epoch();
        let mut out: Vec<AppEffect> = effects.into_iter().map(|effect| AppEffect::Transcript { epoch, job, effect }).collect();
        if let Some(current) = self.transcript.job.as_ref()
            && let Some(telemetry) = Self::status_telemetry(before, current)
        {
            out.push(telemetry);
        }
        let has_lines = !self.transcript.lines.is_empty();
        if has_lines != self.has_lines {
            self.has_lines = has_lines;
            let analysis = self.analysis.update(analysis::Event::LinesChanged { has_lines });
            out.extend(self.analysis_effects(job, analysis));
        }
        out
    }

    fn auth_event(&mut self, event: AuthEvent) -> Vec<AppEffect> {
        match event {
            AuthEvent::Restored { facts: Some(facts) } | AuthEvent::SignedIn { facts } => self.begin_session(facts),
            AuthEvent::Restored { facts: None } => self.end_session(),
            AuthEvent::PasswordRecovery { facts } => {
                let mut effects = self.begin_session(facts);
                effects.extend(self.navigate(Route::NewPassword, true));
                effects
            }
            AuthEvent::SessionAdopted { facts } => {
                self.refreshing = false;
                match &self.facts {
                    // A rotated token for the same user changes nothing on screen.
                    Some(current) if current.user_id == facts.user_id && self.signed_in() => {
                        self.facts = Some(facts);
                        vec![]
                    }
                    _ => self.begin_session(facts),
                }
            }
            // The erased tab stays erased while its local session is cleared.
            AuthEvent::SignedOut if self.phase == Phase::Erased => vec![],
            AuthEvent::SignedOut => {
                let leaving = self.signing_out;
                let mut effects = self.end_session();
                if leaving {
                    // IDN-024: after sign-out the source pushed `/`.
                    effects = vec![AppEffect::Leave { url: route::public_root(self.locale) }];
                }
                effects
            }
            AuthEvent::EmailSent => {
                self.auth.busy = false;
                self.auth.sent = true;
                vec![]
            }
            AuthEvent::PasswordUpdated => {
                self.new_password.busy = false;
                self.new_password.done = true;
                self.new_password.password.clear();
                vec![AppEffect::Timer { epoch: self.epoch(), timer: Timer::PasswordUpdated, after_ms: PASSWORD_UPDATED_REDIRECT_MS }]
            }
            AuthEvent::Failed { .. } if self.signing_out => {
                // The identity service kept the session (offline, 5xx): still signed in,
                // and the user can sign out again.
                self.signing_out = false;
                vec![]
            }
            AuthEvent::Failed { kind } => {
                if self.new_password.busy {
                    self.new_password.busy = false;
                    self.new_password.failure = Some(kind);
                    return vec![];
                }
                self.auth.busy = false;
                self.auth.failure = Some(kind);
                if self.phase == Phase::Restoring {
                    // A failed callback exchange leaves the visitor signed out.
                    return self.end_session_keeping_form(kind);
                }
                if self.refreshing {
                    self.refreshing = false;
                    return self.end_session();
                }
                vec![]
            }
        }
    }

    fn end_session_keeping_form(&mut self, kind: AuthFailure) -> Vec<AppEffect> {
        let effects = self.end_session();
        self.auth.failure = Some(kind);
        effects
    }

    fn auth_submit(&mut self) -> Vec<AppEffect> {
        let form = &self.auth;
        if form.busy || (self.config.captcha && form.captcha.is_none()) {
            return vec![];
        }
        let (email, password, captcha) = (form.email.clone(), form.password.clone(), form.captcha.clone());
        let home = self.redirect_url(Route::Home(HomeView::Default));
        let effect = match form.mode {
            AuthMode::SignIn => AuthEffect::PasswordSignIn { email, password, captcha },
            AuthMode::SignUp => AuthEffect::SignUp { email, password, redirect: home, captcha },
            AuthMode::MagicLink => AuthEffect::MagicLink { email, redirect: home, captcha },
            AuthMode::ForgottenPassword => AuthEffect::Recover { email, redirect: self.redirect_url(Route::NewPassword), captcha },
        };
        self.auth.busy = true;
        self.auth.failure = None;
        self.auth.sent = false;
        vec![AppEffect::Auth { epoch: self.epoch(), effect }]
    }

    fn intent(&mut self, intent: AppIntent, now: Micros) -> Vec<AppEffect> {
        let epoch = self.epoch();
        match intent {
            AppIntent::Started { url, viewport, utc_offset_minutes } if self.phase == Phase::Starting => {
                self.viewport = viewport;
                self.utc_offset_minutes = utc_offset_minutes;
                self.phase = Phase::Restoring;
                let mut effects = Vec::new();
                let location = route::parse(&url);
                let (route, code, failed) = match location {
                    Some(l) => {
                        self.locale = l.locale;
                        if l.canonical {
                            effects.push(AppEffect::Navigate { path: route::href(l.route, self.locale), replace: true });
                        }
                        (l.route, l.auth_code, l.auth_error.is_some())
                    }
                    None => {
                        effects.push(AppEffect::Navigate { path: route::href(Route::Home(HomeView::Default), self.locale), replace: true });
                        (Route::Home(HomeView::Default), None, false)
                    }
                };
                self.route = route;
                if route == Route::CheckoutConfirm {
                    self.phase = Phase::SignedOut;
                    self.confirmation_sequence += 1;
                    effects.push(AppEffect::ReadConfirmation { epoch, sequence: self.confirmation_sequence });
                    return effects;
                }
                if failed {
                    self.auth.failure = Some(AuthFailure::InvalidOrExpiredCode);
                }
                let effect = match code {
                    Some(code) => AuthEffect::ExchangeCallback { code },
                    None => AuthEffect::RestoreSession,
                };
                effects.push(AppEffect::Auth { epoch, effect });
                effects
            }
            AppIntent::Started { .. } => vec![],
            AppIntent::RouteChanged { url } => match route::parse(&url) {
                Some(l) => self.enter_route(l.route),
                None => vec![],
            },
            // The shell reports the initial visibility once after Started. A pending tick is
            // superseded by a fresh one (stale sequence), so there is still one chain.
            AppIntent::VisibilityChanged { visible } if visible == self.visible => {
                if !visible || self.jobs_poll_in_flight {
                    return vec![];
                }
                self.jobs_poll_sequence += 1;
                self.jobs_poll_timer = false;
                self.arm_jobs_poll()
            }
            AppIntent::VisibilityChanged { visible } => {
                self.visible = visible;
                if !visible {
                    self.cancel_jobs_poll();
                    return vec![];
                }
                // Back in view: catch up at once, then the chain continues.
                self.poll_jobs_now()
            }
            AppIntent::ViewportChanged { viewport } => {
                self.viewport = viewport;
                let route = self.route;
                if self.signed_in() && route == Route::Home(HomeView::Default) { self.enter_route(route) } else { vec![] }
            }
            AppIntent::Navigate { route } if route != self.route => self.navigate(route, false),
            AppIntent::Navigate { .. } => vec![],
            AppIntent::LocaleChosen { code } => {
                let locale = locale_of(&code);
                vec![AppEffect::Leave { url: route::href(Route::Home(HomeView::Default), locale) }]
            }
            AppIntent::SignOutRequested => self.sign_out(),
            AppIntent::SessionRetried if self.signed_in() && self.session_failed => {
                self.session_failed = false;
                vec![AppEffect::LoadSession { epoch, locale: self.locale.to_string() }]
            }
            AppIntent::SessionRetried => vec![],
            AppIntent::Auth(intent) if self.phase == Phase::SignedOut => match intent {
                AuthIntent::ModeChosen { mode } => {
                    self.auth = AuthForm { mode, email: std::mem::take(&mut self.auth.email), captcha: self.auth.captcha.take(), ..AuthForm::default() };
                    vec![]
                }
                AuthIntent::EmailChanged { text } => {
                    self.auth.email = text;
                    vec![]
                }
                AuthIntent::PasswordChanged { text } => {
                    self.auth.password = text;
                    vec![]
                }
                AuthIntent::CaptchaSolved { token } => {
                    self.auth.captcha = Some(token);
                    vec![]
                }
                AuthIntent::CaptchaExpired => {
                    self.auth.captcha = None;
                    vec![]
                }
                AuthIntent::Submitted => self.auth_submit(),
                AuthIntent::GoogleChosen => vec![AppEffect::Auth { epoch, effect: AuthEffect::GoogleRedirect { redirect: self.redirect_url(Route::Home(HomeView::Default)) } }],
            },
            AppIntent::Auth(_) => vec![],
            AppIntent::NewPassword(intent) => match intent {
                NewPasswordIntent::PasswordChanged { text } if !self.new_password.busy => {
                    self.new_password.password = text;
                    vec![]
                }
                NewPasswordIntent::Submitted if !self.new_password.busy && !self.new_password.done => {
                    self.new_password.busy = true;
                    self.new_password.failure = None;
                    vec![AppEffect::Auth { epoch, effect: AuthEffect::UpdatePassword { password: self.new_password.password.clone() } }]
                }
                _ => vec![],
            },
            _ if !self.signed_in() => vec![],
            AppIntent::Upload(intent) => {
                let frame = self.layouts().frame;
                let event = match intent {
                    UploadIntent::Picked { files } => upload_queue::Event::Picked { files, frame },
                    UploadIntent::Dropped { files } => upload_queue::Event::Dropped { files },
                    UploadIntent::Removed { file } => upload_queue::Event::Removed { file },
                    UploadIntent::LanguageChosen { choice } => upload_queue::Event::LanguageChosen { choice },
                    UploadIntent::SpeakersToggled { enabled } => upload_queue::Event::SpeakersToggled { enabled },
                    UploadIntent::TabChanged { mode } => upload_queue::Event::TabChanged { mode, frame },
                    UploadIntent::SourceLanguageChosen { language } => upload_queue::Event::SourceLanguageChosen { language },
                    UploadIntent::TargetLanguageChosen { language } => upload_queue::Event::TargetLanguageChosen { language },
                    UploadIntent::YouTubeDialogOpened => upload_queue::Event::YouTubeDialogOpened,
                    UploadIntent::YouTubeDialogClosed => upload_queue::Event::YouTubeDialogClosed,
                    UploadIntent::YouTubeUrlChanged { url } => upload_queue::Event::YouTubeUrlChanged { url },
                    UploadIntent::YouTubeSubmitted => upload_queue::Event::YouTubeSubmitted,
                    UploadIntent::StartPressed => upload_queue::Event::StartPressed,
                    UploadIntent::DismissAlert { alert } => upload_queue::Event::DismissAlert { alert },
                };
                let effects = self.queue.update(event);
                self.upload_effects(effects)
            }
            AppIntent::Transcript(intent) => {
                let Some(job) = self.selected else { return vec![] };
                let event = match intent {
                    TranscriptIntent::TabSelected { tab } => transcript::Event::TabSelected { tab },
                    TranscriptIntent::EditStarted { line } => transcript::Event::EditStarted { line },
                    TranscriptIntent::EditCanceled => transcript::Event::EditCanceled,
                    TranscriptIntent::EditSubmitted { text } => transcript::Event::EditSubmitted { text },
                    TranscriptIntent::SpeakerChosen { line, speaker } => transcript::Event::SpeakerChosen { line, speaker },
                    TranscriptIntent::SpeakerRenamed { from, to } => transcript::Event::SpeakerRenamed { from, to },
                    TranscriptIntent::ShowTimeToggled { on } => transcript::Event::ShowTimeToggled { on },
                    TranscriptIntent::ShowSpeakersToggled { on } => transcript::Event::ShowSpeakersToggled { on },
                    TranscriptIntent::TimeClicked { line } => transcript::Event::TimeClicked { line },
                    TranscriptIntent::DownloadPressed { all_formats } => transcript::Event::DownloadPressed { all_formats },
                    TranscriptIntent::CopyPressed => transcript::Event::CopyPressed,
                    TranscriptIntent::PlayerState { playing } => transcript::Event::PlayerState { playing },
                    TranscriptIntent::NoAudioAlertDismissed => transcript::Event::NoAudioAlertDismissed,
                };
                self.transcript_event(job, event)
            }
            AppIntent::Jobs(intent) => {
                let event = match intent {
                    JobsIntent::LoadMore => job_list::Event::LoadMore,
                    JobsIntent::Refresh => job_list::Event::Refresh,
                    JobsIntent::DeleteRequested { job } => job_list::Event::DeleteRequested { job },
                    JobsIntent::DeleteConfirmed => job_list::Event::DeleteConfirmed,
                    JobsIntent::DeleteCanceled => job_list::Event::DeleteCanceled,
                    JobsIntent::DownloadRequested { job } => job_list::Event::DownloadRequested { job },
                };
                self.jobs.update(event).into_iter().map(|effect| AppEffect::Jobs { epoch, effect }).collect()
            }
            AppIntent::Settings(event) => {
                let capabilities = self.capabilities();
                self.settings.update(event, &capabilities);
                vec![]
            }
            AppIntent::ManageData(intent) => {
                let event = match intent {
                    ManageDataIntent::CleanupToggled { on } => data_management::Event::CleanupToggled { on },
                    ManageDataIntent::DeleteMonthlyToggled { on } => data_management::Event::DeleteMonthlyToggled { on },
                    ManageDataIntent::DeleteAllPressed => data_management::Event::DeleteAllPressed,
                    ManageDataIntent::DeleteAccountPressed => data_management::Event::DeleteAccountPressed,
                    ManageDataIntent::Confirmed => data_management::Event::Confirmed,
                    ManageDataIntent::Canceled => data_management::Event::Canceled,
                };
                let effects = self.manage_data.update(event);
                self.manage_data_effects(effects)
            }
            AppIntent::Feedback(intent) => {
                let event = match intent {
                    FeedbackIntent::Opened { job } => feedback::Event::Opened { job },
                    FeedbackIntent::Closed => feedback::Event::Closed,
                    FeedbackIntent::Selected { rating } => feedback::Event::Selected { rating },
                    FeedbackIntent::CommentChanged { text } => feedback::Event::CommentChanged { text },
                    FeedbackIntent::Submitted => feedback::Event::Submitted,
                };
                self.feedback.update(event).into_iter().map(|effect| AppEffect::Feedback { epoch, effect }).collect()
            }
            AppIntent::Support(intent) => {
                let event = match intent {
                    SupportIntent::Opened => support::Event::Opened,
                    SupportIntent::Closed => support::Event::Closed,
                    SupportIntent::MessageChanged { text } => support::Event::MessageChanged { text },
                    SupportIntent::Submitted => support::Event::Submitted { job: self.route.job() },
                };
                self.support.update(event).into_iter().map(|effect| AppEffect::Support { epoch, effect }).collect()
            }
            AppIntent::Consent(intent) => {
                let event = match intent {
                    ConsentIntent::EnableRequested => consent::Event::EnableRequested,
                    ConsentIntent::DeclineRequested => consent::Event::DeclineRequested,
                    ConsentIntent::DisableConfirmed => consent::Event::DisableConfirmed,
                    ConsentIntent::ErrorDismissed => consent::Event::ErrorDismissed,
                };
                self.consent_update(event)
            }
            AppIntent::Analysis(intent) => {
                let Some(job) = self.selected else { return vec![] };
                let phone = self.layouts().frame == Frame::Phone;
                let offset = self.utc_offset_minutes;
                let event = match intent {
                    AnalysisIntent::Shown => analysis::Event::Shown { key: Uuid::new_v4() },
                    AnalysisIntent::Hidden => analysis::Event::Hidden,
                    AnalysisIntent::RetryRequested => analysis::Event::RetryRequested { key: Uuid::new_v4() },
                    AnalysisIntent::CopyRequested => analysis::Event::CopyRequested { at: now, utc_offset_minutes: offset },
                    AnalysisIntent::CopySectionRequested { section } => analysis::Event::CopySectionRequested { section },
                    AnalysisIntent::ExportRequested { format } => analysis::Event::ExportRequested { format, at: now, utc_offset_minutes: offset, phone },
                    AnalysisIntent::ExportErrorDismissed => analysis::Event::ExportErrorDismissed,
                };
                let effects = self.analysis.update(event);
                self.analysis_effects(job, effects)
            }
            AppIntent::Chat(intent) => {
                let Some(job) = self.selected else { return vec![] };
                let event = match intent {
                    ChatIntent::SendRequested { text } => chat::Event::SendRequested { text, key: Uuid::new_v4() },
                    ChatIntent::RetryRequested { index } => chat::Event::RetryRequested { index, key: Uuid::new_v4() },
                    ChatIntent::CopyMessageRequested { index } => chat::Event::CopyMessageRequested { index },
                    ChatIntent::RateRequested { index, rating } => chat::Event::RateRequested { index, rating },
                    ChatIntent::CopyChatRequested => chat::Event::CopyChatRequested,
                    ChatIntent::DownloadRequested => chat::Event::DownloadRequested { at: now },
                    ChatIntent::ErrorDismissed => chat::Event::ErrorDismissed,
                    ChatIntent::NoticeDismissed => chat::Event::NoticeDismissed,
                };
                let effects = self.chat.update(event);
                self.chat_effects(job, effects)
            }
            AppIntent::PurchaseOpened => self.open_purchase(),
            AppIntent::PurchaseClosed => {
                self.purchase = None;
                vec![]
            }
            AppIntent::Purchase(intent) => {
                let event = match intent {
                    PurchaseIntent::SelectPayAsYouGo => PurchaseEvent::SelectPayAsYouGo,
                    PurchaseIntent::SelectPlan { package_id } => PurchaseEvent::SelectPlan { package_id },
                    PurchaseIntent::PlanSectionClicked => PurchaseEvent::PlanSectionClicked,
                    PurchaseIntent::IntervalChosen { yearly } => PurchaseEvent::IntervalChosen { yearly },
                    PurchaseIntent::PresetHours { hours } => PurchaseEvent::PresetHours(hours),
                    PurchaseIntent::HoursTyped { text } => PurchaseEvent::HoursTyped(text),
                    PurchaseIntent::StepHours { delta } => PurchaseEvent::StepHours(delta),
                    PurchaseIntent::CouponTyped { text } => PurchaseEvent::CouponTyped(text),
                    PurchaseIntent::CouponApplied => PurchaseEvent::CouponApplied,
                    PurchaseIntent::TermsAccepted { accepted } => PurchaseEvent::TermsAccepted(accepted),
                    PurchaseIntent::PayRequested => PurchaseEvent::PayRequested,
                };
                match self.purchase.as_mut() {
                    Some(purchase) => {
                        let effects = purchase.update(event, now);
                        self.purchase_effects(effects)
                    }
                    None => vec![],
                }
            }
            AppIntent::Subscription(intent) => {
                let event = match intent {
                    SubscriptionIntent::Refresh => SubscriptionEvent::Refresh,
                    SubscriptionIntent::UpgradeClicked => SubscriptionEvent::UpgradeClicked,
                    SubscriptionIntent::CancelClicked => SubscriptionEvent::CancelClicked,
                    SubscriptionIntent::CancelDismissed => SubscriptionEvent::CancelDismissed,
                    SubscriptionIntent::CancelConfirmed => SubscriptionEvent::CancelConfirmed,
                };
                let effects = self.subscription.update(event, now);
                self.subscription_effects(effects)
            }
            AppIntent::Invoices(intent) if self.route == Route::Billing => {
                if matches!(&intent, InvoicesIntent::Preview { .. } | InvoicesIntent::ClosePreview
                    | InvoicesIntent::LookupRequested | InvoicesIntent::LookupCleared | InvoicesIntent::TemporaryBillToTyped { .. })
                {
                    self.invalidate_invoice_preview();
                }
                let Some(state) = self.invoices.as_mut() else { return vec![] };
                let event = match intent {
                    InvoicesIntent::Preview { payment_id } => InvoicesEvent::Preview { payment_id },
                    InvoicesIntent::ClosePreview => InvoicesEvent::ClosePreview,
                    InvoicesIntent::Download { payment_id } => InvoicesEvent::Download { payment_id },
                    InvoicesIntent::LookupEmailTyped { text } => InvoicesEvent::LookupEmailTyped(text),
                    InvoicesIntent::LookupRequested => InvoicesEvent::LookupRequested,
                    InvoicesIntent::LookupCleared => InvoicesEvent::LookupCleared,
                    InvoicesIntent::TemporaryBillToTyped { text } => InvoicesEvent::TemporaryBillToTyped(text),
                };
                let effects = state.update(event);
                self.invoice_effects(effects)
            }
            AppIntent::Invoices(_) => vec![],
        }
    }

    // ---- view ----------------------------------------------------------------------------------

    fn locales(&self) -> Vec<LocaleOption> {
        LOCALE_SWITCHER
            .iter()
            .map(|(code, name, flag)| LocaleOption { code: (*code).to_string(), name: common(name), flag: (*flag).to_string(), current: *code == self.locale })
            .collect()
    }

    fn auth_vm(&self) -> AuthVm {
        let form = &self.auth;
        let notice = match (form.sent, form.mode) {
            (true, AuthMode::SignUp) => Some(auth("sign_up.confirmation_text")),
            (true, AuthMode::MagicLink) => Some(auth("magic_link.confirmation_text")),
            (true, AuthMode::ForgottenPassword) => Some(auth("forgotten_password.confirmation_text")),
            _ => None,
        };
        AuthVm {
            mode: form.mode,
            email: form.email.clone(),
            password: form.password.clone(),
            captcha_required: self.config.captcha,
            busy: form.busy,
            can_submit: !form.busy && (!self.config.captcha || form.captcha.is_some()),
            error: form.failure.map(AuthFailure::message),
            notice,
        }
    }

    fn new_password_vm(&self) -> NewPasswordVm {
        let f = &self.new_password;
        NewPasswordVm {
            password: f.password.clone(),
            busy: f.busy,
            error: f.failure.map(AuthFailure::message),
            success: f.done.then_some(PASSWORD_UPDATED),
        }
    }

    pub fn view(&self, now: Micros) -> AppVm {
        let locale = self.locale.to_string();
        if self.route == Route::CheckoutConfirm && self.phase != Phase::Erased {
            let vm = self.confirmation.as_ref().map(ConfirmPageState::view);
            let messages = vm.as_ref().map(billing::confirmation);
            return AppVm::CheckoutConfirm { locale, vm, messages };
        }
        match self.phase {
            Phase::Starting | Phase::Restoring => AppVm::Restoring { locale },
            Phase::Erased => AppVm::Erased { locale, message: ACCOUNT_ERASED },
            Phase::SignedOut => AppVm::SignedOut(SignedOutVm {
                locale,
                route: self.route,
                locales: self.locales(),
                auth: self.auth_vm(),
                new_password: self.new_password_vm(),
            }),
            Phase::SignedIn => {
                let selected = self.selected;
                let purchase = self.purchase.as_ref().map(PurchaseState::view);
                let subscription = self.subscription.view();
                let invoices = self.invoices.as_ref().map(InvoicesState::view);
                let billing_messages = billing::messages(purchase.as_ref(), &subscription, invoices.as_ref(), self.invoice_lookup_failed, now);
                AppVm::SignedIn(Box::new(SignedInVm {
                    locale,
                    route: self.route,
                    layouts: self.layouts(),
                    locales: self.locales(),
                    profile: self.profile.clone(),
                    joined_on_text: self.profile.as_ref().and_then(|p| p.joined_on).map(|at| dates::day_month_year(at, self.utc_offset_minutes)),
                    session_error: self.session_failed.then_some(SESSION_UNAVAILABLE),
                    capabilities: self.capabilities(),
                    balance: balance_text(self.balance.as_ref().and_then(|b| b.balance)),
                    header_title_text: selected.and(self.transcript.job.as_ref()).and_then(|j| j.name.clone()).filter(|n| !n.is_empty()),
                    header_title: match self.route {
                        Route::Home(HomeView::AllTranscripts) => app("All Transcripts"),
                        Route::Home(HomeView::Job(_)) => job_list::UNTITLED,
                        _ => app("My Transcripts"),
                    },
                    new_transcript_route: match self.layouts().frame {
                        Frame::Phone => Route::Home(HomeView::New),
                        Frame::Desktop => Route::Home(HomeView::Default),
                    },
                    signing_out: self.signing_out,
                    queue: self.queue.view(),
                    jobs: self.jobs.view(selected, self.utc_offset_minutes),
                    transcript: (selected.is_some() && self.transcript.job.is_some()).then(|| self.transcript.view()),
                    consent: self.consent.view(),
                    analysis: selected.map(|_| self.analysis.view(now)),
                    chat: selected.map(|_| self.chat.view(now)),
                    settings: self.profile.as_ref().map(|p| self.settings.view(p)),
                    manage_data: self.manage_data.view(),
                    feedback: self.feedback.view(),
                    support: self.support.view(),
                    new_password: self.new_password_vm(),
                    subscription,
                    purchase,
                    invoices,
                    billing_messages,
                    special_client_alert: self.balance.as_ref().and_then(|b| b.special_client_alert.clone()),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests;
