//! TUI shell state: only what is terminal specific (cursor, input mode, help
//! overlay). Everything else comes from the shared `TaskListVm`.

use taskr_core::tasks::app::Scope;
use taskr_core::tasks::presentation::{Intent, TaskListController, TaskListVm};

use crate::keymap::{Action, Key, action_for};

const HALF_PAGE: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Adding(String),
    Filtering(String),
}

pub struct Shell {
    controller: TaskListController,
    pub vm: TaskListVm,
    pub cursor: usize,
    pub mode: Mode,
    pub show_help: bool,
    pub should_quit: bool,
}

impl Shell {
    pub fn new(mut controller: TaskListController) -> Self {
        let vm = controller.dispatch(Intent::Opened);
        Self { controller, vm, cursor: 0, mode: Mode::Normal, show_help: false, should_quit: false }
    }

    pub fn tick(&mut self) {
        self.send(Intent::Refreshed);
    }

    pub fn selected_id(&self) -> Option<u32> {
        self.vm.rows.get(self.cursor).map(|r| r.id)
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
                if let Some(action) = action_for(key) {
                    self.apply(action);
                }
            }
            Mode::Adding(text) => self.handle_adding(key, text),
            Mode::Filtering(text) => self.handle_filtering(key, text),
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
            Action::Help => self.show_help = true,
            Action::Quit => self.should_quit = true,
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

    fn show(&mut self, scope: Scope) {
        self.send(Intent::ScopeChanged(scope));
        self.cursor = 0;
    }

    fn send(&mut self, intent: Intent) {
        self.vm = self.controller.dispatch(intent);
        self.cursor = self.cursor.min(self.vm.rows.len().saturating_sub(1));
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

    pub fn shell() -> Shell {
        let tasks = Tasks::new(
            Arc::new(InMemoryTaskListRepository::default()),
            Arc::new(FixedClock::at(0)),
            Arc::new(NoTelemetry),
        );
        Shell::new(TaskListController::new(tasks))
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

    #[test]
    fn errors_come_from_the_shared_presentation() {
        let mut s = shell();
        add(&mut s, "   ");
        assert_eq!(s.vm.error.as_deref(), Some("A task needs a title."));
        s.handle(Key::Esc);
        assert_eq!(s.vm.error, None);
    }
}
