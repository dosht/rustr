//! Pure rendering of `Shell`. No state changes, no I/O.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Clear, Paragraph, Row, Table, TableState};
use taskr_core::tasks::presentation::Align;

use crate::keymap::{Binding, INPUT_MODE_HELP, MEMBERS_KEYS, TASKS_KEYS, active, hint_bar};
use crate::shell::{Mode, Screen, Shell};

const ACCENT: Color = Color::Rgb(0x8e, 0xc0, 0x7c);
const DIM: Color = Color::Rgb(0x6b, 0x70, 0x89);
const KEY: Color = Color::Rgb(0xe5, 0xc0, 0x7b);
const ERROR: Color = Color::Rgb(0xe0, 0x6c, 0x75);

pub fn draw(f: &mut Frame, shell: &Shell) {
    let [tabs, input, body, message, hints] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_tabs(f, shell, tabs);
    draw_input(f, shell, input);
    match shell.screen() {
        Screen::Tasks => draw_table(f, shell, body),
        Screen::Members(_) => draw_members(f, shell, body),
        Screen::SignIn(_) | Screen::Starting => draw_sign_in(f, shell, body),
    }
    draw_message(f, shell, message);
    draw_hints(f, shell, hints);
    if shell.show_help {
        draw_help(f, shell);
    }
}

fn keys_for(shell: &Shell) -> &'static [Binding] {
    match shell.screen() {
        Screen::Members(_) => MEMBERS_KEYS,
        _ => TASKS_KEYS,
    }
}

fn draw_tabs(f: &mut Frame, shell: &Shell, area: Rect) {
    let mut spans = vec![Span::styled(" taskr ", Style::new().fg(Color::Black).bg(ACCENT).bold())];
    if let Some(title) = shell.title() {
        spans.push(Span::styled(format!(" {title} "), Style::new().fg(ACCENT).bold()));
    }
    if !matches!(shell.screen(), Screen::Tasks) {
        f.render_widget(Line::from(spans), area);
        return;
    }
    for (i, tab) in shell.vm.tabs.iter().enumerate() {
        let style = if tab.selected {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new().fg(DIM)
        };
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{}", i + 1), Style::new().fg(KEY)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(tab.label.clone(), style));
    }
    f.render_widget(Line::from(spans), area);
}

fn draw_input(f: &mut Frame, shell: &Shell, area: Rect) {
    let line = match &shell.mode {
        Mode::Adding(text) => prompt("add", text),
        Mode::Filtering(text) => prompt("/", text),
        Mode::AddingMember(text) => prompt("member (email [owner|editor|viewer])", text),
        Mode::SignInEmail(_) | Mode::SignInPassword { .. } => Line::default(),
        Mode::Normal if !shell.vm.filter.is_empty() => Line::from(vec![
            Span::styled(" filter: ", Style::new().fg(DIM)),
            Span::styled(shell.vm.filter.clone(), Style::new().fg(KEY)),
            Span::styled("  (esc to clear)", Style::new().fg(DIM)),
        ]),
        Mode::Normal => Line::default(),
    };
    f.render_widget(line, area);
}

fn prompt(label: &str, text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label} > "), Style::new().fg(ACCENT).bold()),
        Span::raw(text.to_string()),
        Span::styled("█", Style::new().fg(ACCENT)),
    ])
}

fn draw_table(f: &mut Frame, shell: &Shell, area: Rect) {
    let vm = &shell.vm;
    let block =
        Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(DIM));

    if let Some(msg) = &vm.empty_message {
        let p =
            Paragraph::new(Line::styled(msg.clone(), Style::new().fg(DIM)).centered()).block(block);
        f.render_widget(p, area);
        return;
    }

    let widths: Vec<Constraint> = vm
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| match c.align {
            Align::Left => Constraint::Fill(1),
            Align::Right => {
                let widest = vm.rows.iter().map(|r| r.cells[i].chars().count()).max().unwrap_or(0);
                Constraint::Length(widest.max(c.title.len()) as u16)
            }
        })
        .collect();
    let aligned = |text: String, align: Align| match align {
        Align::Left => Cell::from(Line::from(text)),
        Align::Right => Cell::from(Line::from(text).right_aligned()),
    };
    let header = Row::new(vm.columns.iter().map(|c| aligned(c.title.to_string(), c.align)))
        .style(Style::new().fg(DIM).bold());
    let rows = vm.rows.iter().map(|r| {
        let style = if r.done {
            Style::new().fg(DIM).add_modifier(Modifier::CROSSED_OUT)
        } else {
            Style::new()
        };
        Row::new(r.cells.iter().zip(&vm.columns).map(|(text, c)| aligned(text.clone(), c.align)))
            .style(style)
    });
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(2)
        .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT))
        .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(shell.cursor));
    f.render_stateful_widget(table, area, &mut state);
}

fn draw_members(f: &mut Frame, shell: &Shell, area: Rect) {
    let Screen::Members(vm) = shell.screen() else { return };
    let block =
        Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(DIM));
    let header = Row::new(["EMAIL", "ROLE"]).style(Style::new().fg(DIM).bold());
    let rows = vm.rows.iter().map(|r| {
        let email = if r.is_you { format!("{} (you)", r.email) } else { r.email.clone() };
        Row::new([email, r.role_label.clone()])
    });
    let table = Table::new(rows, [Constraint::Fill(1), Constraint::Length(8)])
        .header(header)
        .block(block)
        .column_spacing(2)
        .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT))
        .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(shell.member_cursor));
    f.render_stateful_widget(table, area, &mut state);
}

fn draw_sign_in(f: &mut Frame, shell: &Shell, area: Rect) {
    let field = |label: &str, value: String, active: bool| {
        let cursor = if active { "█" } else { "" };
        Line::from(vec![
            Span::styled(
                format!("{label:>10} > "),
                Style::new().fg(if active { ACCENT } else { DIM }),
            ),
            Span::raw(value),
            Span::styled(cursor.to_string(), Style::new().fg(ACCENT)),
        ])
    };
    let mut lines = vec![
        Line::styled(
            format!("Sign in to {}", shell.server().unwrap_or("the server")),
            Style::new().bold(),
        ),
        Line::default(),
    ];
    match &shell.mode {
        Mode::SignInEmail(email) => lines.push(field("email", email.clone(), true)),
        Mode::SignInPassword { email, password } => {
            lines.push(field("email", email.clone(), false));
            lines.push(field("password", "•".repeat(password.chars().count()), true));
        }
        _ => lines.push(Line::styled("Signing in…", Style::new().fg(DIM))),
    }
    let block =
        Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(DIM));
    let inner = centered(area, 60, lines.len() as u16 + 2);
    f.render_widget(Paragraph::new(lines).block(block), inner);
}

fn draw_message(f: &mut Frame, shell: &Shell, area: Rect) {
    let (error, notice) = match shell.screen() {
        Screen::Members(m) => (m.error.clone(), m.notice.clone()),
        Screen::SignIn(s) => (s.error.clone(), s.notice.clone()),
        Screen::Starting => (None, None),
        Screen::Tasks => (shell.vm.error.clone(), shell.vm.notice.clone()),
    };
    let error = shell.app_error().map(str::to_string).or(error);
    let line = match (&error, &notice) {
        (Some(e), _) => Line::styled(format!(" {e}"), Style::new().fg(ERROR)),
        (None, Some(n)) => Line::styled(format!(" {n}"), Style::new().fg(ACCENT)),
        (None, None) => Line::default(),
    };
    f.render_widget(line, area);
}

fn draw_hints(f: &mut Frame, shell: &Shell, area: Rect) {
    let pairs: Vec<(&str, &str)> = match (&shell.mode, shell.screen()) {
        (Mode::Normal, Screen::SignIn(_) | Screen::Starting) => vec![("q", "Quit")],
        (Mode::Normal, _) => {
            hint_bar(keys_for(shell), shell.signed_in()).map(|b| (b.label, b.description)).collect()
        }
        _ => INPUT_MODE_HELP.to_vec(),
    };
    let mut spans = Vec::new();
    for (key, desc) in pairs {
        spans.push(Span::styled(format!(" <{key}>"), Style::new().fg(KEY)));
        spans.push(Span::styled(format!(" {desc}"), Style::new().fg(DIM)));
    }
    f.render_widget(Line::from(spans), area);
}

fn draw_help(f: &mut Frame, shell: &Shell) {
    let mut lines: Vec<Line> = active(keys_for(shell), shell.signed_in())
        .map(|b| {
            Line::from(vec![
                Span::styled(format!("{:>8}  ", b.label), Style::new().fg(KEY)),
                Span::raw(b.description),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled("Typing (add / filter)", Style::new().fg(DIM)));
    for (key, desc) in INPUT_MODE_HELP {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:>8}  "), Style::new().fg(KEY)),
            Span::raw(*desc),
        ]));
    }
    let height = lines.len() as u16 + 2;
    let area = centered(f.area(), 48, height);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Keys (any key closes) ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Key;
    use crate::shell::tests::{add, remote_shell, shell, type_text};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(shell: &Shell) -> String {
        let mut terminal = Terminal::new(TestBackend::new(72, 26)).unwrap();
        terminal.draw(|f| draw(f, shell)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn empty_screen_explains_itself_and_shows_hints() {
        let screen = render(&shell());
        assert!(screen.contains("Nothing here yet. Add your first task."), "{screen}");
        assert!(screen.contains("<j/↓> Move down"), "{screen}");
        assert!(screen.contains("Open (0)"), "{screen}");
    }

    #[test]
    fn rows_render_with_shared_columns() {
        let mut s = shell();
        add(&mut s, "Buy milk");
        let screen = render(&s);
        assert!(screen.contains("TITLE"), "{screen}");
        assert!(screen.contains("Buy milk"), "{screen}");
        assert!(screen.contains("Added \"Buy milk\""), "{screen}");
    }

    #[test]
    fn sign_in_and_members_screens_render() {
        let mut s = remote_shell();
        let screen = render(&s);
        assert!(screen.contains("Sign in to http://test/api"), "{screen}");
        type_text(&mut s, "ada@example.com");
        s.handle(Key::Enter);
        s.handle(Key::Char('m'));
        let screen = render(&s);
        assert!(screen.contains("ada's workspace · Members"), "{screen}");
        assert!(screen.contains("ada@example.com (you)"), "{screen}");
        assert!(screen.contains("Owner"), "{screen}");
        assert!(screen.contains("<a> Add member"), "{screen}");
    }

    #[test]
    fn help_overlay_lists_every_binding() {
        let mut s = shell();
        s.handle(Key::Char('?'));
        let screen = render(&s);
        assert!(screen.contains("Half page down"), "{screen}");
        assert!(screen.contains("Clear input"), "{screen}");
    }
}
