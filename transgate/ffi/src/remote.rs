//! The core view models and intents as UniFFI remote types (P8 D2).
//!
//! Hand-maintained. Each declaration repeats the shape of a core type that stays
//! UniFFI-free; **the compiler checks every one against its original**: a core field
//! or variant that is added, removed or retyped, or a newly reachable type with no
//! declaration here, fails the build of this crate. Types UniFFI cannot take as
//! plain remote records are converted in `custom.rs`.
//!
//! Ambiguous Rust names get one foreign name each (`use ... as ...`), because the
//! Kotlin and Swift namespace is flat: `AnalysisDecision`/`ConsentDecision`,
//! `ChatRating`, `DataManagementConfirmVm`/`BillingConfirmVm`, `ApiKeysIntent` and
//! `SettingsIntent` (Android's `Intent` must stay free).

use billing_core::decimal::Decimal;
use billing_core::types::{AccountId, Micros};
use transgate_core::ai::analysis::Rating as AnalysisRating;
use transgate_core::api::KeyId;
use transgate_core::ids::{JobId, LineId};
use transgate_core::languages::Language;
use transgate_core::presentation::msg::Msg;

use crate::custom::{Fade, HourPresets, PolicyOption, ScopeOption};

use transgate_core::ai::analysis::ActionItem;
use transgate_core::ai::analysis::ActionStatus;
use billing_core::presentation::invoices::AdminVm;
use transgate_core::presentation::upload_queue::Alert;
use transgate_core::ai::analysis::Analysis;
use transgate_core::ai::analysis::Decision as AnalysisDecision;
use transgate_core::presentation::app::AnalysisIntent;
use transgate_core::presentation::ai::analysis::AnalysisPanelVm;
use transgate_core::ai::analysis::AnalysisType;
use transgate_core::presentation::api_keys::Intent as ApiKeysIntent;
use transgate_core::presentation::api_keys::ApiKeysVm;
use transgate_core::presentation::app::AppIntent;
use transgate_core::presentation::app::AppVm;
use transgate_core::ai::analysis::Assessment;
use transgate_core::presentation::app::AuthIntent;
use transgate_core::presentation::app::AuthMode;
use transgate_core::presentation::app::AuthVm;
use billing_core::presentation::confirm::ConfirmVm as BillingConfirmVm;
use transgate_core::presentation::billing::BillingMessages;
use transgate_core::ai::analysis::CallMetrics;
use transgate_core::flags::Capabilities;
use billing_core::presentation::purchase::ChargedQuote;
use transgate_core::presentation::ai::chat::ChatErrorVm;
use transgate_core::presentation::app::ChatIntent;
use transgate_core::presentation::ai::chat::ChatLineVm;
use transgate_core::presentation::ai::chat::ChatPanelVm;
use transgate_core::presentation::ai::chat::Rating as ChatRating;
use transgate_core::ai::ports::ChatRole;
use transgate_core::presentation::upload_queue::Chip;
use transgate_core::presentation::upload_queue::Choice;
use transgate_core::ai::analysis::CoachingCategory;
use transgate_core::ai::analysis::CoachingTip;
use transgate_core::ai::analysis::Concept;
use transgate_core::presentation::data_management::Confirm;
use billing_core::presentation::confirm::ConfirmKind;
use transgate_core::presentation::billing::ConfirmMessages;
use transgate_core::presentation::ai::consent::Decision as ConsentDecision;
use transgate_core::presentation::app::ConsentIntent;
use transgate_core::presentation::ai::consent::ConsentPanelVm;
use transgate_core::presentation::api_keys::Copied;
use billing_core::pricing::CouponMessage;
use billing_core::pricing::Currency;
use transgate_core::ai::analysis::CustomerInsights;
use transgate_core::presentation::data_management::ConfirmVm as DataManagementConfirmVm;
use transgate_core::presentation::data_management::DataManagementVm;
use transgate_core::ai::analysis::DetectedType;
use transgate_core::documents::DocumentFormat;
use transgate_core::api::keys::ExpirationPolicy;
use transgate_core::presentation::api_keys::ExpiryLabel;
use transgate_core::presentation::app::FeedbackIntent;
use transgate_core::account::FeedbackRating;
use transgate_core::presentation::feedback::FeedbackVm;
use transgate_core::presentation::upload_queue::FileRow;
use transgate_core::presentation::layout::Frame;
use transgate_core::presentation::transcript::BadgeIcon;
use transgate_core::presentation::transcript::FreemiumBanner;
use transgate_core::presentation::ai::consent::Gate;
use transgate_core::ai::analysis::GenericAnalysis;
use transgate_core::presentation::route::HomeView;
use transgate_core::ai::analysis::InterviewAnalysis;
use transgate_core::ai::analysis::InterviewNextStep;
use billing_core::presentation::invoices::InvoiceRow;
use transgate_core::presentation::app::InvoicesIntent;
use billing_core::presentation::invoices::InvoicesVm;
use transgate_core::ai::analysis::IssueResolution;
use transgate_core::presentation::job_list::JobListVm;
use transgate_core::presentation::job_list::JobRow;
use transgate_core::jobs::domain::JobStatus;
use transgate_core::presentation::app::JobsIntent;
use transgate_core::presentation::api_keys::KeyFormVm;
use transgate_core::presentation::api_keys::KeyRowVm;
use transgate_core::presentation::upload_queue::LanguageOption;
use transgate_core::transcript::LanguageTab;
use transgate_core::presentation::layout::Layouts;
use transgate_core::ai::analysis::LectureAnalysis;
use transgate_core::presentation::ai::chat::LimitsVm;
use transgate_core::presentation::transcript::LineRow;
use transgate_core::presentation::app::LocaleOption;
use transgate_core::presentation::app::ManageDataIntent;
use transgate_core::presentation::transcript::Media;
use transgate_core::ai::analysis::MeetingAnalysis;
use transgate_core::presentation::upload_queue::Mode;
use transgate_core::presentation::app::NewPasswordIntent;
use transgate_core::presentation::app::NewPasswordVm;
use transgate_core::ai::analysis::NotableResponse;
use transgate_core::presentation::msg::Ns;
use transgate_core::presentation::ai::analysis::PanelState;
use billing_core::presentation::purchase::PaygCardView;
use transgate_core::presentation::upload_queue::PickedFile;
use billing_core::presentation::purchase::PlanCardView;
use billing_core::pricing::PlanInterval;
use billing_core::presentation::purchase::PlanOption;
use billing_core::presentation::subscription::PlanSummary;
use transgate_core::ai::analysis::Priority;
use transgate_core::presentation::settings::ProfileVm;
use transgate_core::presentation::ai::analysis::ProgressVm;
use billing_core::presentation::purchase::PurchaseFailure;
use transgate_core::presentation::app::PurchaseIntent;
use billing_core::presentation::purchase::PurchaseVm;
use transgate_core::presentation::feedback::RatingOption;
use transgate_core::presentation::job_list::RecentRow;
use billing_core::presentation::RenewalNotice;
use transgate_core::presentation::ai::chat::ResetLabel;
use transgate_core::ai::analysis::ResolutionStatus;
use transgate_core::ai::analysis::ResponseAssessment;
use transgate_core::presentation::route::Route;
use transgate_core::ai::analysis::SalesAnalysis;
use transgate_core::presentation::api_keys::ScopeChoice;
use transgate_core::presentation::api_keys::ScopeLabel;
use transgate_core::presentation::ai::section_copy::SectionId;
use transgate_core::ai::analysis::Sentiment;
use transgate_core::ai::analysis::SentimentKind;
use transgate_core::ai::analysis::SentimentMoment;
use transgate_core::account::SessionProfile;
use transgate_core::presentation::settings::Event as SettingsIntent;
use transgate_core::presentation::settings::SettingsVm;
use transgate_core::presentation::app::SignedInVm;
use transgate_core::presentation::app::SignedOutVm;
use transgate_core::ai::analysis::SkillAssessment;
use transgate_core::ai::analysis::SkillLevel;
use transgate_core::transcript::SpeakerLabel;
use transgate_core::presentation::transcript::SpeakerOption;
use billing_core::pricing::SpecialClientAlert;
use transgate_core::presentation::transcript::StatusBadge;
use billing_core::presentation::subscription::SubscriptionFailure;
use transgate_core::presentation::app::SubscriptionIntent;
use billing_core::presentation::subscription::SubscriptionVm;
use transgate_core::ai::analysis::SupportAnalysis;
use transgate_core::ai::analysis::SupportInsights;
use transgate_core::presentation::app::SupportIntent;
use transgate_core::presentation::support::SupportVm;
use transgate_core::presentation::settings::Tab;
use transgate_core::presentation::transcript::TabOption;
use transgate_core::presentation::settings::TabVm;
use transgate_core::presentation::job_list::Tone;
use transgate_core::ai::analysis::Topic;
use transgate_core::presentation::app::TranscriptIntent;
use transgate_core::presentation::layout::TranscriptLayout;
use transgate_core::presentation::transcript::TranscriptVm;
use transgate_core::transcript::TranslationStatus;
use transgate_core::presentation::app::UploadIntent;
use transgate_core::presentation::upload_queue::UploadQueueVm;
use transgate_core::presentation::ai::chat::UsageVm;
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::upload_queue::YouTubeDialog;
use transgate_core::presentation::layout::YouTubeLayout;

#[uniffi::remote(Record)]
pub struct ActionItem {
    pub task: String,
    pub owner: Option<String>,
    pub deadline: Option<String>,
    pub status: ActionStatus,
}

#[uniffi::remote(Enum)]
pub enum ActionStatus {
    Pending,
    InProgress,
    Done,
}

#[uniffi::remote(Record)]
pub struct AdminVm {
    pub email: String,
    pub looking_up: bool,
    pub viewing_customer: Option<String>,
    pub not_found: bool,
    pub temporary_bill_to: String,
}

#[uniffi::remote(Enum)]
pub enum Alert {
    Balance,
    Language,
    ProcessingTimeout,
    YouTube,
}

#[uniffi::remote(Enum)]
pub enum Analysis {
    Generic(GenericAnalysis),
    Lecture(LectureAnalysis),
    Meeting(MeetingAnalysis),
    Sales(SalesAnalysis),
    Support(SupportAnalysis),
    Interview(InterviewAnalysis),
}

#[uniffi::remote(Record)]
pub struct AnalysisDecision {
    pub decision: String,
    pub timestamp: Option<f64>,
    pub participants: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum AnalysisIntent {
    Shown,
    Hidden,
    RetryRequested,
    CopyRequested,
    CopySectionRequested { section: SectionId },
    ExportRequested { format: DocumentFormat },
    ExportErrorDismissed,
}

#[uniffi::remote(Record)]
pub struct AnalysisPanelVm {
    pub state: PanelState,
    pub no_transcript: bool,
    pub detected: Option<DetectedType>,
    pub summary: Option<String>,
    pub analysis: Option<Analysis>,
    pub has_data: bool,
    pub progress: Option<ProgressVm>,
    pub can_retry: bool,
    pub error_detail: Option<String>,
    pub error_hint: Option<Msg>,
    pub export_busy: Option<DocumentFormat>,
    pub export_error: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum AnalysisType {
    Generic,
    Lecture,
    Meeting,
    Sales,
    Support,
    Interview,
}

#[uniffi::remote(Enum)]
pub enum ApiKeysIntent {
    FormToggled,
    NameChanged { text: String },
    ScopeChosen { scope: ScopeChoice },
    PolicyChosen { policy: ExpirationPolicy },
    CustomDaysChanged { text: String },
    CreateRequested,
    CopyRequested { what: Copied },
    RevokeRequested { id: KeyId },
    RevokeConfirmed,
    RevokeCanceled,
    NoticeDismissed,
}

#[uniffi::remote(Record)]
pub struct ApiKeysVm {
    pub loading: bool,
    pub error: Option<Msg>,
    pub rows: Vec<KeyRowVm>,
    pub empty: bool,
    pub form_toggle: Msg,
    pub form: Option<KeyFormVm>,
    pub created_key: Option<String>,
    pub key_copied: bool,
    pub confirm_revoke: Option<KeyId>,
    pub secret_title: Msg,
    pub secret_description: Msg,
    pub secret_copy_label: Msg,
    pub signing_secret: Option<String>,
    pub secret_copied: bool,
    pub secret_error: Option<Msg>,
    pub notice: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum AppIntent {
    Started { url: String, viewport: Viewport, utc_offset_minutes: i16 },
    RouteChanged { url: String },
    ViewportChanged { viewport: Viewport },
    VisibilityChanged { visible: bool },
    Navigate { route: Route },
    LocaleChosen { code: String },
    SignOutRequested,
    SessionRetried,
    Auth(AuthIntent),
    NewPassword(NewPasswordIntent),
    Upload(UploadIntent),
    Transcript(TranscriptIntent),
    Jobs(JobsIntent),
    Settings(SettingsIntent),
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
    ApiKeys(ApiKeysIntent),
}

#[uniffi::remote(Enum)]
pub enum AppVm {
    Restoring { locale: String },
    SignedOut(SignedOutVm),
    SignedIn(Box<SignedInVm>),
    CheckoutConfirm { locale: String, vm: Option<BillingConfirmVm>, messages: Option<ConfirmMessages> },
    Erased { locale: String, message: Msg },
}

#[uniffi::remote(Record)]
pub struct Assessment {
    pub position: Option<String>,
    pub overall_rating: Option<AnalysisRating>,
    pub strengths: Option<Vec<String>>,
    pub concerns: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum AuthIntent {
    ModeChosen { mode: AuthMode },
    EmailChanged { text: String },
    PasswordChanged { text: String },
    CaptchaSolved { token: String },
    CaptchaExpired,
    Submitted,
    GoogleChosen,
}

#[uniffi::remote(Enum)]
pub enum AuthMode {
    SignIn,
    SignUp,
    MagicLink,
    ForgottenPassword,
}

#[uniffi::remote(Record)]
pub struct AuthVm {
    pub mode: AuthMode,
    pub email: String,
    pub password: String,
    pub captcha_required: bool,
    pub busy: bool,
    pub can_submit: bool,
    pub error: Option<Msg>,
    pub notice: Option<Msg>,
}

#[uniffi::remote(Record)]
pub struct BillingConfirmVm {
    pub kind: ConfirmKind,
    pub countdown: Option<u32>,
    pub payment_id: Option<i64>,
}

#[uniffi::remote(Record)]
pub struct BillingMessages {
    pub purchase_failure: Option<Msg>,
    pub coupon_notice: Option<Msg>,
    pub renewal: Option<Msg>,
    pub renewal_at_ms: Option<i64>,
    pub subscription_failure: Option<Msg>,
    pub invoice_failure: Option<Msg>,
    pub document_failure: Option<Msg>,
    pub customer_not_found: Option<Msg>,
}

#[uniffi::remote(Record)]
pub struct CallMetrics {
    pub talk_listen_ratio: Option<String>,
    pub questions_asked: Option<f64>,
    pub objections_handled: Option<f64>,
    pub next_steps_confirmed: Option<bool>,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct ChargedQuote {
    pub amount: Decimal,
    pub currency: Currency,
}

#[uniffi::remote(Record)]
pub struct ChatErrorVm {
    pub message: Option<Msg>,
    pub detail: Option<String>,
}

#[uniffi::remote(Enum)]
pub enum ChatIntent {
    SendRequested { text: String },
    RetryRequested { index: usize },
    CopyMessageRequested { index: usize },
    RateRequested { index: usize, rating: ChatRating },
    CopyChatRequested,
    DownloadRequested,
    ErrorDismissed,
    NoticeDismissed,
}

#[uniffi::remote(Record)]
pub struct ChatLineVm {
    pub role: ChatRole,
    pub content: String,
    pub streaming: bool,
    pub show_actions: bool,
}

#[uniffi::remote(Record)]
pub struct ChatPanelVm {
    pub enabled: bool,
    pub input_locked: bool,
    pub loading_history: bool,
    pub lines: Vec<ChatLineVm>,
    pub show_history_header: bool,
    pub show_welcome: bool,
    pub typing: bool,
    pub can_send: bool,
    pub limits: Option<LimitsVm>,
    pub error: Option<ChatErrorVm>,
    pub sync_warning: Option<Msg>,
    pub notice: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum ChatRating {
    Like,
    Dislike,
}

#[uniffi::remote(Enum)]
pub enum ChatRole {
    User,
    Assistant,
}

#[uniffi::remote(Enum)]
pub enum Chip {
    Uploading { progress: u8 },
    Preparing,
    Ready,
    UploadFailed,
}

#[uniffi::remote(Enum)]
pub enum Choice {
    AutoDetect,
    Language(Language),
}

#[uniffi::remote(Enum)]
pub enum CoachingCategory {
    Strength,
    Improvement,
}

#[uniffi::remote(Record)]
pub struct CoachingTip {
    pub category: CoachingCategory,
    pub tip: String,
}

#[uniffi::remote(Record)]
pub struct Concept {
    pub concept: String,
    pub explanation: String,
    pub examples: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum Confirm {
    DisableCleanup,
    DeleteMonthly,
    DeleteAll,
    DeleteAccount,
}

#[uniffi::remote(Enum)]
pub enum ConfirmKind {
    Succeeded,
    Failed,
    Unconfirmed,
}

#[uniffi::remote(Record)]
pub struct ConfirmMessages {
    pub title: Msg,
    pub status: Option<Msg>,
    pub message: Msg,
    pub detail: Option<Msg>,
    pub redirect: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum ConsentDecision {
    Enable,
    Decline,
    Disable,
}

#[uniffi::remote(Enum)]
pub enum ConsentIntent {
    EnableRequested,
    DeclineRequested,
    DisableConfirmed,
    ErrorDismissed,
}

#[uniffi::remote(Record)]
pub struct ConsentPanelVm {
    pub gate: Gate,
    pub has_consented: bool,
    pub can_revoke: bool,
    pub cleanup_pending: bool,
    pub consent_date: Option<Micros>,
    pub revoked_date: Option<Micros>,
    pub provider_name: Option<String>,
    pub consent_version: Option<String>,
    pub privacy_policy_url: String,
    pub busy: Option<ConsentDecision>,
    pub load_error: Option<Msg>,
    pub action_error: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum Copied {
    Key,
    Secret,
}

#[uniffi::remote(Enum)]
pub enum CouponMessage {
    NotValid,
    Expired,
    Error,
}

#[uniffi::remote(Enum)]
pub enum Currency {
    Usd,
    Try,
}

#[uniffi::remote(Record)]
pub struct CustomerInsights {
    pub pain_points: Option<Vec<String>>,
    pub budget_signals: Option<Vec<String>>,
    pub objections: Option<Vec<String>>,
    pub buying_signals: Option<Vec<String>>,
}

#[uniffi::remote(Record)]
pub struct DataManagementConfirmVm {
    pub kind: Confirm,
    pub title: Msg,
    pub description: Msg,
    pub confirm_label: Msg,
}

#[uniffi::remote(Record)]
pub struct DataManagementVm {
    pub privacy_info: Vec<Msg>,
    pub cleanup_policy: Msg,
    pub cleanup_on: bool,
    pub cleanup_warning: Option<Msg>,
    pub delete_monthly_on: bool,
    pub files_gone_note: Msg,
    pub confirm: Option<DataManagementConfirmVm>,
    pub erasure: Option<Msg>,
    pub error: Option<Msg>,
    pub save_error: Option<Msg>,
}

#[uniffi::remote(Record)]
pub struct DetectedType {
    pub kind: AnalysisType,
    pub confidence: f64,
}

#[uniffi::remote(Enum)]
pub enum DocumentFormat {
    Docx,
    Pdf,
    Txt,
    Md,
}

#[uniffi::remote(Enum)]
pub enum ExpirationPolicy {
    Never,
    Days30,
    Days90,
    OneYear,
    Custom,
}

#[uniffi::remote(Enum)]
pub enum ExpiryLabel {
    Never,
    Expired,
    Days { days: i64 },
}

#[uniffi::remote(Enum)]
pub enum FeedbackIntent {
    Opened { job: JobId },
    Closed,
    Selected { rating: FeedbackRating },
    CommentChanged { text: String },
    Submitted,
}

#[uniffi::remote(Enum)]
pub enum FeedbackRating {
    Disappointed,
    Satisfied,
    Outstanding,
}

#[uniffi::remote(Record)]
pub struct FeedbackVm {
    pub open: bool,
    pub thanked: bool,
    pub options: Vec<RatingOption>,
    pub comment: String,
    pub submitting: bool,
    pub can_submit: bool,
    pub notice: Option<Msg>,
}

#[uniffi::remote(Record)]
pub struct FileRow {
    pub id: u64,
    pub name: String,
    pub size: i64,
    pub desktop_size: String,
    pub phone_size: String,
    pub chip: Chip,
    pub label: Msg,
    pub phone_label: Msg,
    pub progress: u8,
    pub duration: Option<i32>,
    pub duration_text: Option<Msg>,
    pub youtube: bool,
}

#[uniffi::remote(Enum)]
pub enum Frame {
    Desktop,
    Phone,
}

#[uniffi::remote(Record)]
pub struct FreemiumBanner {
    pub current_minutes: i64,
    pub max_minutes: i64,
    pub percent: u8,
}

#[uniffi::remote(Enum)]
pub enum Gate {
    Loading,
    Required,
    Granted,
}

#[uniffi::remote(Record)]
pub struct GenericAnalysis {
    pub summary: Option<String>,
    pub highlights: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum HomeView {
    Default,
    New,
    AllTranscripts,
    Job(JobId),
}

#[uniffi::remote(Record)]
pub struct InterviewAnalysis {
    pub assessment: Option<Assessment>,
    pub skills_assessment: Option<Vec<SkillAssessment>>,
    pub notable_responses: Option<Vec<NotableResponse>>,
    pub next_steps: Option<Vec<InterviewNextStep>>,
}

#[uniffi::remote(Record)]
pub struct InterviewNextStep {
    pub action: String,
    pub priority: Priority,
}

#[uniffi::remote(Record)]
pub struct InvoiceRow {
    pub payment_id: i64,
    pub invoice_id: String,
    pub date: String,
    pub amount_text: String,
    pub package_name: String,
    pub gateway: String,
}

#[uniffi::remote(Enum)]
pub enum InvoicesIntent {
    Preview { payment_id: i64 },
    ClosePreview,
    Download { payment_id: i64 },
    LookupEmailTyped { text: String },
    LookupRequested,
    LookupCleared,
    TemporaryBillToTyped { text: String },
}

#[uniffi::remote(Record)]
pub struct InvoicesVm {
    pub loading: bool,
    pub rows: Vec<InvoiceRow>,
    pub preview: Option<i64>,
    pub admin: Option<AdminVm>,
    pub failure: Option<PurchaseFailure>,
    pub document_failure: Option<PurchaseFailure>,
}

#[uniffi::remote(Record)]
pub struct IssueResolution {
    pub problem: String,
    pub root_cause: Option<String>,
    pub resolution: Option<String>,
    pub status: ResolutionStatus,
    pub resolution_time_minutes: Option<f64>,
}

#[uniffi::remote(Record)]
pub struct JobListVm {
    pub rows: Vec<JobRow>,
    pub can_load_more: bool,
    pub initial_loading: bool,
    pub loading_more: bool,
    pub end_of_list: Option<Msg>,
    pub error: Option<Msg>,
    pub error_detail: Option<String>,
    pub confirm_delete: Option<JobId>,
    pub empty: bool,
    pub not_available: Msg,
    pub untitled: Msg,
    pub auto_detect: Msg,
    pub recent: Vec<RecentRow>,
    pub recent_empty: Msg,
}

#[uniffi::remote(Record)]
pub struct JobRow {
    pub id: JobId,
    pub display_name: Option<String>,
    pub status: JobStatus,
    pub status_label: Msg,
    pub tone: Tone,
    pub spinner: bool,
    pub is_translation: bool,
    pub can_download: bool,
    pub can_feedback: bool,
    pub duration: Option<String>,
    pub speakers: Option<Msg>,
    pub language: Option<String>,
    pub flag: Option<String>,
    pub date_text: String,
    pub created_at_ms: i64,
    pub selected: bool,
    pub downloading: bool,
    pub sidebar_name: String,
    pub sidebar_feedback: bool,
}

#[uniffi::remote(Enum)]
pub enum JobStatus {
    Draft,
    NotStarted,
    Pending,
    Running,
    Completed,
    Failed,
    Canceled,
    Deleted,
    SpeakerLabeling,
    Uploaded,
    Processing,
    Ready,
}

#[uniffi::remote(Enum)]
pub enum JobsIntent {
    LoadMore,
    Refresh,
    DeleteRequested { job: JobId },
    DeleteConfirmed,
    DeleteCanceled,
    DownloadRequested { job: JobId },
}

#[uniffi::remote(Record)]
pub struct KeyFormVm {
    pub name: String,
    pub scopes: Vec<ScopeOption>,
    pub policies: Vec<PolicyOption>,
    pub custom_days: Option<String>,
    pub custom_days_invalid: bool,
    pub can_create: bool,
    pub creating: bool,
}

#[uniffi::remote(Record)]
pub struct KeyRowVm {
    pub id: KeyId,
    pub name: String,
    pub scopes: Vec<ScopeLabel>,
    pub created_at_ms: i64,
    pub expiry: ExpiryLabel,
    pub expiry_tone: Tone,
    pub active: bool,
    pub status: Msg,
    pub status_tone: Tone,
    pub can_revoke: bool,
    pub revoking: bool,
}

#[uniffi::remote(Record)]
pub struct LanguageOption {
    pub language: Language,
    pub name: String,
    pub flag: String,
}

#[uniffi::remote(Record)]
pub struct LanguageTab {
    pub language: Language,
    pub translation_status: TranslationStatus,
}

#[uniffi::remote(Record)]
pub struct Layouts {
    pub frame: Frame,
    pub transcript: TranscriptLayout,
    pub youtube: YouTubeLayout,
    pub phone_ai: bool,
}

#[uniffi::remote(Record)]
pub struct LectureAnalysis {
    pub topics: Option<Vec<Topic>>,
    pub concepts: Option<Vec<Concept>>,
    pub review_questions: Option<Vec<String>>,
    pub study_guide: Option<Vec<String>>,
}

#[uniffi::remote(Record)]
pub struct LimitsVm {
    pub questions: Option<UsageVm>,
    pub tokens: Option<UsageVm>,
    pub reset: Option<ResetLabel>,
}

#[uniffi::remote(Record)]
pub struct LineRow {
    pub id: LineId,
    pub start_time: String,
    pub end_time: String,
    pub time_text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    pub speaker: Option<SpeakerLabel>,
    pub speaker_value: Option<String>,
    pub speaker_pending: bool,
    pub editing: bool,
}

#[uniffi::remote(Record)]
pub struct LocaleOption {
    pub code: String,
    pub name: Msg,
    pub flag: String,
    pub current: bool,
}

#[uniffi::remote(Enum)]
pub enum ManageDataIntent {
    CleanupToggled { on: bool },
    DeleteMonthlyToggled { on: bool },
    DeleteAllPressed,
    DeleteAccountPressed,
    Confirmed,
    Canceled,
}

#[uniffi::remote(Enum)]
pub enum Media {
    None,
    Audio { url: Option<String>, loading: bool, unavailable: bool },
    YouTube { video_id: String },
}

#[uniffi::remote(Record)]
pub struct MeetingAnalysis {
    pub action_items: Option<Vec<ActionItem>>,
    pub decisions: Option<Vec<AnalysisDecision>>,
    pub discussion_points: Option<Vec<String>>,
    pub next_steps: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum Mode {
    Transcription,
    Translation,
}

#[uniffi::remote(Enum)]
pub enum NewPasswordIntent {
    PasswordChanged { text: String },
    Submitted,
}

#[uniffi::remote(Record)]
pub struct NewPasswordVm {
    pub password: String,
    pub busy: bool,
    pub error: Option<Msg>,
    pub success: Option<Msg>,
}

#[uniffi::remote(Record)]
pub struct NotableResponse {
    pub question: String,
    pub answer: String,
    pub assessment: ResponseAssessment,
}

#[uniffi::remote(Enum)]
pub enum Ns {
    Common,
    App,
    Ai,
    Auth,
}

#[uniffi::remote(Enum)]
pub enum PanelState {
    Idle,
    CheckingCache,
    Loading,
    Ready,
    Interrupted,
    Cancelled,
    Error,
}

#[uniffi::remote(Record)]
pub struct PaygCardView {
    pub price_text: String,
    pub struck_per_hour: Decimal,
}

#[uniffi::remote(Record)]
pub struct PickedFile {
    pub handle: u64,
    pub name: String,
    pub size: i64,
    pub mime: String,
}

#[uniffi::remote(Record)]
pub struct PlanCardView {
    pub name: String,
    pub interval: PlanInterval,
    pub usd_per_month: Decimal,
    pub struck_per_month: Decimal,
    pub badge: String,
}

#[uniffi::remote(Enum)]
pub enum PlanInterval {
    Monthly,
    Yearly,
}

#[uniffi::remote(Record)]
pub struct PlanOption {
    pub package_id: usize,
    pub card: PlanCardView,
    pub selected: bool,
}

#[uniffi::remote(Record)]
pub struct PlanSummary {
    pub name: String,
    pub minutes_included: i64,
    pub valid_until: Micros,
}

#[uniffi::remote(Enum)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[uniffi::remote(Record)]
pub struct ProfileVm {
    pub email: String,
    pub display_name: String,
    pub joined_on: Option<Micros>,
}

#[uniffi::remote(Record)]
pub struct ProgressVm {
    pub stage: Msg,
    pub percentage: u8,
}

#[uniffi::remote(Enum)]
pub enum PurchaseFailure {
    Unauthenticated,
    Erased,
    Unavailable,
    Refused { code: String },
}

#[uniffi::remote(Enum)]
pub enum PurchaseIntent {
    SelectPayAsYouGo,
    SelectPlan { package_id: usize },
    PlanSectionClicked,
    IntervalChosen { yearly: bool },
    PresetHours { hours: i64 },
    HoursTyped { text: String },
    StepHours { delta: i64 },
    CouponTyped { text: String },
    CouponApplied,
    TermsAccepted { accepted: bool },
    PayRequested,
}

#[uniffi::remote(Record)]
pub struct PurchaseVm {
    pub plans: Vec<PlanOption>,
    pub pay_as_you_go: Option<PaygCardView>,
    pub pay_as_you_go_selected: bool,
    pub yearly: bool,
    pub hours: i64,
    pub hour_presets: HourPresets,
    pub minimum_warning: bool,
    pub coupon_code: String,
    pub coupon_locked: bool,
    pub coupon_notice: Option<CouponMessage>,
    pub terms_accepted: bool,
    pub display_total: Option<Decimal>,
    pub charged_quote: Option<ChargedQuote>,
    pub try_display: Option<Decimal>,
    pub renewal: RenewalNotice,
    pub quote_pending: bool,
    pub processing: bool,
    pub can_pay: bool,
    pub failure: Option<PurchaseFailure>,
}

#[uniffi::remote(Record)]
pub struct RatingOption {
    pub rating: FeedbackRating,
    pub label: Msg,
    pub selected: bool,
}

#[uniffi::remote(Record)]
pub struct RecentRow {
    pub id: JobId,
    pub file_name: Option<String>,
    pub duration: Option<String>,
    pub created_at_ms: i64,
    pub selected: bool,
}

#[uniffi::remote(Enum)]
pub enum RenewalNotice {
    InDays(u32),
    WillNotRenew,
}

#[uniffi::remote(Enum)]
pub enum ResetLabel {
    LessThanHour,
    InHours { hours: i64 },
    InDays { days: i64 },
    On { at: Micros },
}

#[uniffi::remote(Enum)]
pub enum ResolutionStatus {
    Resolved,
    Unresolved,
    Escalated,
}

#[uniffi::remote(Enum)]
pub enum ResponseAssessment {
    Positive,
    Neutral,
    Negative,
}

#[uniffi::remote(Enum)]
pub enum Route {
    Home(HomeView),
    SignIn,
    NewPassword,
    Billing,
    CheckoutConfirm,
    ApiKeys,
}

#[uniffi::remote(Record)]
pub struct SalesAnalysis {
    pub call_metrics: Option<CallMetrics>,
    pub coaching_tips: Option<Vec<CoachingTip>>,
    pub customer_insights: Option<CustomerInsights>,
    pub next_steps: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum ScopeChoice {
    Read,
    Create,
    ReadAndCreate,
}

#[uniffi::remote(Enum)]
pub enum ScopeLabel {
    Known { label: Msg },
    Raw { value: String },
}

#[uniffi::remote(Enum)]
pub enum SectionId {
    ActionItems,
    Decisions,
    DiscussionPoints,
    NextSteps,
    Topics,
    Concepts,
    ReviewQuestions,
    StudyGuide,
    CallMetrics,
    CoachingTips,
    CustomerInsights,
    SalesNextSteps,
    KbUpdates,
    FollowUpActions,
    SupportInsights,
    CandidateAssessment,
    SkillsAssessment,
    NotableResponses,
    InterviewNextSteps,
}

#[uniffi::remote(Record)]
pub struct Sentiment {
    pub initial: String,
    pub r#final: String,
    pub key_moments: Option<Vec<SentimentMoment>>,
}

#[uniffi::remote(Enum)]
pub enum SentimentKind {
    Frustrated,
    Neutral,
    Satisfied,
    Relieved,
}

#[uniffi::remote(Record)]
pub struct SentimentMoment {
    pub timestamp: f64,
    pub sentiment: SentimentKind,
    pub reason: Option<String>,
}

#[uniffi::remote(Record)]
pub struct SessionProfile {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    pub display_name: String,
    pub joined_on: Option<Micros>,
    pub capabilities: Capabilities,
}

#[uniffi::remote(Enum)]
pub enum SettingsIntent {
    Opened,
    Closed,
    TabSelected { tab: Tab },
}

#[uniffi::remote(Record)]
pub struct SettingsVm {
    pub open: bool,
    pub title: Msg,
    pub tabs: Vec<TabVm>,
    pub active: Tab,
    pub profile: ProfileVm,
}

#[uniffi::remote(Record)]
pub struct SignedInVm {
    pub locale: String,
    pub route: Route,
    pub layouts: Layouts,
    pub locales: Vec<LocaleOption>,
    pub profile: Option<SessionProfile>,
    pub joined_on_text: Option<String>,
    pub session_error: Option<Msg>,
    pub capabilities: Capabilities,
    pub balance: Msg,
    pub header_title_text: Option<String>,
    pub header_title: Msg,
    pub new_transcript_route: Route,
    pub signing_out: bool,
    pub queue: UploadQueueVm,
    pub jobs: JobListVm,
    pub transcript: Option<TranscriptVm>,
    pub consent: ConsentPanelVm,
    pub analysis: Option<AnalysisPanelVm>,
    pub chat: Option<ChatPanelVm>,
    pub settings: Option<SettingsVm>,
    pub manage_data: DataManagementVm,
    pub feedback: FeedbackVm,
    pub support: SupportVm,
    pub new_password: NewPasswordVm,
    pub subscription: SubscriptionVm,
    pub purchase: Option<PurchaseVm>,
    pub invoices: Option<InvoicesVm>,
    pub api_keys: Option<ApiKeysVm>,
    pub billing_messages: BillingMessages,
    pub special_client_alert: Option<SpecialClientAlert>,
}

#[uniffi::remote(Record)]
pub struct SignedOutVm {
    pub locale: String,
    pub route: Route,
    pub locales: Vec<LocaleOption>,
    pub auth: AuthVm,
    pub new_password: NewPasswordVm,
}

#[uniffi::remote(Record)]
pub struct SkillAssessment {
    pub skill: String,
    pub rating: AnalysisRating,
    pub level: SkillLevel,
}

#[uniffi::remote(Enum)]
pub enum SkillLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

#[uniffi::remote(Enum)]
pub enum SpeakerLabel {
    Numbered { number: i64 },
    Named { name: String },
    None,
}

#[uniffi::remote(Record)]
pub struct SpeakerOption {
    pub value: String,
    pub label: SpeakerLabel,
}

#[uniffi::remote(Record)]
pub struct SpecialClientAlert {
    pub code: String,
    pub percent: u32,
    pub message: String,
}

#[uniffi::remote(Record)]
pub struct StatusBadge {
    pub label: Msg,
    pub status: JobStatus,
    pub tone: Tone,
    pub icon: Option<BadgeIcon>,
    pub spinning: bool,
}

#[uniffi::remote(Enum)]
pub enum BadgeIcon {
    Dot,
    Upload,
    Check,
    Cross,
}

#[uniffi::remote(Enum)]
pub enum SubscriptionFailure {
    NoActiveSubscription,
    Request(PurchaseFailure),
}

#[uniffi::remote(Enum)]
pub enum SubscriptionIntent {
    Refresh,
    UpgradeClicked,
    CancelClicked,
    CancelDismissed,
    CancelConfirmed,
}

#[uniffi::remote(Record)]
pub struct SubscriptionVm {
    pub loading: bool,
    pub plan: Option<PlanSummary>,
    pub next_payment: Option<Micros>,
    pub canceled: bool,
    pub show_cancel: bool,
    pub confirming: bool,
    pub cancelling: bool,
    pub failure: Option<SubscriptionFailure>,
}

#[uniffi::remote(Record)]
pub struct SupportAnalysis {
    pub issue_resolution: Option<IssueResolution>,
    pub sentiment: Option<Sentiment>,
    pub insights: Option<SupportInsights>,
    pub kb_updates: Option<Vec<String>>,
    pub follow_up_actions: Option<Vec<String>>,
}

#[uniffi::remote(Record)]
pub struct SupportInsights {
    pub strengths: Option<Vec<String>>,
    pub improvements: Option<Vec<String>>,
}

#[uniffi::remote(Enum)]
pub enum SupportIntent {
    Opened,
    Closed,
    MessageChanged { text: String },
    Submitted,
}

#[uniffi::remote(Record)]
pub struct SupportVm {
    pub open: bool,
    pub message: String,
    pub sending: bool,
    pub notice: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum Tab {
    Profile,
    AiFeatures,
    Subscription,
    ManageData,
}

#[uniffi::remote(Record)]
pub struct TabOption {
    pub tab: LanguageTab,
    pub name: String,
    pub flag: String,
}

#[uniffi::remote(Record)]
pub struct TabVm {
    pub tab: Tab,
    pub label: Msg,
    pub active: bool,
}

#[uniffi::remote(Enum)]
pub enum Tone {
    Neutral,
    Progress,
    Accent,
    Success,
    Danger,
}

#[uniffi::remote(Record)]
pub struct Topic {
    pub title: String,
    pub timestamp: Option<f64>,
    pub duration: Option<f64>,
}

#[uniffi::remote(Enum)]
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

#[uniffi::remote(Enum)]
pub enum TranscriptLayout {
    Split,
    Tabbed,
}

#[uniffi::remote(Record)]
pub struct TranscriptVm {
    pub job: Option<JobId>,
    pub title: String,
    pub duration_minutes: Option<i32>,
    pub minutes_unit: Msg,
    pub status: Option<JobStatus>,
    pub status_badge: Option<StatusBadge>,
    pub is_translation: bool,
    pub translation_flag: Option<String>,
    pub loading_message: Option<Msg>,
    pub empty: Option<Msg>,
    pub load_error: Option<Msg>,
    pub load_error_detail: Option<String>,
    pub can_copy: bool,
    pub rows: Vec<LineRow>,
    pub fades: Vec<Fade>,
    pub upgrade_banner: bool,
    pub freemium: Option<FreemiumBanner>,
    pub show_tabs: bool,
    pub tabs: Vec<TabOption>,
    pub selected: Option<LanguageTab>,
    pub lines_tab_label: Msg,
    pub lines_tab_flag: Option<String>,
    pub rtl: bool,
    pub speakers: Vec<String>,
    pub speaker_options: Vec<SpeakerOption>,
    pub time_toggle: bool,
    pub speaker_toggle: bool,
    pub phone_speaker_toggle: bool,
    pub show_time: bool,
    pub show_speakers: bool,
    pub download_enabled: bool,
    pub media: Media,
    pub no_audio_alert: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum TranslationStatus {
    Original,
    Translation,
}

#[uniffi::remote(Enum)]
pub enum UploadIntent {
    Picked { files: Vec<PickedFile> },
    Dropped { files: Vec<PickedFile> },
    Removed { file: u64 },
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

#[uniffi::remote(Record)]
pub struct UploadQueueVm {
    pub rows: Vec<FileRow>,
    pub mode: Mode,
    pub language: Option<Choice>,
    pub source_language: Option<Language>,
    pub target_language: Option<Language>,
    pub speakers: bool,
    pub auto_detect_offered: bool,
    pub auto_detect_label: Msg,
    pub preferred: Vec<LanguageOption>,
    pub available: Vec<LanguageOption>,
    pub source_languages: Vec<LanguageOption>,
    pub target_languages: Vec<LanguageOption>,
    pub preferred_targets: Vec<LanguageOption>,
    pub phone_preferred: Vec<LanguageOption>,
    pub start_disabled: bool,
    pub files_ready: bool,
    pub transcribing: bool,
    pub max_files_reached: bool,
    pub picker_accept: String,
    pub show_language_selector: bool,
    pub total_minutes: Option<i64>,
    pub alerts: Vec<Alert>,
    pub youtube_error: Option<Msg>,
    pub youtube_dialog: YouTubeDialog,
    pub initializing: bool,
}

#[uniffi::remote(Record)]
pub struct UsageVm {
    pub current: i64,
    pub max: i64,
    pub width_percent: u8,
    pub tone: Tone,
}

#[uniffi::remote(Record)]
pub struct Viewport {
    pub touch: bool,
    pub width: u32,
}

#[uniffi::remote(Record)]
pub struct YouTubeDialog {
    pub open: bool,
    pub url: String,
    pub busy: bool,
    pub error: Option<Msg>,
}

#[uniffi::remote(Enum)]
pub enum YouTubeLayout {
    Desktop,
    Touch,
}
