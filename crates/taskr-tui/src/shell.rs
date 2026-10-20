//! TUI shell state: only what is terminal specific (cursor, input mode, help
//! overlay). Everything else comes from the shared `TaskListVm`.
//!
//! Signed out it drives a local `TaskListController`. After `taskr login` it
//! drives the shared `AppController` instead, the same one the web app runs,
//! which adds sign-in, workspaces and members.

use taskr_core::session::presentation::{
    AppController, AppIntent, AppVm, Screen as AppScreen, ScreenVm, SignInVm,
};
use taskr_core::shared::access::{Role, WorkspaceId};
use taskr_core::shared::future::block_on;
use taskr_core::tasks::app::Scope;
use taskr_core::tasks::presentation::{Intent, TaskListController, TaskListVm};
use taskr_core::workspaces::presentation::{MembersIntent, MembersVm};

use crate::keymap::{Action, Key, MEMBERS_KEYS, TASKS_KEYS, action_for};

const HALF_PAGE: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Adding(String),
    Filtering(String),
    AddingMember(String),
    SignInEmail(String),
    SignInPassword { email: String, password: String },
}

#[allow(clippy::large_enum_variant)]
pub enum Backend {
    Local(TaskListController),
    /// Network calls block the UI loop for their duration, like k9s does.
    Remote {
        app: AppController,
        runtime: tokio::runtime::Runtime,
        server: String,
        dev: bool,
    },
}

/// What the body shows.
pub enum Screen<'a> {
    Starting,
    SignIn(&'a SignInVm),
    Tasks,
    Members(&'a MembersVm),
}

pub struct Shell {
    backend: Backend,
    pub vm: TaskListVm,
    pub app: Option<AppVm>,
    pub cursor: usize,
    pub member_cursor: usize,
    pub mode: Mode,
    pub show_help: bool,
    pub should_quit: bool,
}

impl Shell {
    pub fn new(controller: TaskListController) -> Self {
        let vm = block_on(controller.dispatch(Intent::Opened));
        Self::with(Backend::Local(controller), vm)
    }

    pub fn remote(
        app: AppController,
        runtime: tokio::runtime::Runtime,
        server: String,
        dev: bool,
    ) -> Self {
        let vm = taskr_core::tasks::presentation::fixtures::empty();
        let mut shell = Self::with(Backend::Remote { app, runtime, server, dev }, vm);
        shell.dispatch(AppIntent::Started);
        shell
    }

    fn with(backend: Backend, vm: TaskListVm) -> Self {
        Self {
            backend,
            vm,
            app: None,
            cursor: 0,
            member_cursor: 0,
            mode: Mode::Normal,
            show_help: false,
            should_quit: false,
        }
    }

    pub fn signed_in(&self) -> bool {
        matches!(self.app, Some(AppVm::SignedIn(_)))
    }

    pub fn server(&self) -> Option<&str> {
        match &self.backend {
            Backend::Remote { server, .. } => Some(server),
            Backend::Local(_) => None,
        }
    }

    pub fn screen(&self) -> Screen<'_> {
        match &self.app {
            None => Screen::Tasks,
            Some(AppVm::Starting) => Screen::Starting,
            Some(AppVm::SignedOut(vm)) => Screen::SignIn(vm),
            Some(AppVm::SignedIn(s)) => match &s.screen {
                ScreenVm::Members(m) => Screen::Members(m),
                ScreenVm::Tasks(_) | ScreenVm::Loading => Screen::Tasks,
            },
        }
    }

    /// Workspace and screen, e.g. "ada's workspace · Tasks", when signed in.
    pub fn title(&self) -> Option<&str> {
        match &self.app {
            Some(AppVm::SignedIn(s)) => Some(&s.title),
            _ => None,
        }
    }

    pub fn app_error(&self) -> Option<&str> {
        match &self.app {
            Some(AppVm::SignedIn(s)) => s.error.as_deref(),
            _ => None,
        }
    }

    pub fn tick(&mut self) {
        match self.screen() {
            Screen::Tasks => self.send(Intent::Refreshed),
            Screen::Members(_) => self.members(MembersIntent::Opened),
            Screen::SignIn(_) | Screen::Starting => {}
        }
    }

    pub fn selected_id(&self) -> Option<u32> {
        self.vm.rows.get(self.cursor).map(|r| r.id)
    }

    fn selected_member(&self) -> Option<String> {
        match self.screen() {
            Screen::Members(m) => m.rows.get(self.member_cursor).map(|r| r.email.clone()),
            _ => None,
        }
    }

    pub fn handle(&mut self, key: Key) {
        if key == Key::Ctrl('c') {
            self.should_quit = true;
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        match self.mode.clone() {
            Mode::Normal => {
                let signed_in = self.signed_in();
                let action = match self.screen() {
                    Screen::Tasks => action_for(TASKS_KEYS, signed_in, key),
                    Screen::Members(_) => action_for(MEMBERS_KEYS, signed_in, key),
                    Screen::SignIn(_) | Screen::Starting => {
                        (key == Key::Char('q')).then_some(Action::Quit)
                    }
                };
                match (action, self.screen()) {
                    (Some(a), Screen::Members(_)) => self.apply_members(a),
                    (Some(a), _) => self.apply(a),
                    (None, _) => {}
                }
            }
            Mode::Adding(text) => self.handle_adding(key, text),
            Mode::Filtering(text) => self.handle_filtering(key, text),
            Mode::AddingMember(text) => self.handle_adding_member(key, text),
            Mode::SignInEmail(text) => self.handle_email(key, text),
            Mode::SignInPassword { email, password } => self.handle_password(key, email, password),
        }
    }

    fn apply(&mut self, action: Action) {
        let last = self.vm.rows.len().saturating_sub(1);
        match action {
            Action::Down => self.cursor = (self.cursor + 1).min(last),
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Top => self.cursor = 0,
            Action::Bottom => self.cursor = last,
            Action::HalfPageDown => self.cursor = (self.cursor + HALF_PAGE).min(last),
            Action::HalfPageUp => self.cursor = self.cursor.saturating_sub(HALF_PAGE),
            Action::Toggle => {
                if let Some(id) = self.selected_id() {
                    self.send(Intent::ToggleRequested(id));
                }
            }
            Action::Add => self.mode = Mode::Adding(String::new()),
            Action::Filter => self.mode = Mode::Filtering(self.vm.filter.clone()),
            Action::Clear => {
                self.send(Intent::FilterChanged(String::new()));
                self.send(Intent::MessageDismissed);
                if self.app_error().is_some() {
                    self.dispatch(AppIntent::MessageDismissed);
                }
            }
            Action::ShowOpen => self.show(Scope::Open),
            Action::ShowDone => self.show(Scope::Done),
            Action::ShowAll => self.show(Scope::All),
            Action::NextScope => {
                let current = self.vm.tabs.iter().position(|t| t.selected).unwrap_or(0);
                let next = self.vm.tabs[(current + 1) % self.vm.tabs.len()].scope;
                self.show(next);
            }
            Action::Refresh => self.send(Intent::Refreshed),
            other => self.apply_common(other),
        }
    }

    fn apply_members(&mut self, action: Action) {
        let rows = match self.screen() {
            Screen::Members(m) => m.rows.len(),
            _ => 0,
        };
        let last = rows.saturating_sub(1);
        let role =
            |r| self.selected_member().map(|email| MembersIntent::RoleChanged { email, role: r });
        match action {
            Action::Down => self.member_cursor = (self.member_cursor + 1).min(last),
            Action::Up => self.member_cursor = self.member_cursor.saturating_sub(1),
            Action::Top => self.member_cursor = 0,
            Action::Bottom => self.member_cursor = last,
            Action::Add => self.mode = Mode::AddingMember(String::new()),
            Action::MakeOwner | Action::MakeEditor | Action::MakeViewer => {
                let r = match action {
                    Action::MakeOwner => Role::Owner,
                    Action::MakeEditor => Role::Editor,
                    _ => Role::Viewer,
                };
                if let Some(intent) = role(r) {
                    self.members(intent);
                }
            }
            Action::Remove => {
                if let Some(email) = self.selected_member() {
                    self.members(MembersIntent::RemoveRequested { email });
                }
            }
            Action::Clear => self.members(MembersIntent::MessageDismissed),
            Action::Refresh => self.members(MembersIntent::Opened),
            other => self.apply_common(other),
        }
    }

    fn apply_common(&mut self, action: Action) {
        match action {
            Action::Help => self.show_help = true,
            Action::Quit => self.should_quit = true,
            Action::ShowMembers => self.dispatch(AppIntent::ScreenSelected(AppScreen::Members)),
            Action::ShowTasks => self.dispatch(AppIntent::ScreenSelected(AppScreen::Tasks)),
            Action::SignOut => self.dispatch(AppIntent::SignOutRequested),
            Action::NextWorkspace => {
                let Some(AppVm::SignedIn(s)) = &self.app else { return };
                let current = s.workspaces.iter().position(|w| w.selected).unwrap_or(0);
                if let Some(next) = s.workspaces.get((current + 1) % s.workspaces.len().max(1)) {
                    let id = next.id.clone();
                    self.cursor = 0;
                    self.member_cursor = 0;
                    self.dispatch(AppIntent::WorkspaceSelected(id));
                }
            }
            _ => {}
        }
    }

    fn handle_adding(&mut self, key: Key, mut text: String) {
        match key {
            Key::Enter => {
                self.mode = Mode::Normal;
                self.send(Intent::AddSubmitted(text));
            }
            Key::Esc => self.mode = Mode::Normal,
            _ => {
                edit(&mut text, key);
                self.mode = Mode::Adding(text);
            }
        }
    }

    fn handle_filtering(&mut self, key: Key, mut text: String) {
        match key {
            Key::Enter => self.mode = Mode::Normal,
            Key::Esc => {
                self.mode = Mode::Normal;
                self.send(Intent::FilterChanged(String::new()));
            }
            _ => {
                edit(&mut text, key);
                self.send(Intent::FilterChanged(text.clone()));
                self.mode = Mode::Filtering(text);
            }
        }
    }

    /// `email` or `email role`; the role defaults to editor.
    fn handle_adding_member(&mut self, key: Key, mut text: String) {
        match key {
            Key::Enter => {
                let mut words = text.split_whitespace();
                let email = words.next().unwrap_or_default().to_string();
                let role = match words.next() {
                    None => Some(Role::Editor),
                    Some(word) => Role::parse(&word.to_lowercase()),
                };
                if let Some(role) = role {
                    self.mode = Mode::Normal;
                    self.members(MembersIntent::AddSubmitted { email, role });
                }
            }
            Key::Esc => self.mode = Mode::Normal,
            _ => {
                edit(&mut text, key);
                self.mode = Mode::AddingMember(text);
            }
        }
    }

    fn handle_email(&mut self, key: Key, mut text: String) {
        let dev = matches!(self.backend, Backend::Remote { dev: true, .. });
        match key {
            Key::Enter if dev => self.sign_in(text, String::new()),
            Key::Enter => self.mode = Mode::SignInPassword { email: text, password: String::new() },
            Key::Esc => self.mode = Mode::SignInEmail(String::new()),
            _ => {
                edit(&mut text, key);
                self.mode = Mode::SignInEmail(text);
            }
        }
    }

    fn handle_password(&mut self, key: Key, email: String, mut password: String) {
        match key {
            Key::Enter => self.sign_in(email, password),
            Key::Esc => self.mode = Mode::SignInEmail(email),
            _ => {
                edit(&mut password, key);
                self.mode = Mode::SignInPassword { email, password };
            }
        }
    }

    fn sign_in(&mut self, email: String, password: String) {
        self.mode = Mode::Normal;
        self.dispatch(AppIntent::SignInSubmitted { email, password });
    }

    fn show(&mut self, scope: Scope) {
        self.send(Intent::ScopeChanged(scope));
        self.cursor = 0;
    }

    fn send(&mut self, intent: Intent) {
        match &self.backend {
            Backend::Local(controller) => self.vm = block_on(controller.dispatch(intent)),
            Backend::Remote { .. } => {
                if let Some(ws) = self.open_workspace() {
                    self.dispatch(AppIntent::Tasks(ws, intent));
                }
            }
        }
        self.cursor = self.cursor.min(self.vm.rows.len().saturating_sub(1));
    }

    fn members(&mut self, intent: MembersIntent) {
        if let Some(ws) = self.open_workspace() {
            self.dispatch(AppIntent::Members(ws, intent));
        }
    }

    /// The workspace the screen shows, sent back with its intents.
    fn open_workspace(&self) -> Option<WorkspaceId> {
        match &self.app {
            Some(AppVm::SignedIn(s)) => s.workspace.clone(),
            _ => None,
        }
    }

    fn dispatch(&mut self, intent: AppIntent) {
        let Backend::Remote { app, runtime, .. } = &self.backend else { return };
        let vm = runtime.block_on(app.dispatch(intent));
        if let AppVm::SignedIn(s) = &vm {
            if let ScreenVm::Tasks(t) = &s.screen {
                self.vm = t.clone();
            }
            if let ScreenVm::Members(m) = &s.screen {
                self.member_cursor = self.member_cursor.min(m.rows.len().saturating_sub(1));
            }
        }
        if matches!(vm, AppVm::SignedOut(_)) && self.mode == Mode::Normal {
            self.mode = Mode::SignInEmail(String::new());
        }
        self.cursor = self.cursor.min(self.vm.rows.len().saturating_sub(1));
        self.app = Some(vm);
    }
}

fn edit(text: &mut String, key: Key) {
    match key {
        Key::Char(c) => text.push(c),
        Key::Backspace => {
            text.pop();
        }
        Key::Ctrl('u') => text.clear(),
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use taskr_core::shared::telemetry::NoTelemetry;
    use taskr_core::tasks::app::Tasks;
    use taskr_core::testing::{FixedClock, InMemoryTaskListRepository};

    pub fn remote_shell() -> Shell {
        use taskr_core::api::InProcessApi;
        use taskr_core::session::presentation::AppController;
        use taskr_core::shared::access::{Email, Principal, UserId};
        use taskr_core::testing::{FakeAuth, InMemoryWorkspaceRepository, MemorySessionStore};
        use taskr_core::workspaces::app::Workspaces;

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
        let app = AppController::new(
            Arc::new(FakeAuth),
            Arc::new(MemorySessionStore::default()),
            api.clone(),
            api,
            clock,
        );
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        Shell::remote(app, runtime, "http://test/api".into(), true)
    }

    pub fn shell() -> Shell {
        let tasks = Tasks::new(
            Arc::new(InMemoryTaskListRepository::default()),
            Arc::new(FixedClock::at(0)),
            Arc::new(NoTelemetry),
        );
        Shell::new(TaskListController::local(tasks))
    }

    pub fn type_text(s: &mut Shell, text: &str) {
        for c in text.chars() {
            s.handle(Key::Char(c));
        }
    }

    pub fn add(s: &mut Shell, title: &str) {
        s.handle(Key::Char('a'));
        type_text(s, title);
        s.handle(Key::Enter);
    }

    fn titles(s: &Shell) -> Vec<&str> {
        s.vm.rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn add_mode_collects_text_and_submits() {
        let mut s = shell();
        s.handle(Key::Char('a'));
        type_text(&mut s, "Buy milkk");
        s.handle(Key::Backspace);
        assert_eq!(s.mode, Mode::Adding("Buy milk".into()));
        s.handle(Key::Enter);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(titles(&s), ["Buy milk"]);
    }

    #[test]
    fn letters_typed_in_add_mode_are_not_commands() {
        let mut s = shell();
        add(&mut s, "quit jogging");
        assert!(!s.should_quit);
        assert_eq!(titles(&s), ["quit jogging"]);
    }

    #[test]
    fn vim_motion_moves_the_cursor_within_bounds() {
        let mut s = shell();
        for t in ["a", "b", "c"] {
            add(&mut s, t);
        }
        s.handle(Key::Char('j'));
        s.handle(Key::Char('j'));
        s.handle(Key::Char('j'));
        assert_eq!(s.cursor, 2);
        s.handle(Key::Char('g'));
        assert_eq!(s.cursor, 0);
        s.handle(Key::Char('G'));
        assert_eq!(s.selected_id(), Some(3));
    }

    #[test]
    fn space_completes_the_selected_task_and_cursor_stays_valid() {
        let mut s = shell();
        add(&mut s, "a");
        add(&mut s, "b");
        s.handle(Key::Char('G'));
        s.handle(Key::Char(' '));
        assert_eq!(titles(&s), ["a"]);
        assert_eq!(s.cursor, 0);
        s.handle(Key::Char('2'));
        assert_eq!(titles(&s), ["b"]);
    }

    #[test]
    fn filter_is_live_and_esc_clears_it() {
        let mut s = shell();
        add(&mut s, "Buy milk");
        add(&mut s, "Call mom");
        s.handle(Key::Char('/'));
        type_text(&mut s, "mom");
        assert_eq!(titles(&s), ["Call mom"]);
        s.handle(Key::Esc);
        assert_eq!(titles(&s).len(), 2);
    }

    #[test]
    fn tab_cycles_scopes() {
        let mut s = shell();
        let selected = |s: &Shell| s.vm.tabs.iter().position(|t| t.selected);
        s.handle(Key::Tab);
        assert_eq!(selected(&s), Some(1));
        s.handle(Key::Tab);
        s.handle(Key::Tab);
        assert_eq!(selected(&s), Some(0));
    }

    fn signed_in_shell() -> Shell {
        let mut s = remote_shell();
        type_text(&mut s, "ada@example.com");
        s.handle(Key::Enter);
        s
    }

    fn members(s: &Shell) -> Vec<(String, String)> {
        match s.screen() {
            Screen::Members(m) => {
                m.rows.iter().map(|r| (r.email.clone(), r.role_label.clone())).collect()
            }
            _ => panic!("not on the members screen"),
        }
    }

    #[test]
    fn signed_out_remote_starts_at_the_email_prompt() {
        let s = remote_shell();
        assert!(matches!(s.screen(), Screen::SignIn(_)));
        assert_eq!(s.mode, Mode::SignInEmail(String::new()));
        assert!(!s.signed_in());
    }

    #[test]
    fn dev_sign_in_opens_the_workspace_and_tasks_go_to_the_server() {
        let mut s = signed_in_shell();
        assert!(s.signed_in());
        assert_eq!(s.title(), Some("ada's workspace · Tasks"));
        add(&mut s, "Ship it");
        assert_eq!(titles(&s), ["Ship it"]);
    }

    #[test]
    fn members_screen_adds_changes_and_removes() {
        let mut s = signed_in_shell();
        s.handle(Key::Char('m'));
        assert_eq!(s.title(), Some("ada's workspace · Members"));
        s.handle(Key::Char('a'));
        type_text(&mut s, "bob@example.com viewer");
        s.handle(Key::Enter);
        assert_eq!(members(&s)[1], ("bob@example.com".into(), "Viewer".into()));

        s.handle(Key::Char('j'));
        s.handle(Key::Char('e'));
        assert_eq!(members(&s)[1].1, "Editor");
        s.handle(Key::Char('d'));
        assert_eq!(members(&s).len(), 1);

        s.handle(Key::Char('d'));
        match s.screen() {
            Screen::Members(m) => {
                assert_eq!(m.error.as_deref(), Some("A workspace needs at least one owner."))
            }
            _ => panic!(),
        }
        s.handle(Key::Char('t'));
        assert!(matches!(s.screen(), Screen::Tasks));
    }

    #[test]
    fn an_unknown_role_keeps_the_member_prompt_open() {
        let mut s = signed_in_shell();
        s.handle(Key::Char('m'));
        s.handle(Key::Char('a'));
        type_text(&mut s, "bob@example.com boss");
        s.handle(Key::Enter);
        assert_eq!(s.mode, Mode::AddingMember("bob@example.com boss".into()));
    }

    #[test]
    fn sign_out_returns_to_the_prompt() {
        let mut s = signed_in_shell();
        s.handle(Key::Char('L'));
        assert!(matches!(s.screen(), Screen::SignIn(_)));
        assert_eq!(s.mode, Mode::SignInEmail(String::new()));
    }

    #[test]
    fn server_keys_do_nothing_locally() {
        let mut s = shell();
        s.handle(Key::Char('m'));
        s.handle(Key::Char('w'));
        assert!(matches!(s.screen(), Screen::Tasks));
        assert_eq!(s.title(), None);
    }

    #[test]
    fn errors_come_from_the_shared_presentation() {
        let mut s = shell();
        add(&mut s, "   ");
        assert_eq!(s.vm.error.as_deref(), Some("A task needs a title."));
        s.handle(Key::Esc);
        assert_eq!(s.vm.error, None);
    }
}
