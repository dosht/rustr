//! The seam between the terminal loop and the pure parts: one `step` follows the
//! view, applies at most one key and draws one frame. `main` calls it with a real
//! terminal; tests call it with a `TestBackend`.
use std::process::ExitCode;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEvent};
use transgate_core::presentation::app::{AppVm, SESSION_UNAVAILABLE};

use crate::config;
use crate::design_tokens::{DEFAULT_DARK, DEFAULT_LIGHT, Tokens};
use crate::host::{Exit, Flow, Host, exit_for, reachable};
use crate::ui::{Action, UiState};
use crate::view;

const POLL: Duration = Duration::from_millis(100);
const REACH_TIMEOUT: Duration = Duration::from_secs(3);

/// Light tokens on a light terminal, dark otherwise. `COLORFGBG` is `fg;bg` with ANSI
/// color indices; backgrounds 7 and 9..=15 are light.
pub fn tokens_for(colorfgbg: Option<&str>) -> &'static Tokens {
    let light = colorfgbg.and_then(|v| v.rsplit(';').next()).and_then(|bg| bg.parse::<u8>().ok()).is_some_and(|bg| bg == 7 || (9..=15).contains(&bg));
    if light { &DEFAULT_LIGHT } else { &DEFAULT_DARK }
}

/// Follows `vm`, applies `key` when there is one, draws, and returns what the key asked for.
pub fn step<B: Backend>(terminal: &mut Terminal<B>, vm: &AppVm, ui: &mut UiState, key: Option<KeyEvent>, locale: &str, tokens: &Tokens) -> Result<Vec<Action>, B::Error> {
    ui.sync(vm);
    let actions = key.map(|key| ui.update(vm, key)).unwrap_or_default();
    ui.tick = ui.tick.wrapping_add(1);
    terminal.draw(|frame| view::render(frame, vm, ui, locale, tokens))?;
    Ok(actions)
}

/// Puts the terminal back on every exit path: a normal return, an error and a panic
/// (`ratatui::try_init` also installs a panic hook that restores before the report prints).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Draws and reads keys until a quit; returns the last view, which decides the exit code.
fn event_loop(host: &mut Host, terminal: &mut ratatui::DefaultTerminal, ui: &mut UiState, tokens: &Tokens) -> Result<AppVm, String> {
    let locale = host.locale().to_string();
    loop {
        // One turn: host facts, the current view, a frame, then at most one key. A view
        // change from the presenter's thread is picked up within one poll interval.
        host.poll(ui);
        let mut vm = host.view()?;
        step(terminal, &vm, ui, None, &locale, tokens).map_err(|e| e.to_string())?;
        let key = if event::poll(POLL).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) => Some(key),
                _ => None,
            }
        } else {
            None
        };
        let Some(key) = key else { continue };
        vm = host.view()?;
        for action in step(terminal, &vm, ui, Some(key), &locale, tokens).map_err(|e| e.to_string())? {
            if host.apply(action, ui) == Flow::Quit {
                return host.view();
            }
        }
    }
}

fn fail(exit: Exit, message: impl std::fmt::Display) -> ExitCode {
    eprintln!("{message}");
    exit.into()
}

/// The process: configuration (exit 2 when it is wrong), the host, the terminal, the
/// loop, and the exit code the last view names (P8 D13).
pub fn run(args: impl IntoIterator<Item = String>, env: impl Fn(&str) -> Option<String>) -> ExitCode {
    let config = match config::parse(args, &env) {
        Ok(config) => config,
        Err(error) => return fail(Exit::Usage, format_args!("{error}\n{}", config::USAGE)),
    };
    let tokens = tokens_for(env("COLORFGBG").as_deref());
    if !reachable(&config.host, REACH_TIMEOUT) {
        return fail(Exit::Network, view::say(&config.locale, &SESSION_UNAVAILABLE));
    }
    let mut host = match Host::start(config) {
        Ok(host) => host,
        Err(error) => return fail(Exit::Refused, error),
    };
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => return fail(Exit::Refused, error),
    };
    let guard = TerminalGuard;
    let mut ui = UiState::default();
    let result = event_loop(&mut host, &mut terminal, &mut ui, tokens);
    drop(guard);
    match result {
        Ok(vm) => exit_for(&vm).into(),
        Err(error) => fail(Exit::Refused, error),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use transgate_core::presentation::fixtures;

    use super::*;
    use crate::ui::Screen;

    #[test]
    fn colorfgbg_picks_the_token_set() {
        assert_eq!(tokens_for(Some("0;15")).bg, DEFAULT_LIGHT.bg);
        assert_eq!(tokens_for(Some("15;0")).bg, DEFAULT_DARK.bg);
        assert_eq!(tokens_for(Some("0;default;7")).bg, DEFAULT_LIGHT.bg);
        assert_eq!(tokens_for(None).bg, DEFAULT_DARK.bg);
        assert_eq!(tokens_for(Some("junk")).bg, DEFAULT_DARK.bg);
    }

    #[test]
    fn a_step_follows_the_view_applies_the_key_and_draws() {
        let vm = fixtures::app_fixtures().into_iter().find(|(name, _)| *name == "desktop_new").unwrap().1;
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut ui = UiState::default();
        let none = step(&mut terminal, &vm, &mut ui, None, "en", &DEFAULT_DARK).unwrap();
        assert!(none.is_empty());
        assert_eq!(ui.screen, Screen::Jobs, "a signed-in view leaves the sign-in screen");
        let key = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE);
        step(&mut terminal, &vm, &mut ui, Some(key), "en", &DEFAULT_DARK).unwrap();
        assert_eq!(ui.screen, Screen::Upload);
        let quit = step(&mut terminal, &vm, &mut ui, Some(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), "en", &DEFAULT_DARK).unwrap();
        assert_eq!(quit, vec![Action::Quit]);
    }
}
