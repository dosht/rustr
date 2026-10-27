//! Pure UI state and the key map. `update` turns one key into the intents and shell
//! requests it stands for; what a key may do right now (enabled, visible, refused)
//! is read from view-model fields, never decided here. The only state kept is what
//! a terminal needs by itself: which screen, which row the cursor is on, the text
//! being typed, and where the focus is.
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use transgate_core::presentation::app::{AppIntent, AppVm, AuthIntent, JobsIntent, SignedInVm, TranscriptIntent, UploadIntent};
use transgate_core::presentation::job_list::JobListVm;
use transgate_core::presentation::route::{HomeView, Route};
use transgate_core::presentation::upload_queue::{Choice, Mode, UploadQueueVm};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    SignIn,
    Jobs,
    Transcript,
    Upload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Keys are commands.
    Menu,
    Email,
    Password,
    /// The path prompt of the upload screen.
    Path,
}

/// What the shell must do for a key; intents go to the presenter, the rest is the host's.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Intent(AppIntent),
    /// Hold this file for upload (`TerminalPorts::pick`), then send `UploadIntent::Picked`.
    PickFile(PathBuf),
    GoogleSignIn,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub screen: Screen,
    pub focus: Focus,
    /// The highlighted job row.
    pub cursor: usize,
    /// The first transcript row shown.
    pub scroll: usize,
    pub email: String,
    pub password: String,
    pub path: String,
    /// A host-side fact worth showing (an OS error, the saved file, the sign-in link);
    /// raw text, never copy.
    pub notice: Option<String>,
    /// The authorization URL when no browser could be opened; stays until sign-in ends.
    pub link: Option<String>,
    /// Frame counter for the spinner glyph.
    pub tick: usize,
}

impl Default for UiState {
    fn default() -> UiState {
        UiState { screen: Screen::SignIn, focus: Focus::Email, cursor: 0, scroll: 0, email: String::new(), password: String::new(), path: String::new(), notice: None, link: None, tick: 0 }
    }
}

/// The commands of the key map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Down,
    Up,
    Open,
    Back,
    Refresh,
    LoadMore,
    Delete,
    Upload,
    Speakers,
    Mode,
    PrevTab,
    NextTab,
    PrevTarget,
    NextTarget,
    AutoDetect,
    Export,
    ExportAll,
    Google,
    Yes,
    No,
    Focus,
    Quit,
}

/// The whole menu key table.
const KEYS: &[(KeyCode, Command)] = &[
    (KeyCode::Char('j'), Command::Down),
    (KeyCode::Down, Command::Down),
    (KeyCode::Char('k'), Command::Up),
    (KeyCode::Up, Command::Up),
    (KeyCode::Enter, Command::Open),
    (KeyCode::Esc, Command::Back),
    (KeyCode::Char('r'), Command::Refresh),
    (KeyCode::Char('l'), Command::LoadMore),
    (KeyCode::Char('L'), Command::LoadMore),
    (KeyCode::Char('d'), Command::Delete),
    (KeyCode::Char('u'), Command::Upload),
    (KeyCode::Char('s'), Command::Speakers),
    (KeyCode::Char('t'), Command::Mode),
    (KeyCode::Char('['), Command::PrevTab),
    (KeyCode::Char(']'), Command::NextTab),
    (KeyCode::Char('<'), Command::PrevTarget),
    (KeyCode::Char('>'), Command::NextTarget),
    (KeyCode::Char('a'), Command::AutoDetect),
    (KeyCode::Char('e'), Command::Export),
    (KeyCode::Char('E'), Command::ExportAll),
    (KeyCode::Char('g'), Command::Google),
    (KeyCode::Char('y'), Command::Yes),
    (KeyCode::Char('n'), Command::No),
    (KeyCode::Tab, Command::Focus),
    (KeyCode::Char('q'), Command::Quit),
];

/// The intent families a key can send, each wrapped in `AppIntent`.
trait Wrap {
    fn wrap(self) -> AppIntent;
}

impl Wrap for AppIntent {
    fn wrap(self) -> AppIntent {
        self
    }
}

impl Wrap for AuthIntent {
    fn wrap(self) -> AppIntent {
        AppIntent::Auth(self)
    }
}

impl Wrap for JobsIntent {
    fn wrap(self) -> AppIntent {
        AppIntent::Jobs(self)
    }
}

impl Wrap for TranscriptIntent {
    fn wrap(self) -> AppIntent {
        AppIntent::Transcript(self)
    }
}

impl Wrap for UploadIntent {
    fn wrap(self) -> AppIntent {
        AppIntent::Upload(self)
    }
}

fn intent(intent: impl Wrap) -> Vec<Action> {
    vec![Action::Intent(intent.wrap())]
}

fn command(key: KeyEvent) -> Option<Command> {
    KEYS.iter().find(|(code, _)| *code == key.code).map(|(_, command)| *command)
}

impl UiState {
    /// Follows the view: a signed-out view is the sign-in screen; signing in leaves
    /// it; starting a transcription (the route becoming a job) opens the transcript;
    /// a cursor never points past the rows.
    pub fn sync(&mut self, vm: &AppVm) {
        match vm {
            AppVm::SignedOut(out) => {
                if self.screen != Screen::SignIn {
                    self.screen = Screen::SignIn;
                    self.focus = Focus::Email;
                    self.email = out.auth.email.clone();
                    self.password = out.auth.password.clone();
                }
            }
            AppVm::SignedIn(inn) => {
                if self.screen == Screen::SignIn {
                    self.screen = Screen::Jobs;
                    self.focus = Focus::Menu;
                    self.password.clear();
                }
                if self.screen == Screen::Upload && inn.route.job().is_some() {
                    self.screen = Screen::Transcript;
                    self.focus = Focus::Menu;
                    self.scroll = 0;
                }
                self.cursor = self.cursor.min(inn.jobs.rows.len().saturating_sub(1));
                if let Some(t) = &inn.transcript {
                    self.scroll = self.scroll.min(t.rows.len().saturating_sub(1));
                }
            }
            AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } | AppVm::Erased { .. } => {}
        }
    }

    pub fn update(&mut self, vm: &AppVm, key: KeyEvent) -> Vec<Action> {
        if key.kind == KeyEventKind::Release {
            return vec![];
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return if key.code == KeyCode::Char('c') { vec![Action::Quit] } else { vec![] };
        }
        self.notice = None;
        match vm {
            AppVm::SignedOut(_) => self.sign_in(key),
            AppVm::SignedIn(inn) => match self.screen {
                Screen::SignIn => vec![],
                Screen::Jobs => self.jobs(&inn.jobs, key),
                Screen::Transcript => self.transcript(inn, key),
                Screen::Upload => self.upload(&inn.queue, key),
            },
            AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } | AppVm::Erased { .. } => match command(key) {
                Some(Command::Quit) => vec![Action::Quit],
                _ => vec![],
            },
        }
    }

    /// Typing into a text field; `None` when the key is not text editing.
    fn typing(text: &mut String, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) => text.push(c),
            KeyCode::Backspace => {
                text.pop();
            }
            _ => return false,
        }
        true
    }

    fn sign_in(&mut self, key: KeyEvent) -> Vec<Action> {
        match self.focus {
            Focus::Email | Focus::Password => {
                let email = self.focus == Focus::Email;
                if Self::typing(if email { &mut self.email } else { &mut self.password }, key) {
                    return if email {
                        intent(AuthIntent::EmailChanged { text: self.email.clone() })
                    } else {
                        intent(AuthIntent::PasswordChanged { text: self.password.clone() })
                    };
                }
                match key.code {
                    KeyCode::Tab => {
                        self.focus = if email { Focus::Password } else { Focus::Menu };
                        vec![]
                    }
                    KeyCode::Esc => {
                        self.focus = Focus::Menu;
                        vec![]
                    }
                    KeyCode::Enter if email => {
                        self.focus = Focus::Password;
                        vec![]
                    }
                    KeyCode::Enter => intent(AuthIntent::Submitted),
                    _ => vec![],
                }
            }
            _ => match command(key) {
                Some(Command::Open) => intent(AuthIntent::Submitted),
                Some(Command::Google) => vec![Action::GoogleSignIn],
                Some(Command::Focus) => {
                    self.focus = Focus::Email;
                    vec![]
                }
                Some(Command::Quit) => vec![Action::Quit],
                _ => vec![],
            },
        }
    }

    fn jobs(&mut self, jobs: &JobListVm, key: KeyEvent) -> Vec<Action> {
        let command = command(key);
        // A pending delete owns the keyboard: confirm, cancel, nothing else.
        if jobs.confirm_delete.is_some() {
            return match command {
                Some(Command::Yes) => intent(JobsIntent::DeleteConfirmed),
                Some(Command::No | Command::Back) => intent(JobsIntent::DeleteCanceled),
                _ => vec![],
            };
        }
        let row = jobs.rows.get(self.cursor);
        match command {
            Some(Command::Down) => {
                self.cursor = (self.cursor + 1).min(jobs.rows.len().saturating_sub(1));
                vec![]
            }
            Some(Command::Up) => {
                self.cursor = self.cursor.saturating_sub(1);
                vec![]
            }
            Some(Command::Open) => match row {
                Some(row) => {
                    self.screen = Screen::Transcript;
                    self.scroll = 0;
                    intent(AppIntent::Navigate { route: Route::Home(HomeView::Job(row.id)) })
                }
                None => vec![],
            },
            Some(Command::Refresh) => intent(JobsIntent::Refresh),
            Some(Command::LoadMore) if jobs.can_load_more => intent(JobsIntent::LoadMore),
            Some(Command::Delete) => match row {
                Some(row) => intent(JobsIntent::DeleteRequested { job: row.id }),
                None => vec![],
            },
            Some(Command::Export) => match row {
                Some(row) if row.can_download => intent(JobsIntent::DownloadRequested { job: row.id }),
                _ => vec![],
            },
            Some(Command::Upload) => {
                self.screen = Screen::Upload;
                self.focus = Focus::Path;
                vec![]
            }
            Some(Command::Quit) => vec![Action::Quit],
            _ => vec![],
        }
    }

    fn transcript(&mut self, vm: &SignedInVm, key: KeyEvent) -> Vec<Action> {
        let Some(t) = &vm.transcript else {
            return match command(key) {
                Some(Command::Back) => self.leave_transcript(),
                Some(Command::Quit) => vec![Action::Quit],
                _ => vec![],
            };
        };
        match command(key) {
            Some(Command::Down) => {
                self.scroll = (self.scroll + 1).min(t.rows.len().saturating_sub(1));
                vec![]
            }
            Some(Command::Up) => {
                self.scroll = self.scroll.saturating_sub(1);
                vec![]
            }
            Some(Command::Back) => self.leave_transcript(),
            Some(Command::Export) if t.download_enabled => intent(TranscriptIntent::DownloadPressed { all_formats: false }),
            Some(Command::ExportAll) if t.download_enabled => intent(TranscriptIntent::DownloadPressed { all_formats: true }),
            Some(command @ (Command::PrevTab | Command::NextTab)) if t.show_tabs && !t.tabs.is_empty() => {
                let here = t.selected.as_ref().and_then(|s| t.tabs.iter().position(|o| &o.tab == s)).unwrap_or(0);
                let step = if command == Command::NextTab { 1 } else { t.tabs.len() - 1 };
                intent(TranscriptIntent::TabSelected { tab: t.tabs[(here + step) % t.tabs.len()].tab.clone() })
            }
            Some(Command::Mode) if t.time_toggle => intent(TranscriptIntent::ShowTimeToggled { on: !t.show_time }),
            Some(Command::Speakers) if t.speaker_toggle => intent(TranscriptIntent::ShowSpeakersToggled { on: !t.show_speakers }),
            Some(Command::Quit) => vec![Action::Quit],
            _ => vec![],
        }
    }

    fn leave_transcript(&mut self) -> Vec<Action> {
        self.screen = Screen::Jobs;
        intent(AppIntent::Navigate { route: Route::Home(HomeView::Default) })
    }

    fn upload(&mut self, queue: &UploadQueueVm, key: KeyEvent) -> Vec<Action> {
        if self.focus == Focus::Path {
            if Self::typing(&mut self.path, key) {
                return vec![];
            }
            return match key.code {
                KeyCode::Enter => {
                    let path = self.path.trim().to_string();
                    self.path.clear();
                    if path.is_empty() { vec![] } else { vec![Action::PickFile(PathBuf::from(path))] }
                }
                KeyCode::Esc | KeyCode::Tab => {
                    self.focus = Focus::Menu;
                    vec![]
                }
                _ => vec![],
            };
        }
        match command(key) {
            Some(Command::Back) => {
                self.screen = Screen::Jobs;
                vec![]
            }
            Some(Command::Open) if !queue.start_disabled => intent(UploadIntent::StartPressed),
            Some(Command::Speakers) => intent(UploadIntent::SpeakersToggled { enabled: !queue.speakers }),
            Some(Command::Mode) => intent(UploadIntent::TabChanged {
                mode: match queue.mode {
                    Mode::Transcription => Mode::Translation,
                    Mode::Translation => Mode::Transcription,
                },
            }),
            Some(Command::AutoDetect) if queue.auto_detect_offered => intent(UploadIntent::LanguageChosen { choice: Some(Choice::AutoDetect) }),
            Some(command @ (Command::PrevTab | Command::NextTab)) => {
                let forward = command == Command::NextTab;
                match queue.mode {
                    Mode::Transcription => {
                        let items: Vec<_> = queue.available.iter().map(|o| o.language).collect();
                        let current = match queue.language {
                            Some(Choice::Language(l)) => Some(l),
                            _ => None,
                        };
                        match cycle(&items, current, forward) {
                            Some(l) => intent(UploadIntent::LanguageChosen { choice: Some(Choice::Language(l)) }),
                            None => vec![],
                        }
                    }
                    Mode::Translation => {
                        let items: Vec<_> = queue.source_languages.iter().map(|o| o.language).collect();
                        match cycle(&items, queue.source_language, forward) {
                            Some(l) => intent(UploadIntent::SourceLanguageChosen { language: Some(l) }),
                            None => vec![],
                        }
                    }
                }
            }
            Some(command @ (Command::PrevTarget | Command::NextTarget)) if queue.mode == Mode::Translation => {
                let items: Vec<_> = queue.target_languages.iter().map(|o| o.language).collect();
                match cycle(&items, queue.target_language, command == Command::NextTarget) {
                    Some(l) => intent(UploadIntent::TargetLanguageChosen { language: Some(l) }),
                    None => vec![],
                }
            }
            Some(Command::Upload | Command::Focus) => {
                self.focus = Focus::Path;
                vec![]
            }
            Some(Command::Quit) => vec![Action::Quit],
            _ => vec![],
        }
    }
}

/// The neighbour of `current` in `items` (wrapping); the first or last when nothing is chosen.
fn cycle<T: Copy + PartialEq>(items: &[T], current: Option<T>, forward: bool) -> Option<T> {
    if items.is_empty() {
        return None;
    }
    let at = current.and_then(|c| items.iter().position(|i| *i == c));
    let next = match (at, forward) {
        (Some(i), true) => (i + 1) % items.len(),
        (Some(i), false) => (i + items.len() - 1) % items.len(),
        (None, true) => 0,
        (None, false) => items.len() - 1,
    };
    Some(items[next])
}

#[cfg(test)]
mod tests;
