//! Composition root for the TUI: adapters in, event loop out.

mod keymap;
mod shell;
mod view;

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use taskr_core::shared::telemetry::{Level, LogEvent, NoTelemetry, Telemetry};
use taskr_core::tasks::app::Tasks;
use taskr_core::tasks::presentation::TaskListController;
use taskr_infra::clock::SystemClock;
use taskr_infra::json_file::JsonFileTaskListRepository;
use taskr_infra::paths;
use taskr_infra::telemetry::JsonLinesTelemetry;

use keymap::Key;
use shell::Shell;

/// Reloading on a timer picks up changes made from the CLI meanwhile.
const RELOAD_EVERY: Duration = Duration::from_secs(3);

fn main() -> io::Result<()> {
    let home = paths::data_dir();
    let clock = Arc::new(SystemClock);
    let telemetry: Arc<dyn Telemetry> =
        match JsonLinesTelemetry::append_to(&paths::log_file(&home), Level::Info, clock.clone()) {
            Ok(t) => Arc::new(t),
            Err(_) => Arc::new(NoTelemetry),
        };
    telemetry.record(LogEvent::info("app.started").field("shell", "tui"));
    let repo = Arc::new(JsonFileTaskListRepository::new(paths::tasks_file(&home)));
    let shell = Shell::new(TaskListController::new(Tasks::new(repo, clock, telemetry)));

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, shell);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, mut shell: Shell) -> io::Result<()> {
    let mut last_reload = Instant::now();
    while !shell.should_quit {
        terminal.draw(|f| view::draw(f, &shell))?;
        let timeout = RELOAD_EVERY.saturating_sub(last_reload.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(key) = translate(key)
        {
            shell.handle(key);
        }
        if last_reload.elapsed() >= RELOAD_EVERY {
            shell.tick();
            last_reload = Instant::now();
        }
    }
    Ok(())
}

fn translate(key: KeyEvent) -> Option<Key> {
    Some(match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => Key::Ctrl(c),
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        _ => return None,
    })
}
