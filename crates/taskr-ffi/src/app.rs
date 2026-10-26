//! The signed-in app for Kotlin and Swift: one `AppPresenter`, intents in,
//! one `AppView` out, exactly like the web's `AppPresenter`. The session is
//! kept by the platform (Keystore-backed storage, the Keychain) through
//! `SessionVault`.
//!
//! Threading: `dispatch` and `dispatch_with` block while they talk to the
//! server, so shells call them off the main thread, like the local presenter.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use taskr_client::{DevAuth, HttpApi, SupabaseAuth};
use taskr_core::session::app::{AuthClient, Session, SessionStore};
use taskr_core::session::presentation::{
    AppController, AppIntent as CoreIntent, AppVm, Screen, ScreenVm, SignInVm, SignedInVm,
};
use taskr_core::shared::access::{Role, WorkspaceId};
use taskr_core::shared::telemetry::{LogEvent, Telemetry};
use taskr_core::workspaces::presentation::{MembersIntent as CoreMembersIntent, MembersVm};
use taskr_infra::clock::SystemClock;

use crate::{ForeignTelemetry, LogSink, TaskIntent, TaskListView, to_intent, to_view};

/// Implemented in Kotlin and Swift: where the session lives between launches.
/// Values are opaque strings; the platform only stores them securely.
#[uniffi::export(callback_interface)]
pub trait SessionVault: Send + Sync {
    fn load(&self) -> Option<String>;
    fn save(&self, value: String);
    fn clear(&self);
}

/// Implemented in Kotlin and Swift: receives the views `dispatch_with` shows
/// while requests run (e.g. the new workspace loading right after it was
/// selected), so the previous screen does not stay interactive meanwhile.
/// Called on the thread that called `dispatch_with`.
#[uniffi::export(callback_interface)]
pub trait AppViewListener: Send + Sync {
    fn on_view(&self, view: AppView);
}

#[derive(Serialize, Deserialize)]
struct StoredSession {
    access_token: String,
    refresh_token: Option<String>,
    email: String,
    expires_at: Option<i64>,
}

pub(crate) struct VaultStore(pub(crate) Box<dyn SessionVault>);

impl SessionStore for VaultStore {
    fn load(&self) -> Option<Session> {
        let s: StoredSession = serde_json::from_str(&self.0.load()?).ok()?;
        Some(Session {
            access_token: s.access_token,
            refresh_token: s.refresh_token,
            email: s.email,
            expires_at: s.expires_at,
        })
    }

    fn save(&self, s: &Session) {
        let stored = StoredSession {
            access_token: s.access_token.clone(),
            refresh_token: s.refresh_token.clone(),
            email: s.email.clone(),
            expires_at: s.expires_at,
        };
        self.0.save(serde_json::to_string(&stored).expect("plain struct"));
    }

    fn clear(&self) {
        self.0.clear();
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct AppConfig {
    /// API root, e.g. `http://192.168.240.1:8787/api` from Waydroid.
    pub api_url: String,
    /// Supabase Auth root. Without it the app uses dev sign-in.
    pub auth_url: Option<String>,
    pub auth_key: Option<String>,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRole {
    Owner,
    Editor,
    Viewer,
}

impl From<MemberRole> for Role {
    fn from(r: MemberRole) -> Role {
        match r {
            MemberRole::Owner => Role::Owner,
            MemberRole::Editor => Role::Editor,
            MemberRole::Viewer => Role::Viewer,
        }
    }
}

impl From<Role> for MemberRole {
    fn from(r: Role) -> MemberRole {
        match r {
            Role::Owner => MemberRole::Owner,
            Role::Editor => MemberRole::Editor,
            Role::Viewer => MemberRole::Viewer,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppScreen {
    Tasks,
    Members,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum MembersIntent {
    Opened,
    AddSubmitted { email: String, role: MemberRole },
    RoleChanged { email: String, role: MemberRole },
    RemoveRequested { email: String },
    MessageDismissed,
}

/// Screen intents (`Tasks`, `Members`) carry `SignedInView::workspace` of the
/// view that showed the screen.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum AppIntent {
    Started,
    SignInSubmitted { email: String, password: String },
    SignUpSubmitted { email: String, password: String },
    SignOutRequested,
    WorkspaceSelected { id: String },
    ScreenSelected { screen: AppScreen },
    MessageDismissed,
    Tasks { workspace: String, intent: TaskIntent },
    Members { workspace: String, intent: MembersIntent },
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SignInView {
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceNavView {
    pub id: String,
    pub name: String,
    pub role_label: String,
    pub selected: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ScreenNavView {
    pub screen: AppScreen,
    pub label: String,
    pub selected: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MemberRowView {
    pub email: String,
    pub role: MemberRole,
    pub role_label: String,
    pub is_you: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct RoleOptionView {
    pub role: MemberRole,
    pub label: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MembersView {
    pub rows: Vec<MemberRowView>,
    pub can_manage: bool,
    pub role_options: Vec<RoleOptionView>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum ScreenView {
    Loading,
    Tasks { view: TaskListView },
    Members { view: MembersView },
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SignedInView {
    pub email: String,
    pub workspaces: Vec<WorkspaceNavView>,
    /// The open workspace; sent back with `AppIntent::Tasks` and `AppIntent::Members`.
    pub workspace: Option<String>,
    pub screens: Vec<ScreenNavView>,
    pub title: String,
    pub error: Option<String>,
    pub screen: ScreenView,
}

#[allow(clippy::large_enum_variant)]
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum AppView {
    Starting,
    SignedOut { view: SignInView },
    SignedIn { view: SignedInView },
}

#[derive(uniffi::Object)]
pub struct AppPresenter {
    controller: AppController,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl AppPresenter {
    #[uniffi::constructor]
    pub fn new(config: AppConfig, vault: Box<dyn SessionVault>, sink: Box<dyn LogSink>) -> Self {
        let telemetry: Arc<dyn Telemetry> = Arc::new(ForeignTelemetry(sink));
        telemetry.record(
            LogEvent::info("app.started")
                .field("shell", "mobile")
                .field("auth", if config.auth_url.is_some() { "supabase" } else { "dev" }),
        );
        let sessions = Arc::new(VaultStore(vault));
        let api = Arc::new(HttpApi::new(config.api_url, sessions.clone()));
        let auth: Arc<dyn AuthClient> = match config.auth_url {
            Some(url) => Arc::new(SupabaseAuth::new(url, config.auth_key)),
            None => Arc::new(DevAuth),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a one-thread runtime starts");
        let controller =
            AppController::new(auth, sessions, api.clone(), api, Arc::new(SystemClock));
        Self { controller, runtime }
    }

    pub fn dispatch(&self, intent: AppIntent) -> AppView {
        to_app_view(self.runtime.block_on(self.controller.dispatch(to_app_intent(intent))))
    }

    /// `dispatch`, also handing `listener` each view shown while requests run.
    pub fn dispatch_with(&self, intent: AppIntent, listener: Box<dyn AppViewListener>) -> AppView {
        let on_view = |vm| listener.on_view(to_app_view(vm));
        to_app_view(
            self.runtime.block_on(self.controller.dispatch_with(to_app_intent(intent), on_view)),
        )
    }

    pub fn current(&self) -> AppView {
        to_app_view(self.controller.view())
    }
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AppPreviewScreen {
    pub name: String,
    pub view: AppView,
}

/// Signed-in fixtures, by name, for SwiftUI and Compose previews.
#[uniffi::export]
pub fn app_preview_screens() -> Vec<AppPreviewScreen> {
    taskr_core::session::presentation::fixtures::all()
        .into_iter()
        .map(|(name, vm)| AppPreviewScreen { name: name.to_string(), view: to_app_view(vm) })
        .collect()
}

fn to_members_intent(i: MembersIntent) -> CoreMembersIntent {
    match i {
        MembersIntent::Opened => CoreMembersIntent::Opened,
        MembersIntent::AddSubmitted { email, role } => {
            CoreMembersIntent::AddSubmitted { email, role: role.into() }
        }
        MembersIntent::RoleChanged { email, role } => {
            CoreMembersIntent::RoleChanged { email, role: role.into() }
        }
        MembersIntent::RemoveRequested { email } => CoreMembersIntent::RemoveRequested { email },
        MembersIntent::MessageDismissed => CoreMembersIntent::MessageDismissed,
    }
}

fn to_app_intent(i: AppIntent) -> CoreIntent {
    match i {
        AppIntent::Started => CoreIntent::Started,
        AppIntent::SignInSubmitted { email, password } => {
            CoreIntent::SignInSubmitted { email, password }
        }
        AppIntent::SignUpSubmitted { email, password } => {
            CoreIntent::SignUpSubmitted { email, password }
        }
        AppIntent::SignOutRequested => CoreIntent::SignOutRequested,
        AppIntent::WorkspaceSelected { id } => CoreIntent::WorkspaceSelected(WorkspaceId(id)),
        AppIntent::ScreenSelected { screen } => CoreIntent::ScreenSelected(match screen {
            AppScreen::Tasks => Screen::Tasks,
            AppScreen::Members => Screen::Members,
        }),
        AppIntent::MessageDismissed => CoreIntent::MessageDismissed,
        AppIntent::Tasks { workspace, intent } => {
            CoreIntent::Tasks(WorkspaceId(workspace), to_intent(intent))
        }
        AppIntent::Members { workspace, intent } => {
            CoreIntent::Members(WorkspaceId(workspace), to_members_intent(intent))
        }
    }
}

fn to_members_view(vm: MembersVm) -> MembersView {
    MembersView {
        rows: vm
            .rows
            .into_iter()
            .map(|r| MemberRowView {
                email: r.email,
                role: r.role.into(),
                role_label: r.role_label,
                is_you: r.is_you,
            })
            .collect(),
        can_manage: vm.can_manage,
        role_options: vm
            .role_options
            .into_iter()
            .map(|o| RoleOptionView { role: o.role.into(), label: o.label })
            .collect(),
        is_loading: vm.is_loading,
        error: vm.error,
        notice: vm.notice,
    }
}

fn to_sign_in(vm: SignInVm) -> SignInView {
    SignInView { busy: vm.busy, error: vm.error, notice: vm.notice }
}

fn to_signed_in(vm: SignedInVm) -> SignedInView {
    SignedInView {
        email: vm.email,
        workspaces: vm
            .workspaces
            .into_iter()
            .map(|w| WorkspaceNavView {
                id: w.id.0,
                name: w.name,
                role_label: w.role_label,
                selected: w.selected,
            })
            .collect(),
        workspace: vm.workspace.map(|w| w.0),
        screens: vm
            .screens
            .into_iter()
            .map(|s| ScreenNavView {
                screen: match s.screen {
                    Screen::Tasks => AppScreen::Tasks,
                    Screen::Members => AppScreen::Members,
                },
                label: s.label,
                selected: s.selected,
            })
            .collect(),
        title: vm.title,
        error: vm.error,
        screen: match vm.screen {
            ScreenVm::Loading => ScreenView::Loading,
            ScreenVm::Tasks(t) => ScreenView::Tasks { view: to_view(t) },
            ScreenVm::Members(m) => ScreenView::Members { view: to_members_view(m) },
        },
    }
}

fn to_app_view(vm: AppVm) -> AppView {
    match vm {
        AppVm::Starting => AppView::Starting,
        AppVm::SignedOut(v) => AppView::SignedOut { view: to_sign_in(v) },
        AppVm::SignedIn(v) => AppView::SignedIn { view: to_signed_in(v) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default, Clone)]
    struct Vault(Arc<Mutex<Option<String>>>);

    impl SessionVault for Vault {
        fn load(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
        fn save(&self, value: String) {
            *self.0.lock().unwrap() = Some(value);
        }
        fn clear(&self) {
            *self.0.lock().unwrap() = None;
        }
    }

    struct Quiet;

    impl LogSink for Quiet {
        fn log(&self, _: crate::LogLevel, _: String, _: Vec<crate::LogField>) {}
    }

    #[test]
    fn signed_out_without_a_session_and_unreachable_server_is_described() {
        let vault = Vault::default();
        let config =
            AppConfig { api_url: "http://127.0.0.1:9/api".into(), auth_url: None, auth_key: None };
        let p = AppPresenter::new(config, Box::new(vault.clone()), Box::new(Quiet));
        assert!(matches!(p.dispatch(AppIntent::Started), AppView::SignedOut { .. }));

        let view = p.dispatch(AppIntent::SignInSubmitted {
            email: "ada@example.com".into(),
            password: String::new(),
        });
        assert!(
            vault.load().unwrap().contains("dev:ada@example.com"),
            "the session is kept in the vault"
        );
        match view {
            AppView::SignedIn { view } => {
                let error = view.error.unwrap_or_default();
                assert!(error.contains("could not reach the server"), "{error}");
            }
            other => panic!("{other:?}"),
        }
        p.dispatch(AppIntent::SignOutRequested);
        assert_eq!(vault.load(), None);
    }

    #[test]
    fn previews_cover_sign_in_and_both_roles() {
        let screens = app_preview_screens();
        let named = |n: &str| screens.iter().find(|s| s.name == n).unwrap().view.clone();
        assert!(matches!(named("sign-in"), AppView::SignedOut { .. }));
        match &named("viewer-tasks") {
            AppView::SignedIn { view: SignedInView { screen: ScreenView::Tasks { view }, .. } } => {
                assert!(!view.can_change)
            }
            other => panic!("{other:?}"),
        }
    }

    #[derive(Default, Clone)]
    struct Views(Arc<Mutex<Vec<AppView>>>);

    impl AppViewListener for Views {
        fn on_view(&self, view: AppView) {
            self.0.lock().unwrap().push(view);
        }
    }

    #[test]
    fn dispatch_with_shows_the_views_while_requests_run() {
        let config =
            AppConfig { api_url: "http://127.0.0.1:9/api".into(), auth_url: None, auth_key: None };
        let p = AppPresenter::new(config, Box::new(Vault::default()), Box::new(Quiet));
        let views = Views::default();
        let last = p.dispatch_with(AppIntent::Started, Box::new(views.clone()));
        assert!(matches!(last, AppView::SignedOut { .. }));
        assert_eq!(views.0.lock().unwrap().first(), Some(&AppView::Starting));

        views.0.lock().unwrap().clear();
        let last = p.dispatch_with(
            AppIntent::SignInSubmitted { email: "ada@example.com".into(), password: String::new() },
            Box::new(views.clone()),
        );
        let seen = views.0.lock().unwrap().clone();
        assert!(
            matches!(seen.first(), Some(AppView::SignedOut { view }) if view.busy),
            "sign-in shows busy before it answers: {seen:?}"
        );
        assert!(
            seen.iter().any(|v| matches!(v, AppView::SignedIn { .. })),
            "the signed-in frame shows while workspaces load: {seen:?}"
        );
        assert!(matches!(last, AppView::SignedIn { .. }));
    }

    #[test]
    fn screen_intents_carry_the_workspace_the_view_showed() {
        let screens = app_preview_screens();
        let preview = screens.into_iter().find(|s| s.name == "viewer-tasks").unwrap();
        let AppView::SignedIn { view } = preview.view else { panic!("viewer-tasks is signed in") };
        let workspace = view.workspace.expect("a workspace is open");
        assert_eq!(
            to_app_intent(AppIntent::Tasks {
                workspace: workspace.clone(),
                intent: TaskIntent::AddSubmitted { title: "x".into() },
            }),
            CoreIntent::Tasks(
                WorkspaceId(workspace.clone()),
                taskr_core::tasks::presentation::Intent::AddSubmitted("x".into())
            )
        );
        assert_eq!(
            to_app_intent(AppIntent::Members {
                workspace: workspace.clone(),
                intent: MembersIntent::Opened
            }),
            CoreIntent::Members(WorkspaceId(workspace), CoreMembersIntent::Opened)
        );
    }
}
