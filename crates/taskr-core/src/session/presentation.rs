//! The app frame every signed-in shell renders: sign-in, the workspace and
//! screen navigation, and whichever screen is open. Navigation lives here as
//! state ("which workspace, which screen"); how it looks (a sidebar on
//! desktop, a drawer on phones, tabs in a TUI) stays in each shell.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::session::app::{AuthClient, AuthError, Session, SessionStore, SignUp, describe_auth};
use crate::shared::access::{AccessError, Action, Role, WorkspaceId, can, describe_access};
use crate::shared::clock::{Clock, Millis};
use crate::tasks::app::{TasksApi, TasksError};
use crate::tasks::presentation::{self as tasks, TaskListState, TaskListVm};
use crate::workspaces::app::{Me, WorkspaceSummary, WorkspacesApi, WorkspacesError};
use crate::workspaces::presentation::{
    MemberChange, MembersEffect, MembersEvent, MembersIntent, MembersState, MembersVm,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Tasks,
    Members,
}

impl Screen {
    pub const ALL: [Screen; 2] = [Screen::Tasks, Screen::Members];

    pub fn label(self) -> &'static str {
        match self {
            Screen::Tasks => "Tasks",
            Screen::Members => "Members",
        }
    }
}

/// Counts sign-ins and sign-outs. Requests carry the one they were made
/// under, so a late answer for an earlier session (even a 401) is dropped
/// instead of signing out, or showing data in, the current one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Epoch(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppIntent {
    Started,
    SignInSubmitted {
        email: String,
        password: String,
    },
    SignUpSubmitted {
        email: String,
        password: String,
    },
    SignOutRequested,
    WorkspaceSelected(WorkspaceId),
    ScreenSelected(Screen),
    MessageDismissed,
    /// Screen intents name the workspace the shell showed (`SignedInVm::workspace`).
    /// One sent from a screen that was already replaced is dropped, so a
    /// form still on screen cannot write into the workspace being opened.
    Tasks(WorkspaceId, tasks::Intent),
    Members(WorkspaceId, MembersIntent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    User(AppIntent),
    SessionRestored(Option<Session>),
    SignedIn(Result<Session, AuthError>),
    SignedUp(Result<SignUp, AuthError>),
    MeLoaded(Epoch, Result<Me, WorkspacesError>),
    /// Results are tagged with the session and workspace they belong to, so a
    /// slow answer for a workspace the person already left is dropped instead of shown.
    Tasks(Epoch, WorkspaceId, tasks::Event),
    Members(Epoch, WorkspaceId, MembersEvent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEffect {
    RestoreSession,
    SignIn { email: String, password: String },
    SignUp { email: String, password: String },
    SaveSession(Session),
    ClearSession,
    LoadMe(Epoch),
    Tasks(Epoch, WorkspaceId, tasks::Effect),
    Members(Epoch, WorkspaceId, MembersEffect),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInVm {
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceNav {
    pub id: WorkspaceId,
    pub name: String,
    pub role_label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenNav {
    pub screen: Screen,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenVm {
    Loading,
    Tasks(TaskListVm),
    Members(MembersVm),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedInVm {
    pub email: String,
    pub workspaces: Vec<WorkspaceNav>,
    pub screens: Vec<ScreenNav>,
    /// The workspace the screen shows; shells send it back with screen intents.
    pub workspace: Option<WorkspaceId>,
    /// For the window title and the page header, e.g. "Personal · Tasks".
    pub title: String,
    pub error: Option<String>,
    pub screen: ScreenVm,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppVm {
    Starting,
    SignedOut(SignInVm),
    SignedIn(SignedInVm),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Open {
    workspace: WorkspaceSummary,
    screen: Screen,
    tasks: TaskListState,
    members: MembersState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SignedIn {
    email: String,
    workspaces: Vec<WorkspaceSummary>,
    open: Option<Open>,
    error: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Starting,
    SignedOut(SignInVm),
    SignedIn(SignedIn),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppState {
    phase: Phase,
    epoch: Epoch,
}

impl Default for AppState {
    fn default() -> Self {
        Self { phase: Phase::Starting, epoch: Epoch::default() }
    }
}

fn signed_out(error: Option<String>, notice: Option<String>) -> Phase {
    Phase::SignedOut(SignInVm { busy: false, error, notice })
}

fn tasks_access(event: &tasks::Event) -> Option<&AccessError> {
    match event {
        tasks::Event::Loaded(Err(TasksError::Access(a)))
        | tasks::Event::Changed(_, Err(TasksError::Access(a))) => Some(a),
        _ => None,
    }
}

fn members_access(event: &MembersEvent) -> Option<&AccessError> {
    match event {
        MembersEvent::Loaded(Err(WorkspacesError::Access(a)))
        | MembersEvent::Changed(_, Err(WorkspacesError::Access(a))) => Some(a),
        _ => None,
    }
}

impl AppState {
    pub fn update(&mut self, event: AppEvent) -> Vec<AppEffect> {
        match event {
            AppEvent::MeLoaded(epoch, _)
            | AppEvent::Tasks(epoch, ..)
            | AppEvent::Members(epoch, ..)
                if epoch != self.epoch =>
            {
                vec![]
            }
            AppEvent::User(intent) => self.on_intent(intent),
            AppEvent::SessionRestored(Some(session)) => self.start_session(session.email),
            AppEvent::SessionRestored(None) => {
                self.phase = signed_out(None, None);
                vec![]
            }
            AppEvent::SignedIn(Ok(session)) => {
                let mut effects = vec![AppEffect::SaveSession(session.clone())];
                effects.extend(self.start_session(session.email));
                effects
            }
            AppEvent::SignedUp(Ok(SignUp::SignedIn(session))) => {
                self.update(AppEvent::SignedIn(Ok(session)))
            }
            AppEvent::SignedUp(Ok(SignUp::ConfirmationSent)) => {
                self.phase = signed_out(
                    None,
                    Some("Check your email to confirm the account, then sign in.".into()),
                );
                vec![]
            }
            AppEvent::SignedIn(Err(e)) | AppEvent::SignedUp(Err(e)) => {
                self.phase = signed_out(Some(describe_auth(&e)), None);
                vec![]
            }
            AppEvent::MeLoaded(_, Err(WorkspacesError::Access(AccessError::Unauthenticated))) => {
                self.expire()
            }
            AppEvent::MeLoaded(_, Err(e)) => {
                if let Phase::SignedIn(s) = &mut self.phase {
                    s.error = Some(crate::workspaces::presentation::describe(&e));
                }
                vec![]
            }
            AppEvent::MeLoaded(_, Ok(me)) => self.on_me(me),
            AppEvent::Tasks(epoch, ws, event) => {
                let access = tasks_access(&event).cloned();
                if access == Some(AccessError::Unauthenticated) {
                    return self.expire();
                }
                let Some(open) = self.open_in(&ws) else { return vec![] };
                let mut effects = wrap_tasks(epoch, &ws, open.tasks.update(event));
                // Refused: the role we hold is out of date, ask where we stand.
                if access.is_some() {
                    effects.push(AppEffect::LoadMe(epoch));
                }
                effects
            }
            AppEvent::Members(epoch, ws, event) => {
                let access = members_access(&event).cloned();
                if access == Some(AccessError::Unauthenticated) {
                    return self.expire();
                }
                let Phase::SignedIn(s) = &mut self.phase else { return vec![] };
                let Some(open) = s.open.as_mut().filter(|o| o.workspace.id == ws) else {
                    return vec![];
                };
                // Our own row is the current role; the one from `/me` may be stale.
                let mine = match &event {
                    MembersEvent::Loaded(Ok(rows)) => {
                        Some(rows.iter().find(|m| m.email == s.email).map(|m| m.role))
                    }
                    _ => None,
                };
                let mut effects = wrap_members(epoch, &ws, open.members.update(event));
                match mine {
                    Some(Some(role)) => set_role(s, role),
                    Some(None) => effects.push(AppEffect::LoadMe(epoch)),
                    None if access.is_some() => effects.push(AppEffect::LoadMe(epoch)),
                    None => {}
                }
                effects
            }
        }
    }

    fn on_intent(&mut self, intent: AppIntent) -> Vec<AppEffect> {
        let epoch = self.epoch;
        match intent {
            AppIntent::Started => {
                self.phase = Phase::Starting;
                vec![AppEffect::RestoreSession]
            }
            AppIntent::SignInSubmitted { email, password } => {
                self.phase = Phase::SignedOut(SignInVm { busy: true, error: None, notice: None });
                vec![AppEffect::SignIn { email, password }]
            }
            AppIntent::SignUpSubmitted { email, password } => {
                self.phase = Phase::SignedOut(SignInVm { busy: true, error: None, notice: None });
                vec![AppEffect::SignUp { email, password }]
            }
            AppIntent::SignOutRequested => {
                self.next_epoch();
                self.phase = signed_out(None, Some("Signed out.".into()));
                vec![AppEffect::ClearSession]
            }
            AppIntent::MessageDismissed => {
                match &mut self.phase {
                    Phase::SignedOut(vm) => {
                        vm.error = None;
                        vm.notice = None;
                    }
                    Phase::SignedIn(s) => s.error = None,
                    Phase::Starting => {}
                }
                vec![]
            }
            AppIntent::WorkspaceSelected(id) => {
                let Phase::SignedIn(s) = &mut self.phase else { return vec![] };
                let screen = s.open.as_ref().map(|o| o.screen).unwrap_or_default();
                if s.open.as_ref().is_some_and(|o| o.workspace.id == id) {
                    return vec![];
                }
                match s.workspaces.iter().find(|w| w.id == id).cloned() {
                    Some(ws) => open(s, ws, screen, epoch),
                    None => vec![],
                }
            }
            AppIntent::ScreenSelected(screen) => {
                let Some(open) = self.current() else { return vec![] };
                if open.screen == screen {
                    return vec![];
                }
                open.screen = screen;
                enter(open, epoch)
            }
            AppIntent::Tasks(ws, intent) => match self.open_in(&ws) {
                Some(open) => wrap_tasks(epoch, &ws, open.tasks.update(tasks::Event::User(intent))),
                None => vec![],
            },
            AppIntent::Members(ws, intent) => match self.open_in(&ws) {
                Some(open) => {
                    wrap_members(epoch, &ws, open.members.update(MembersEvent::User(intent)))
                }
                None => vec![],
            },
        }
    }

    fn next_epoch(&mut self) {
        self.epoch = Epoch(self.epoch.0 + 1);
    }

    fn start_session(&mut self, email: String) -> Vec<AppEffect> {
        self.next_epoch();
        self.phase =
            Phase::SignedIn(SignedIn { email, workspaces: vec![], open: None, error: None });
        vec![AppEffect::LoadMe(self.epoch)]
    }

    fn expire(&mut self) -> Vec<AppEffect> {
        self.next_epoch();
        self.phase = signed_out(Some(describe_access(&AccessError::Unauthenticated)), None);
        vec![AppEffect::ClearSession]
    }

    fn on_me(&mut self, me: Me) -> Vec<AppEffect> {
        let epoch = self.epoch;
        let Phase::SignedIn(s) = &mut self.phase else { return vec![] };
        s.email = me.email;
        s.workspaces = me.workspaces;
        s.error = None;
        let Some(current) = &s.open else { return open_first(s, epoch) };
        match s.workspaces.iter().find(|w| w.id == current.workspace.id).map(|w| w.role) {
            // Still a member: the screen stays as it is, with the current role.
            Some(role) => {
                set_role(s, role);
                vec![]
            }
            None => {
                let left = current.workspace.name.clone();
                let effects = open_first(s, epoch);
                s.error = Some(format!("You are no longer a member of {left}."));
                effects
            }
        }
    }

    fn current(&mut self) -> Option<&mut Open> {
        match &mut self.phase {
            Phase::SignedIn(s) => s.open.as_mut(),
            _ => None,
        }
    }

    fn open_in(&mut self, ws: &WorkspaceId) -> Option<&mut Open> {
        self.current().filter(|o| &o.workspace.id == ws)
    }

    pub fn view(&self, now: Millis) -> AppVm {
        match &self.phase {
            Phase::Starting => AppVm::Starting,
            Phase::SignedOut(vm) => AppVm::SignedOut(vm.clone()),
            Phase::SignedIn(s) => {
                let open = s.open.as_ref();
                let current = open.map(|o| &o.workspace.id);
                let screen = open.map(|o| o.screen);
                SignedInVm {
                    email: s.email.clone(),
                    workspaces: s
                        .workspaces
                        .iter()
                        .map(|w| WorkspaceNav {
                            id: w.id.clone(),
                            name: w.name.clone(),
                            role_label: w.role.label().to_string(),
                            selected: Some(&w.id) == current,
                        })
                        .collect(),
                    screens: match open {
                        None => vec![],
                        Some(o) => Screen::ALL
                            .into_iter()
                            .map(|sc| ScreenNav {
                                screen: sc,
                                label: sc.label().to_string(),
                                selected: sc == o.screen,
                            })
                            .collect(),
                    },
                    workspace: current.cloned(),
                    title: match open {
                        Some(o) => format!("{} · {}", o.workspace.name, o.screen.label()),
                        None => "taskr".into(),
                    },
                    error: s.error.clone(),
                    screen: match (open, screen) {
                        (Some(o), Some(Screen::Tasks)) => ScreenVm::Tasks(o.tasks.view(now)),
                        (Some(o), Some(Screen::Members)) => ScreenVm::Members(o.members.view()),
                        _ => ScreenVm::Loading,
                    },
                }
                .into()
            }
        }
    }
}

impl From<SignedInVm> for AppVm {
    fn from(vm: SignedInVm) -> Self {
        AppVm::SignedIn(vm)
    }
}

fn open_first(s: &mut SignedIn, epoch: Epoch) -> Vec<AppEffect> {
    match s.workspaces.first().cloned() {
        Some(ws) => open(s, ws, Screen::Tasks, epoch),
        None => {
            s.open = None;
            vec![]
        }
    }
}

fn open(s: &mut SignedIn, ws: WorkspaceSummary, screen: Screen, epoch: Epoch) -> Vec<AppEffect> {
    let open = s.open.insert(Open {
        tasks: TaskListState::new(can(ws.role, Action::ChangeTasks)),
        members: MembersState::new(&s.email, ws.role),
        workspace: ws,
        screen,
    });
    enter(open, epoch)
}

/// Applies what the person may now do in the open workspace to every part
/// that shows it: the navigation label and both screens.
fn set_role(s: &mut SignedIn, role: Role) {
    let Some(open) = s.open.as_mut() else { return };
    open.workspace.role = role;
    open.tasks.set_can_change(can(role, Action::ChangeTasks));
    open.members.set_role(role);
    if let Some(nav) = s.workspaces.iter_mut().find(|w| w.id == open.workspace.id) {
        nav.role = role;
    }
}

fn enter(open: &mut Open, epoch: Epoch) -> Vec<AppEffect> {
    let ws = open.workspace.id.clone();
    match open.screen {
        Screen::Tasks => {
            wrap_tasks(epoch, &ws, open.tasks.update(tasks::Event::User(tasks::Intent::Opened)))
        }
        Screen::Members => {
            wrap_members(epoch, &ws, open.members.update(MembersEvent::User(MembersIntent::Opened)))
        }
    }
}

fn wrap_tasks(epoch: Epoch, ws: &WorkspaceId, effects: Vec<tasks::Effect>) -> Vec<AppEffect> {
    effects.into_iter().map(|e| AppEffect::Tasks(epoch, ws.clone(), e)).collect()
}

fn wrap_members(epoch: Epoch, ws: &WorkspaceId, effects: Vec<MembersEffect>) -> Vec<AppEffect> {
    effects.into_iter().map(|e| AppEffect::Members(epoch, ws.clone(), e)).collect()
}

/// The runtime for `AppState`: runs effects through the ports and feeds the
/// results back in. One per signed-in shell (web, CLI, later mobile).
pub struct AppController {
    state: Mutex<AppState>,
    auth: Arc<dyn AuthClient>,
    sessions: Arc<dyn SessionStore>,
    tasks: Arc<dyn TasksApi>,
    workspaces: Arc<dyn WorkspacesApi>,
    clock: Arc<dyn Clock>,
}

impl AppController {
    pub fn new(
        auth: Arc<dyn AuthClient>,
        sessions: Arc<dyn SessionStore>,
        tasks: Arc<dyn TasksApi>,
        workspaces: Arc<dyn WorkspacesApi>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self { state: Mutex::new(AppState::default()), auth, sessions, tasks, workspaces, clock }
    }

    pub async fn dispatch(&self, intent: AppIntent) -> AppVm {
        self.dispatch_with(intent, |_| {}).await
    }

    /// `dispatch`, also handing `on_view` the view each time requests are
    /// about to run, so a shell shows "opening B" right away instead of
    /// keeping the previous screen until every request has answered.
    pub async fn dispatch_with(&self, intent: AppIntent, mut on_view: impl FnMut(AppVm)) -> AppVm {
        let mut queue = VecDeque::from([AppEvent::User(intent)]);
        while let Some(event) = queue.pop_front() {
            let effects = self.state().update(event);
            if !effects.is_empty() {
                on_view(self.view());
            }
            for effect in effects {
                if let Some(event) = self.run(effect).await {
                    queue.push_back(event);
                }
            }
        }
        self.view()
    }

    pub fn view(&self) -> AppVm {
        self.state().view(self.clock.now())
    }

    fn state(&self) -> MutexGuard<'_, AppState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn run(&self, effect: AppEffect) -> Option<AppEvent> {
        Some(match effect {
            AppEffect::RestoreSession => AppEvent::SessionRestored(self.sessions.load()),
            AppEffect::SignIn { email, password } => {
                AppEvent::SignedIn(self.auth.sign_in(&email, &password).await)
            }
            AppEffect::SignUp { email, password } => {
                AppEvent::SignedUp(self.auth.sign_up(&email, &password).await)
            }
            AppEffect::SaveSession(session) => {
                self.sessions.save(&session);
                return None;
            }
            AppEffect::ClearSession => {
                self.sessions.clear();
                return None;
            }
            AppEffect::LoadMe(epoch) => AppEvent::MeLoaded(epoch, self.workspaces.me().await),
            AppEffect::Tasks(epoch, ws, effect) => {
                let event = tasks::run(self.tasks.as_ref(), &ws, effect).await;
                AppEvent::Tasks(epoch, ws, event)
            }
            AppEffect::Members(epoch, ws, effect) => {
                let api = self.workspaces.as_ref();
                let event = match effect {
                    MembersEffect::Load => MembersEvent::Loaded(api.members(&ws).await),
                    MembersEffect::Set { email, role } => MembersEvent::Changed(
                        MemberChange::Set { email: email.clone(), role },
                        api.set_member(&ws, email, role).await,
                    ),
                    MembersEffect::Remove { email } => MembersEvent::Changed(
                        MemberChange::Removed { email: email.clone() },
                        api.remove_member(&ws, email).await,
                    ),
                };
                AppEvent::Members(epoch, ws, event)
            }
        })
    }
}

/// Named screens for previews and snapshot tests.
pub mod fixtures {
    use super::*;
    use crate::workspaces::app::MemberSummary;

    pub fn sign_in() -> AppVm {
        AppVm::SignedOut(SignInVm { busy: false, error: None, notice: None })
    }

    pub fn sign_in_error() -> AppVm {
        let mut s = AppState::default();
        s.update(AppEvent::SignedIn(Err(AuthError::InvalidCredentials)));
        s.view(tasks::fixtures::NOW)
    }

    fn session(role: Role) -> (AppState, WorkspaceId) {
        let mut s = AppState::default();
        s.update(AppEvent::SessionRestored(Some(Session {
            access_token: "t".into(),
            refresh_token: None,
            email: "ada@example.com".into(),
            expires_at: None,
        })));
        let ws = WorkspaceId("ws-1".into());
        s.update(AppEvent::MeLoaded(
            s.epoch,
            Ok(Me {
                email: "ada@example.com".into(),
                workspaces: vec![
                    WorkspaceSummary { id: ws.clone(), name: "ada's workspace".into(), role },
                    WorkspaceSummary {
                        id: WorkspaceId("ws-2".into()),
                        name: "Team".into(),
                        role: Role::Editor,
                    },
                ],
            }),
        ));
        (s, ws)
    }

    pub fn loading() -> AppVm {
        session(Role::Owner).0.view(tasks::fixtures::NOW)
    }

    pub fn signed_in(role: Role) -> AppVm {
        let (mut s, ws) = session(role);
        let loaded = tasks::Event::Loaded(Ok(tasks::fixtures::sample_tasks()));
        s.update(AppEvent::Tasks(s.epoch, ws, loaded));
        s.view(tasks::fixtures::NOW)
    }

    pub fn members(role: Role) -> AppVm {
        let (mut s, ws) = session(role);
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Members)));
        s.update(AppEvent::Members(
            s.epoch,
            ws,
            MembersEvent::Loaded(Ok(vec![
                MemberSummary { email: "ada@example.com".into(), role },
                MemberSummary { email: "bob@example.com".into(), role: Role::Editor },
                MemberSummary { email: "eve@example.com".into(), role: Role::Viewer },
            ])),
        ));
        s.view(tasks::fixtures::NOW)
    }

    /// Every app screen by name, for Storybook, component tests and previews.
    pub fn all() -> Vec<(&'static str, AppVm)> {
        vec![
            ("sign-in", sign_in()),
            ("sign-in-error", sign_in_error()),
            ("loading", loading()),
            ("owner-tasks", signed_in(Role::Owner)),
            ("viewer-tasks", signed_in(Role::Viewer)),
            ("owner-members", members(Role::Owner)),
            ("viewer-members", members(Role::Viewer)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::InProcessApi;
    use crate::shared::access::{Email, Principal, UserId};
    use crate::shared::telemetry::NoTelemetry;
    use crate::tasks::app::{TaskSummary, Tasks};
    use crate::testing::{
        FakeAuth, FixedClock, InMemoryTaskListRepository, InMemoryWorkspaceRepository,
        MemorySessionStore, block_on,
    };
    use crate::workspaces::app::{MemberSummary, Workspaces};

    fn session(email: &str) -> Session {
        Session {
            access_token: format!("test:{email}"),
            refresh_token: None,
            email: email.into(),
            expires_at: None,
        }
    }

    fn ws(id: &str, role: Role) -> WorkspaceSummary {
        WorkspaceSummary { id: WorkspaceId(id.into()), name: format!("WS {id}"), role }
    }

    fn signed_in(workspaces: Vec<WorkspaceSummary>) -> AppState {
        let mut s = AppState::default();
        s.update(AppEvent::SessionRestored(Some(session("ada@example.com"))));
        let me = Me { email: "ada@example.com".into(), workspaces };
        s.update(AppEvent::MeLoaded(s.epoch, Ok(me)));
        s
    }

    fn signed_in_vm(s: &AppState) -> SignedInVm {
        match s.view(0) {
            AppVm::SignedIn(vm) => vm,
            other => panic!("expected signed in, got {other:?}"),
        }
    }

    #[test]
    fn start_restores_the_session_then_loads_me() {
        let mut s = AppState::default();
        assert_eq!(s.update(AppEvent::User(AppIntent::Started)), vec![AppEffect::RestoreSession]);
        assert_eq!(s.view(0), AppVm::Starting);
        assert_eq!(
            s.update(AppEvent::SessionRestored(Some(session("ada@example.com")))),
            vec![AppEffect::LoadMe(Epoch(1))]
        );
        assert_eq!(signed_in_vm(&s).screen, ScreenVm::Loading);
    }

    #[test]
    fn no_stored_session_shows_sign_in() {
        let mut s = AppState::default();
        s.update(AppEvent::SessionRestored(None));
        assert_eq!(s.view(0), fixtures::sign_in());
    }

    #[test]
    fn sign_in_saves_the_session_and_failures_are_described() {
        let mut s = AppState::default();
        let effects = s.update(AppEvent::User(AppIntent::SignInSubmitted {
            email: "ada@example.com".into(),
            password: "pw".into(),
        }));
        assert_eq!(effects.len(), 1);
        assert!(matches!(s.view(0), AppVm::SignedOut(SignInVm { busy: true, .. })));

        s.update(AppEvent::SignedIn(Err(AuthError::InvalidCredentials)));
        assert!(matches!(
            s.view(0),
            AppVm::SignedOut(SignInVm { busy: false, error: Some(_), .. })
        ));

        let effects = s.update(AppEvent::SignedIn(Ok(session("ada@example.com"))));
        assert_eq!(
            effects,
            vec![AppEffect::SaveSession(session("ada@example.com")), AppEffect::LoadMe(Epoch(1))]
        );
    }

    #[test]
    fn me_opens_the_first_workspace_on_the_tasks_screen() {
        let mut s = AppState::default();
        s.update(AppEvent::SessionRestored(Some(session("ada@example.com"))));
        let effects = s.update(AppEvent::MeLoaded(
            s.epoch,
            Ok(Me {
                email: "ada@example.com".into(),
                workspaces: vec![ws("a", Role::Owner), ws("b", Role::Viewer)],
            }),
        ));
        assert_eq!(
            effects,
            vec![AppEffect::Tasks(Epoch(1), WorkspaceId("a".into()), tasks::Effect::LoadAll)]
        );
        let vm = signed_in_vm(&s);
        assert_eq!(vm.title, "WS a · Tasks");
        assert_eq!(
            vm.workspaces.iter().map(|w| (w.name.as_str(), w.selected)).collect::<Vec<_>>(),
            [("WS a", true), ("WS b", false)]
        );
        assert_eq!(vm.screens.iter().filter(|n| n.selected).count(), 1);
    }

    #[test]
    fn switching_workspace_keeps_the_screen_and_applies_the_role() {
        let mut s = signed_in(vec![ws("a", Role::Owner), ws("b", Role::Viewer)]);
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Members)));
        let effects =
            s.update(AppEvent::User(AppIntent::WorkspaceSelected(WorkspaceId("b".into()))));
        assert_eq!(
            effects,
            vec![AppEffect::Members(Epoch(1), WorkspaceId("b".into()), MembersEffect::Load)]
        );
        let vm = signed_in_vm(&s);
        assert_eq!(vm.title, "WS b · Members");
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Tasks)));
        match signed_in_vm(&s).screen {
            ScreenVm::Tasks(t) => assert!(!t.can_change, "viewers get a read-only list"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn late_results_for_a_workspace_we_left_are_dropped() {
        let mut s = signed_in(vec![ws("a", Role::Owner), ws("b", Role::Owner)]);
        s.update(AppEvent::User(AppIntent::WorkspaceSelected(WorkspaceId("b".into()))));
        let late = tasks::Event::Loaded(Ok(vec![TaskSummary {
            id: 1,
            title: "from a".into(),
            created_at: 0,
            done_at: None,
        }]));
        assert!(s.update(AppEvent::Tasks(s.epoch, WorkspaceId("a".into()), late)).is_empty());
        match signed_in_vm(&s).screen {
            ScreenVm::Tasks(t) => assert!(t.rows.is_empty() && t.is_loading),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_expired_session_signs_out_from_any_screen() {
        let mut s = signed_in(vec![ws("a", Role::Owner)]);
        let expired = tasks::Event::Loaded(Err(TasksError::Access(AccessError::Unauthenticated)));
        assert_eq!(
            s.update(AppEvent::Tasks(s.epoch, WorkspaceId("a".into()), expired)),
            vec![AppEffect::ClearSession]
        );
        match s.view(0) {
            AppVm::SignedOut(vm) => {
                assert_eq!(vm.error.as_deref(), Some("Your session has ended. Sign in again."))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sign_out_clears_the_session() {
        let mut s = signed_in(vec![ws("a", Role::Owner)]);
        assert_eq!(
            s.update(AppEvent::User(AppIntent::SignOutRequested)),
            vec![AppEffect::ClearSession]
        );
        assert!(matches!(s.view(0), AppVm::SignedOut(_)));
    }

    #[test]
    fn a_form_left_on_screen_cannot_write_into_the_workspace_being_opened() {
        let (a, b) = (WorkspaceId("a".into()), WorkspaceId("b".into()));
        let mut s = signed_in(vec![ws("a", Role::Owner), ws("b", Role::Owner)]);
        s.update(AppEvent::Tasks(s.epoch, a.clone(), tasks::Event::Loaded(Ok(vec![]))));
        s.update(AppEvent::User(AppIntent::WorkspaceSelected(b.clone())));

        let from_a = tasks::Intent::AddSubmitted("private note".into());
        assert!(s.update(AppEvent::User(AppIntent::Tasks(a, from_a))).is_empty());
        let from_b = tasks::Intent::AddSubmitted("team task".into());
        assert_eq!(
            s.update(AppEvent::User(AppIntent::Tasks(b.clone(), from_b))),
            vec![AppEffect::Tasks(s.epoch, b.clone(), tasks::Effect::Add("team task".into()))]
        );
        assert_eq!(signed_in_vm(&s).workspace, Some(b));
    }

    #[test]
    fn answers_for_an_earlier_session_are_dropped() {
        let mut s = signed_in(vec![ws("a", Role::Owner)]);
        let old = s.epoch;
        s.update(AppEvent::User(AppIntent::SignOutRequested));
        s.update(AppEvent::SignedIn(Ok(session("eve@example.com"))));
        let eve = Me { email: "eve@example.com".into(), workspaces: vec![ws("e", Role::Owner)] };
        s.update(AppEvent::MeLoaded(s.epoch, Ok(eve)));

        let ada = Me { email: "ada@example.com".into(), workspaces: vec![ws("a", Role::Owner)] };
        assert!(s.update(AppEvent::MeLoaded(old, Ok(ada))).is_empty());
        let expired = tasks::Event::Loaded(Err(TasksError::Access(AccessError::Unauthenticated)));
        assert!(s.update(AppEvent::Tasks(old, WorkspaceId("a".into()), expired)).is_empty());

        let vm = signed_in_vm(&s);
        assert_eq!(vm.email, "eve@example.com");
        assert_eq!(vm.workspace, Some(WorkspaceId("e".into())));
    }

    fn can_manage_and_change(s: &mut AppState) -> (bool, bool, String) {
        let label = signed_in_vm(s).workspaces[0].role_label.clone();
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Members)));
        let ScreenVm::Members(m) = signed_in_vm(s).screen else { panic!() };
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Tasks)));
        let ScreenVm::Tasks(t) = signed_in_vm(s).screen else { panic!() };
        (m.can_manage, t.can_change, label)
    }

    #[test]
    fn the_members_list_updates_what_i_may_do() {
        let a = WorkspaceId("a".into());
        let mut s = signed_in(vec![ws("a", Role::Owner)]);
        s.update(AppEvent::User(AppIntent::ScreenSelected(Screen::Members)));
        let rows = vec![
            MemberSummary { email: "bob@example.com".into(), role: Role::Owner },
            MemberSummary { email: "ada@example.com".into(), role: Role::Viewer },
        ];
        let effects = s.update(AppEvent::Members(s.epoch, a, MembersEvent::Loaded(Ok(rows))));
        assert!(effects.is_empty());
        assert_eq!(can_manage_and_change(&mut s), (false, false, "Viewer".into()));
    }

    #[test]
    fn a_refusal_asks_the_server_where_i_stand() {
        let a = WorkspaceId("a".into());
        let mut s = signed_in(vec![ws("a", Role::Owner), ws("b", Role::Owner)]);
        let refused = tasks::Event::Changed(
            tasks::Change::Added,
            Err(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks))),
        );
        let effects = s.update(AppEvent::Tasks(s.epoch, a.clone(), refused));
        assert_eq!(effects, vec![AppEffect::LoadMe(s.epoch)]);

        let demoted = Me {
            email: "ada@example.com".into(),
            workspaces: vec![ws("a", Role::Editor), ws("b", Role::Owner)],
        };
        assert!(s.update(AppEvent::MeLoaded(s.epoch, Ok(demoted))).is_empty(), "no reload");
        assert_eq!(can_manage_and_change(&mut s), (false, true, "Editor".into()));

        let removed =
            Me { email: "ada@example.com".into(), workspaces: vec![ws("b", Role::Owner)] };
        s.update(AppEvent::MeLoaded(s.epoch, Ok(removed)));
        let vm = signed_in_vm(&s);
        assert_eq!(vm.workspace, Some(WorkspaceId("b".into())));
        assert_eq!(vm.error.as_deref(), Some("You are no longer a member of WS a."));
    }

    #[test]
    fn dispatch_with_shows_the_pending_screen_before_requests_answer() {
        let clock = Arc::new(FixedClock::at(0));
        let principal = Principal {
            user_id: UserId("u1".into()),
            email: Email::parse("ada@example.com").unwrap(),
        };
        let api = Arc::new(InProcessApi::new(
            Tasks::new(
                Arc::new(InMemoryTaskListRepository::default()),
                clock.clone(),
                Arc::new(NoTelemetry),
            ),
            Workspaces::new(
                Arc::new(InMemoryWorkspaceRepository::default()),
                clock.clone(),
                Arc::new(NoTelemetry),
            ),
            principal,
        ));
        let store = Arc::new(MemorySessionStore::default());
        let app = AppController::new(Arc::new(FakeAuth), store, api.clone(), api, clock);
        block_on(app.dispatch(AppIntent::Started));

        let mut shown = vec![];
        let sign_in =
            AppIntent::SignInSubmitted { email: "ada@example.com".into(), password: "pw".into() };
        let last = block_on(app.dispatch_with(sign_in, |vm| shown.push(vm)));
        assert!(matches!(shown[0], AppVm::SignedOut(SignInVm { busy: true, .. })), "{shown:?}");
        assert!(shown.iter().any(|vm| matches!(
            vm,
            AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) if t.is_loading
        )));
        assert!(
            matches!(&last, AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) if !t.is_loading)
        );
    }

    #[test]
    fn every_app_fixture_is_the_screen_its_name_says() {
        for (name, vm) in fixtures::all() {
            let kind = match &vm {
                AppVm::Starting => "starting",
                AppVm::SignedOut(_) => "sign-in",
                AppVm::SignedIn(SignedInVm { screen: ScreenVm::Loading, .. }) => "loading",
                AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) if t.is_loading => {
                    "loading"
                }
                AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(_), .. }) => "tasks",
                AppVm::SignedIn(SignedInVm { screen: ScreenVm::Members(_), .. }) => "members",
            };
            assert!(name.contains(kind), "{name} renders {kind}");
        }
    }

    #[test]
    fn fixtures_render() {
        match fixtures::signed_in(Role::Owner) {
            AppVm::SignedIn(vm) => {
                assert_eq!(vm.workspaces.len(), 2);
                assert!(matches!(vm.screen, ScreenVm::Tasks(ref t) if t.rows.len() == 3));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn controller_signs_in_adds_a_task_and_invites_a_member() {
        let clock = Arc::new(FixedClock::at(0));
        let api = Arc::new(InProcessApi::new(
            Tasks::new(
                Arc::new(InMemoryTaskListRepository::default()),
                clock.clone(),
                Arc::new(NoTelemetry),
            ),
            Workspaces::new(
                Arc::new(InMemoryWorkspaceRepository::default()),
                clock.clone(),
                Arc::new(NoTelemetry),
            ),
            Principal {
                user_id: UserId("u1".into()),
                email: Email::parse("ada@example.com").unwrap(),
            },
        ));
        let store = Arc::new(MemorySessionStore::default());
        let app = AppController::new(Arc::new(FakeAuth), store.clone(), api.clone(), api, clock);

        assert!(matches!(block_on(app.dispatch(AppIntent::Started)), AppVm::SignedOut(_)));
        let vm = block_on(app.dispatch(AppIntent::SignInSubmitted {
            email: "ada@example.com".into(),
            password: "pw".into(),
        }));
        assert!(store.load().is_some());
        let AppVm::SignedIn(vm) = vm else { panic!("{vm:?}") };
        assert_eq!(vm.title, "ada's workspace · Tasks");
        let ws = vm.workspace.expect("a workspace is open");

        let add = AppIntent::Tasks(ws.clone(), tasks::Intent::AddSubmitted("Ship M1".into()));
        let vm = block_on(app.dispatch(add));
        let AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) = vm else { panic!() };
        assert_eq!(t.rows[0].title, "Ship M1");

        block_on(app.dispatch(AppIntent::ScreenSelected(Screen::Members)));
        let vm = block_on(app.dispatch(AppIntent::Members(
            ws,
            MembersIntent::AddSubmitted { email: "bob@example.com".into(), role: Role::Viewer },
        )));
        let AppVm::SignedIn(SignedInVm { screen: ScreenVm::Members(m), .. }) = vm else { panic!() };
        assert_eq!(
            m.rows.iter().map(|r| r.email.as_str()).collect::<Vec<_>>(),
            ["ada@example.com", "bob@example.com"]
        );
        assert_eq!(m.notice.as_deref(), Some("bob@example.com is now Viewer"));

        block_on(app.dispatch(AppIntent::SignOutRequested));
        assert!(store.load().is_none());
    }
}
