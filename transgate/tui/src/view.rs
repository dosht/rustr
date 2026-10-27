//! Rendering. Every function takes a core view model (or a piece of one) and draws
//! it: labels, tones, enablement and visibility are the model's fields; copy is
//! `transgate_i18n::translate`. A message the catalog cannot translate is shown as
//! `⟦ns.key⟧` so the gap is visible; there is no fallback language.
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use transgate_core::presentation::app::{AppVm, AuthVm, SignedInVm};
use transgate_core::presentation::job_list::{JobListVm, JobRow, status_tone};
use transgate_core::presentation::msg::{Msg, app, auth, common};
use transgate_core::presentation::transcript::TranscriptVm;
use transgate_core::presentation::upload_queue::{Choice, FileRow, Mode, UploadQueueVm};
use transgate_core::transcript::SpeakerLabel;

use crate::design_tokens::Tokens;
use crate::ui::{Focus, Screen, UiState};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const CURSOR: &str = "▏";
const MASK: char = '•';
const NAME_WIDTH: usize = 28;

/// The locale and the tokens every drawing function needs.
pub struct Cx<'a> {
    pub locale: &'a str,
    pub tokens: &'a Tokens,
}

impl Cx<'_> {
    pub fn say(&self, msg: &Msg) -> String {
        say(self.locale, msg)
    }

    fn base(&self) -> Style {
        Style::new().fg(self.tokens.fg).bg(self.tokens.bg)
    }

    fn muted(&self) -> Style {
        self.base().fg(self.tokens.muted)
    }

    fn error(&self) -> Style {
        self.base().fg(self.tokens.error)
    }

    fn accent(&self) -> Style {
        self.base().fg(self.tokens.accent)
    }
}

pub fn say(locale: &str, msg: &Msg) -> String {
    transgate_i18n::translate(locale, msg).unwrap_or_else(|_| format!("⟦{}.{}⟧", msg.ns.file(), msg.key))
}

/// A key and what it does; the label is a catalog key when the catalog has a fitting
/// one, else only the glyph shows (the missing words are reported, not invented).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub glyph: &'static str,
    pub label: Option<Msg>,
}

fn hint(glyph: &'static str, label: Option<Msg>) -> Hint {
    Hint { glyph, label }
}

/// The key hints that apply to the current view, in display order.
pub fn hints(vm: &AppVm, ui: &UiState) -> Vec<Hint> {
    match vm {
        AppVm::SignedOut(_) => match ui.focus {
            Focus::Email | Focus::Password => vec![hint("⇥", None), hint("⏎", Some(auth("sign_in.button_label"))), hint("Esc", None)],
            _ => vec![
                hint("g", Some(auth("sign_in.social_provider_text").text("provider", GOOGLE))),
                hint("⏎", Some(auth("sign_in.button_label"))),
                hint("⇥", None),
                hint("q", None),
            ],
        },
        AppVm::SignedIn(inn) => match ui.screen {
            Screen::Jobs if inn.jobs.confirm_delete.is_some() => vec![hint("y", Some(common("Yes"))), hint("n", Some(common("No")))],
            Screen::Jobs => {
                let mut list = vec![hint("↑↓", None), hint("⏎", Some(common("Select"))), hint("r", None)];
                if inn.jobs.can_load_more {
                    list.push(hint("L", Some(common("Load"))));
                }
                list.extend([hint("d", Some(common("Delete"))), hint("e", Some(common("Download"))), hint("u", Some(common("Upload File"))), hint("q", None)]);
                list
            }
            Screen::Transcript => {
                let mut list = vec![hint("Esc", Some(common("Back"))), hint("↑↓", None)];
                if let Some(t) = &inn.transcript {
                    if t.download_enabled {
                        list.extend([hint("e", Some(common("Download"))), hint("E", None)]);
                    }
                    if t.show_tabs {
                        list.push(hint("[ ]", None));
                    }
                    if t.time_toggle {
                        list.push(hint("t", Some(common("Show Time"))));
                    }
                    if t.speaker_toggle {
                        list.push(hint("s", Some(common("Speakers"))));
                    }
                }
                list.push(hint("q", None));
                list
            }
            Screen::Upload if ui.focus == Focus::Path => vec![hint("⏎", Some(common("Upload File"))), hint("Esc", None)],
            Screen::Upload => {
                let q = &inn.queue;
                let mut list = vec![hint("Esc", Some(common("Back"))), hint("⇥", Some(common("Upload File")))];
                if !q.start_disabled {
                    list.push(hint("⏎", Some(start_label(q.mode))));
                }
                list.extend([hint("s", Some(common("Speakers"))), hint("t", Some(mode_label(other_mode(q.mode)))), hint("[ ]", None)]);
                if q.mode == Mode::Translation {
                    list.push(hint("< >", None));
                }
                if q.auto_detect_offered {
                    list.push(hint("a", Some(q.auto_detect_label.clone())));
                }
                list.push(hint("q", None));
                list
            }
            Screen::SignIn => vec![],
        },
        AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } | AppVm::Erased { .. } => vec![hint("q", None)],
    }
}

/// The identity provider's own name, not copy.
const GOOGLE: &str = "Google";

fn other_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Transcription => Mode::Translation,
        Mode::Translation => Mode::Transcription,
    }
}

fn mode_label(mode: Mode) -> Msg {
    match mode {
        Mode::Transcription => common("Transcription"),
        Mode::Translation => common("Translation"),
    }
}

fn start_label(mode: Mode) -> Msg {
    match mode {
        Mode::Transcription => common("Start Transcribing"),
        Mode::Translation => common("Start Translating"),
    }
}

/// Every message the hints of every screen can carry, for the all-locales check.
pub fn hint_messages() -> Vec<Msg> {
    let mut all = vec![
        auth("sign_in.button_label"),
        auth("sign_in.social_provider_text").text("provider", GOOGLE),
        common("Yes"),
        common("No"),
        common("Select"),
        common("Load"),
        common("Delete"),
        common("Download"),
        common("Upload File"),
        common("Back"),
        common("Show Time"),
        common("Speakers"),
    ];
    for mode in [Mode::Transcription, Mode::Translation] {
        all.push(mode_label(mode));
        all.push(start_label(mode));
    }
    all
}

pub fn render(frame: &mut Frame, vm: &AppVm, ui: &UiState, locale: &str, tokens: &Tokens) {
    let cx = Cx { locale, tokens };
    let area = frame.area();
    frame.render_widget(Block::new().style(cx.base()), area);
    let [body, notice, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(area);
    match vm {
        AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } => render_message(frame, body, &common("Loading..."), &cx),
        AppVm::Erased { message, .. } => render_message(frame, body, message, &cx),
        AppVm::SignedOut(out) => render_auth(frame, body, &out.auth, ui, &cx),
        AppVm::SignedIn(inn) => render_signed_in(frame, body, inn, ui, &cx),
    }
    if let Some(text) = ui.notice.as_ref().or(ui.link.as_ref()) {
        frame.render_widget(Paragraph::new(text.as_str()).style(cx.error()), notice);
    }
    frame.render_widget(Paragraph::new(hint_line(&hints(vm, ui), &cx)), footer);
}

fn hint_line(hints: &[Hint], cx: &Cx) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", cx.muted()));
        }
        spans.push(Span::styled(h.glyph, cx.accent().add_modifier(Modifier::BOLD)));
        if let Some(label) = &h.label {
            spans.push(Span::styled(format!(" {}", cx.say(label)), cx.muted()));
        }
    }
    Line::from(spans)
}

fn render_message(frame: &mut Frame, area: Rect, msg: &Msg, cx: &Cx) {
    frame.render_widget(Paragraph::new(cx.say(msg)).style(cx.muted()).alignment(Alignment::Center), area);
}

fn render_signed_in(frame: &mut Frame, area: Rect, vm: &SignedInVm, ui: &UiState, cx: &Cx) {
    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    let header_line = match (&vm.session_error, &vm.profile) {
        (Some(error), _) => Line::styled(cx.say(error), cx.error()),
        (None, Some(profile)) => Line::from(vec![Span::styled(profile.email.clone(), cx.muted()), Span::styled(format!("  {}", cx.say(&vm.balance)), cx.accent())]),
        (None, None) => Line::styled(cx.say(&common("Loading...")), cx.muted()),
    };
    frame.render_widget(Paragraph::new(header_line), header);
    match ui.screen {
        Screen::Transcript => match &vm.transcript {
            Some(t) => render_transcript(frame, body, t, ui, cx),
            None => render_message(frame, body, &common("Loading..."), cx),
        },
        Screen::Upload => render_queue(frame, body, &vm.queue, ui, cx),
        Screen::Jobs | Screen::SignIn => render_jobs(frame, body, &vm.jobs, ui, cx),
    }
}

// ---- sign in -------------------------------------------------------------------------------

pub fn render_auth(frame: &mut Frame, area: Rect, vm: &AuthVm, ui: &UiState, cx: &Cx) {
    let block = Block::bordered().title(cx.say(&auth("sign_in.button_label"))).border_style(cx.muted()).style(cx.base());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let focus = |f: Focus| if ui.focus == f { cx.accent().add_modifier(Modifier::BOLD) } else { cx.muted() };
    let caret = |f: Focus| if ui.focus == f { CURSOR } else { "" };
    let masked: String = vm.password.chars().map(|_| MASK).collect();
    let mut lines = vec![
        Line::styled(cx.say(&auth("sign_in.email_label")), focus(Focus::Email)),
        Line::styled(format!("{}{}", vm.email, caret(Focus::Email)), cx.base()),
        Line::raw(""),
        Line::styled(cx.say(&auth("sign_in.password_label")), focus(Focus::Password)),
        Line::styled(format!("{masked}{}", caret(Focus::Password)), cx.base()),
        Line::raw(""),
    ];
    let button = if vm.busy { auth("sign_in.loading_button_label") } else { auth("sign_in.button_label") };
    let button_style = if vm.can_submit && !vm.busy { cx.accent().add_modifier(Modifier::BOLD) } else { cx.muted() };
    lines.push(Line::styled(format!("[ {} ]", cx.say(&button)), button_style));
    lines.push(Line::styled(cx.say(&auth("sign_in.social_provider_text").text("provider", GOOGLE)), cx.muted()));
    if let Some(error) = &vm.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(cx.say(error), cx.error()));
    }
    if let Some(notice) = &vm.notice {
        lines.push(Line::raw(""));
        lines.push(Line::styled(cx.say(notice), cx.base().bg(cx.tokens.notice_bg)));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ---- jobs ----------------------------------------------------------------------------------

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        format!("{text:<width$}")
    } else {
        let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

fn job_line(row: &JobRow, vm: &JobListVm, ui: &UiState, cx: &Cx) -> Line<'static> {
    let spinner = if row.spinner { SPINNER[ui.tick % SPINNER.len()] } else { " " };
    let name = match &row.display_name {
        Some(name) => name.clone(),
        None => cx.say(&vm.untitled),
    };
    let duration = match &row.duration {
        Some(d) => d.clone(),
        None => cx.say(&vm.not_available),
    };
    let language = match &row.language {
        Some(name) => format!("{} {name}", row.flag.as_deref().unwrap_or("")).trim().to_string(),
        None => cx.say(&vm.auto_detect),
    };
    let tone = cx.base().fg(cx.tokens.tone(row.tone));
    Line::from(vec![
        Span::styled(format!("{spinner} "), tone),
        Span::styled(clip(&name, NAME_WIDTH), if row.selected { cx.accent().add_modifier(Modifier::BOLD) } else { cx.base() }),
        Span::styled(format!(" {} ", clip(&cx.say(&row.status_label), 16)), tone),
        Span::styled(format!("{duration:>6}  {language}  {}", row.date_text), cx.muted()),
    ])
}

pub fn render_jobs(frame: &mut Frame, area: Rect, vm: &JobListVm, ui: &UiState, cx: &Cx) {
    let mut lines_below: Vec<Line> = Vec::new();
    if vm.initial_loading {
        lines_below.push(Line::styled(format!("{} {}", SPINNER[ui.tick % SPINNER.len()], cx.say(&app("Loading jobs..."))), cx.muted()));
    }
    if vm.loading_more {
        lines_below.push(Line::styled(format!("{} {}", SPINNER[ui.tick % SPINNER.len()], cx.say(&app("Loading more..."))), cx.muted()));
    }
    if let Some(error) = &vm.error {
        lines_below.push(Line::styled(cx.say(error), cx.error()));
        if let Some(detail) = &vm.error_detail {
            lines_below.push(Line::styled(detail.clone(), cx.error()));
        }
    }
    if vm.empty && !vm.initial_loading && vm.error.is_none() {
        lines_below.push(Line::styled(cx.say(&app("No transcripts found")), cx.muted()));
        lines_below.push(Line::styled(cx.say(&app("Start by creating your first transcript")), cx.muted()));
    }
    if let Some(end) = &vm.end_of_list {
        lines_below.push(Line::styled(cx.say(end), cx.muted()));
    }
    let below = u16::try_from(lines_below.len()).unwrap_or(u16::MAX).min(area.height.saturating_sub(1));
    let [list_area, below_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(below)]).areas(area);
    let items: Vec<ListItem> = vm.rows.iter().map(|row| ListItem::new(job_line(row, vm, ui, cx))).collect();
    let mut state = ListState::default();
    if !vm.rows.is_empty() {
        state.select(Some(ui.cursor.min(vm.rows.len() - 1)));
    }
    let list = List::new(items).style(cx.base()).highlight_style(Style::new().bg(cx.tokens.cursor_bg));
    frame.render_stateful_widget(list, list_area, &mut state);
    frame.render_widget(Paragraph::new(lines_below), below_area);
    if let Some(job) = vm.confirm_delete {
        let name = vm.rows.iter().find(|r| r.id == job).map(|r| r.display_name.clone().unwrap_or_else(|| cx.say(&vm.untitled))).unwrap_or_default();
        render_confirm(frame, area, &name, cx);
    }
}

fn render_confirm(frame: &mut Frame, area: Rect, name: &str, cx: &Cx) {
    let width = area.width.min(64);
    let height = area.height.min(5);
    let popup = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().border_style(Style::new().fg(cx.tokens.error)).style(Style::new().fg(cx.tokens.fg).bg(cx.tokens.error_bg));
    let text = Text::from(vec![Line::raw(cx.say(&common("Are you sure you want to delete this file?"))), Line::styled(name.to_string(), Style::new().add_modifier(Modifier::BOLD))]);
    frame.render_widget(Paragraph::new(text).block(block).wrap(Wrap { trim: false }), popup);
}

// ---- transcript ----------------------------------------------------------------------------

/// `fg` moved toward `bg` by `100 - percent`; a color that is not RGB cannot be blended, so it dims.
pub fn faded(style: Style, fg: Color, bg: Color, percent: u8) -> Style {
    let percent = u16::from(percent.min(100));
    match (fg, bg) {
        (Color::Rgb(r, g, b), Color::Rgb(r2, g2, b2)) => {
            let mix = |a: u8, c: u8| ((u16::from(a) * percent + u16::from(c) * (100 - percent)) / 100) as u8;
            style.fg(Color::Rgb(mix(r, r2), mix(g, g2), mix(b, b2)))
        }
        _ => style.add_modifier(Modifier::DIM),
    }
}

fn speaker_text(label: &SpeakerLabel, cx: &Cx) -> Option<String> {
    match label {
        SpeakerLabel::Numbered { number } => Some(format!("{} {number}", cx.say(&common("Speaker")))),
        SpeakerLabel::Named { name } => Some(name.clone()),
        SpeakerLabel::None => None,
    }
}

pub fn render_transcript(frame: &mut Frame, area: Rect, vm: &TranscriptVm, ui: &UiState, cx: &Cx) {
    let tab_height = u16::from(vm.show_tabs);
    let [head, tabs, body] = Layout::vertical([Constraint::Length(1), Constraint::Length(tab_height), Constraint::Min(1)]).areas(area);
    let mut title = vec![Span::styled(vm.title.clone(), cx.base().add_modifier(Modifier::BOLD))];
    if let Some(minutes) = vm.duration_minutes {
        title.push(Span::styled(format!("  {minutes} {}", cx.say(&vm.minutes_unit)), cx.muted()));
    }
    if let Some(badge) = &vm.status_badge {
        let spin = if badge.spinning { format!("{} ", SPINNER[ui.tick % SPINNER.len()]) } else { String::new() };
        title.push(Span::styled(format!("  {spin}{}", cx.say(&badge.label)), cx.base().fg(cx.tokens.tone(status_tone(badge.status)))));
    }
    frame.render_widget(Paragraph::new(Line::from(title)), head);
    if vm.show_tabs {
        let current = vm.selected.as_ref().and_then(|s| vm.tabs.iter().position(|o| &o.tab == s)).unwrap_or(0);
        let mut spans = Vec::new();
        for (i, option) in vm.tabs.iter().enumerate() {
            let style = if i == current { cx.accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { cx.muted() };
            spans.push(Span::styled(format!(" {} {} ", option.flag, option.name).replace("  ", " "), style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), tabs);
    }

    let align = if vm.rtl { Alignment::Right } else { Alignment::Left };
    let mut lines: Vec<Line> = Vec::new();
    if let Some(message) = &vm.loading_message {
        lines.push(Line::styled(format!("{} {}", SPINNER[ui.tick % SPINNER.len()], cx.say(message)), cx.base().fg(cx.tokens.progress)));
    }
    if let Some(error) = &vm.load_error {
        lines.push(Line::styled(cx.say(error), cx.error()));
        if let Some(detail) = &vm.load_error_detail {
            lines.push(Line::styled(detail.clone(), cx.error()));
        }
    }
    if let Some(empty) = &vm.empty {
        lines.push(Line::styled(cx.say(empty), cx.muted()));
    }
    for (index, row) in vm.rows.iter().enumerate().skip(ui.scroll) {
        let fade = vm.fades.iter().find(|(i, _)| *i == index).map(|(_, percent)| *percent);
        let paint = |style: Style| match fade {
            Some(percent) => faded(style, style.fg.unwrap_or(cx.tokens.fg), cx.tokens.bg, percent),
            None => style,
        };
        let mut meta = Vec::new();
        if vm.show_time {
            meta.push(row.time_text.clone());
        }
        if vm.show_speakers {
            if row.speaker_pending {
                meta.push("…".to_string());
            } else if let Some(text) = row.speaker.as_ref().and_then(|l| speaker_text(l, cx)) {
                meta.push(text);
            }
        }
        if !meta.is_empty() {
            lines.push(Line::styled(meta.join("  "), paint(cx.muted())).alignment(align));
        }
        lines.push(Line::styled(row.text.clone(), paint(cx.base())).alignment(align));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

// ---- upload --------------------------------------------------------------------------------

fn bar(progress: u8, width: usize) -> String {
    let filled = usize::from(progress.min(100)) * width / 100;
    format!("{}{} {progress:>3}%", "█".repeat(filled), "░".repeat(width - filled))
}

fn file_line(row: &FileRow, cx: &Cx) -> Line<'static> {
    let tone = match row.chip {
        transgate_core::presentation::upload_queue::Chip::UploadFailed => cx.tokens.error,
        transgate_core::presentation::upload_queue::Chip::Ready => cx.tokens.success,
        _ => cx.tokens.progress,
    };
    let mut spans = vec![
        Span::styled(clip(&row.name, NAME_WIDTH), cx.base()),
        Span::styled(format!(" {:>10} ", row.desktop_size), cx.muted()),
        Span::styled(clip(&cx.say(&row.label), 14), cx.base().fg(tone)),
        Span::styled(format!(" {}", bar(row.progress, 10)), cx.base().fg(tone)),
    ];
    if let Some(minutes) = &row.duration_text {
        spans.push(Span::styled(format!("  {}", cx.say(minutes)), cx.muted()));
    }
    Line::from(spans)
}

pub fn render_queue(frame: &mut Frame, area: Rect, vm: &UploadQueueVm, ui: &UiState, cx: &Cx) {
    let block = Block::bordered().title(cx.say(&common("Upload File"))).border_style(cx.muted()).style(cx.base());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let alert_text = alert_lines(vm, cx);
    let alert_rows: usize = alert_text.iter().map(|(text, _)| wrapped_rows(text, usize::from(inner.width))).sum();
    let alert_height = u16::try_from(alert_rows).unwrap_or(u16::MAX).min(inner.height / 2);
    let [options, files, alerts, prompt] = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(alert_height), Constraint::Length(1)]).areas(inner);

    let name = |language: Option<transgate_core::languages::Language>| language.map_or_else(String::new, |l| l.name().to_string());
    let languages = match vm.mode {
        Mode::Transcription => match vm.language {
            Some(Choice::AutoDetect) => cx.say(&vm.auto_detect_label),
            Some(Choice::Language(l)) => l.name().to_string(),
            None => String::new(),
        },
        Mode::Translation => format!("{} → {}", name(vm.source_language), name(vm.target_language)),
    };
    let speakers = format!("[{}] {}", if vm.speakers { "x" } else { " " }, cx.say(&common("Speakers")));
    let start = if vm.transcribing { common("Starting...") } else { start_label(vm.mode) };
    let start_style = if vm.start_disabled || vm.transcribing { cx.muted() } else { cx.accent().add_modifier(Modifier::BOLD) };
    let mut summary = vec![Span::styled(cx.say(&mode_label(vm.mode)), cx.accent().add_modifier(Modifier::BOLD)), Span::styled(format!("  {languages}  {speakers}  "), cx.base())];
    if let Some(total) = vm.total_minutes {
        summary.push(Span::styled(format!("{}  ", cx.say(&app("p5.duration.minutes").number("minutes", total))), cx.muted()));
    }
    summary.push(Span::styled(format!("[ {} ]", cx.say(&start)), start_style));
    frame.render_widget(Paragraph::new(vec![Line::from(summary)]), options);

    let mut file_lines: Vec<Line> = vm.rows.iter().map(|row| file_line(row, cx)).collect();
    if vm.initializing {
        file_lines.push(Line::styled(cx.say(&common("Loading...")), cx.muted()));
    }
    frame.render_widget(Paragraph::new(file_lines), files);

    let alert_body: Vec<Line> = alert_text.into_iter().map(|(text, style)| Line::styled(text, style)).collect();
    frame.render_widget(Paragraph::new(alert_body).wrap(Wrap { trim: false }), alerts);

    let path_style = if ui.focus == Focus::Path { cx.accent() } else { cx.muted() };
    let caret = if ui.focus == Focus::Path { CURSOR } else { "" };
    frame.render_widget(Paragraph::new(Line::styled(format!("{}: {}{caret}", cx.say(&common("Upload File")), ui.path), path_style)), prompt);
}

/// Rows `text` takes when word-wrapped to `width` columns (a word longer than the width splits).
fn wrapped_rows(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let (mut rows, mut used) = (1, 0);
    for word in text.split_whitespace() {
        let mut len = word.chars().count();
        if used > 0 && used + 1 + len <= width {
            used += 1 + len;
            continue;
        }
        if used > 0 {
            rows += 1;
        }
        while len > width {
            rows += 1;
            len -= width;
        }
        used = len;
    }
    rows
}

fn alert_lines(vm: &UploadQueueVm, cx: &Cx) -> Vec<(String, Style)> {
    let mut lines = Vec::new();
    for alert in &vm.alerts {
        lines.push((cx.say(&alert.title()), cx.error().add_modifier(Modifier::BOLD)));
        for paragraph in alert.paragraphs() {
            lines.push((cx.say(&paragraph), cx.error()));
        }
    }
    if let Some(error) = &vm.youtube_error {
        lines.push((cx.say(error), cx.error()));
    }
    lines
}

/// Every message a screen draws from the fixture view models is a catalog key; the
/// shell's own chrome keys are listed here so a test can translate each of them.
pub fn chrome_messages() -> Vec<Msg> {
    let mut all = hint_messages();
    all.extend([
        auth("sign_in.email_label"),
        auth("sign_in.password_label"),
        auth("sign_in.loading_button_label"),
        common("Loading..."),
        common("Speaker"),
        common("Are you sure you want to delete this file?"),
        common("Starting..."),
        app("Loading jobs..."),
        app("Loading more..."),
        app("No transcripts found"),
        app("Start by creating your first transcript"),
    ]);
    all
}

#[cfg(test)]
mod tests;
