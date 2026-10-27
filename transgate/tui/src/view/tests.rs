//! Render tests on a `TestBackend`: each screen is drawn from the core's public
//! fixture view models, the buffer must carry exactly what `translate(locale, msg)`
//! returns for the model's messages, and colours must be the tokens' colours.
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use transgate_core::presentation::fixtures;
use transgate_core::presentation::job_list::{Tone, status_tone};
use transgate_core::presentation::msg::Msg;

use super::*;
use crate::design_tokens::{DEFAULT_DARK, DEFAULT_LIGHT};

fn draw(width: u16, height: u16, paint: impl FnOnce(&mut Frame)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(paint).unwrap();
    terminal.backend().buffer().clone()
}

fn rows_of(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height).map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect()
}

fn text_of(buffer: &Buffer) -> String {
    rows_of(buffer).join("\n")
}

fn tr(locale: &str, msg: &Msg) -> String {
    transgate_i18n::translate(locale, msg).unwrap_or_else(|e| panic!("{locale} {msg:?}: {e:?}"))
}

/// The first cell of `needle` on the screen.
fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
    for (y, row) in rows_of(buffer).iter().enumerate() {
        if let Some(byte) = row.find(needle) {
            return (row[..byte].chars().count() as u16, y as u16);
        }
    }
    panic!("{needle:?} is not on the screen:\n{}", text_of(buffer));
}

fn fg_at(buffer: &Buffer, needle: &str) -> Color {
    let (x, y) = find(buffer, needle);
    buffer[(x, y)].fg
}

fn bg_at(buffer: &Buffer, needle: &str) -> Color {
    let (x, y) = find(buffer, needle);
    buffer[(x, y)].bg
}

fn jobs(vm: &JobListVm, ui: &UiState, locale: &str, tokens: &Tokens) -> Buffer {
    draw(120, 20, |frame| render_jobs(frame, frame.area(), vm, ui, &Cx { locale, tokens }))
}

fn transcript(vm: &TranscriptVm, ui: &UiState, locale: &str, tokens: &Tokens) -> Buffer {
    draw(100, 20, |frame| render_transcript(frame, frame.area(), vm, ui, &Cx { locale, tokens }))
}

fn queue(vm: &UploadQueueVm, ui: &UiState, locale: &str, tokens: &Tokens) -> Buffer {
    draw(120, 20, |frame| render_queue(frame, frame.area(), vm, ui, &Cx { locale, tokens }))
}

fn auth_screen(vm: &AuthVm, ui: &UiState, locale: &str, tokens: &Tokens) -> Buffer {
    draw(80, 20, |frame| render_auth(frame, frame.area(), vm, ui, &Cx { locale, tokens }))
}

fn ui() -> UiState {
    UiState { screen: Screen::Jobs, focus: Focus::Menu, ..UiState::default() }
}

/// The translation fixture has one language tab; a second one makes the selection visible.
fn two_tabs() -> TranscriptVm {
    let mut vm = fixtures::transcript_translation();
    let mut second = vm.tabs[0].clone();
    second.tab.language = transgate_core::languages::Language::from_name("French").expect("French");
    second.name = "French".into();
    second.flag = "🇫🇷".into();
    vm.tabs.push(second);
    vm
}

fn auth_fixture(name: &str) -> AuthVm {
    fixtures::auth_fixtures().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("auth fixture {name}")).1
}

fn app_fixture(name: &str) -> AppVm {
    fixtures::app_fixtures().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("app fixture {name}")).1
}

// ---- jobs ------------------------------------------------------------------------------------

#[test]
fn the_job_list_shows_names_translated_status_and_the_tone_colour_from_the_tokens() {
    let vm = fixtures::history_sample();
    let buffer = jobs(&vm, &ui(), "en", &DEFAULT_DARK);
    let screen = text_of(&buffer);
    for row in &vm.rows {
        assert!(screen.contains(row.display_name.as_deref().unwrap()), "{:?}", row.display_name);
        let label = tr("en", &row.status_label);
        assert!(screen.contains(&label), "{label}");
        assert_eq!(fg_at(&buffer, &label), DEFAULT_DARK.tone(row.tone), "the {label} badge takes its tone");
    }
    let tones: std::collections::BTreeSet<_> = vm.rows.iter().map(|r| format!("{:?}", DEFAULT_DARK.tone(r.tone))).collect();
    assert!(tones.len() >= 3, "the fixture covers several tones, so the colour check means something");
}

#[test]
fn the_cursor_row_is_highlighted_with_the_cursor_token_and_light_tokens_differ() {
    let vm = fixtures::history_sample();
    let mut at = ui();
    at.cursor = 2;
    let buffer = jobs(&vm, &at, "en", &DEFAULT_DARK);
    assert_eq!(bg_at(&buffer, vm.rows[2].display_name.as_deref().unwrap()), DEFAULT_DARK.cursor_bg);
    assert_ne!(bg_at(&buffer, vm.rows[0].display_name.as_deref().unwrap()), DEFAULT_DARK.cursor_bg);
    let light = jobs(&vm, &at, "en", &DEFAULT_LIGHT);
    assert_eq!(bg_at(&light, vm.rows[2].display_name.as_deref().unwrap()), DEFAULT_LIGHT.cursor_bg);
    assert_ne!(DEFAULT_LIGHT.cursor_bg, DEFAULT_DARK.cursor_bg);
}

#[test]
fn a_processing_row_animates_its_spinner_with_the_frame_counter() {
    let vm = fixtures::history_sample();
    assert!(vm.rows.iter().any(|r| r.spinner));
    let first = jobs(&vm, &UiState { tick: 0, ..ui() }, "en", &DEFAULT_DARK);
    let second = jobs(&vm, &UiState { tick: 1, ..ui() }, "en", &DEFAULT_DARK);
    assert_ne!(text_of(&first), text_of(&second));
}

#[test]
fn an_empty_history_says_so_in_the_catalog_words() {
    let buffer = jobs(&fixtures::history_empty(), &ui(), "en", &DEFAULT_DARK);
    let screen = text_of(&buffer);
    assert!(screen.contains(&tr("en", &app("No transcripts found"))));
    assert!(screen.contains(&tr("en", &app("Start by creating your first transcript"))));
}

#[test]
fn loading_and_load_more_show_the_spinner_line() {
    let loading = text_of(&jobs(&fixtures::history_loading(), &ui(), "en", &DEFAULT_DARK));
    assert!(loading.contains(&tr("en", &app("Loading jobs..."))));
    assert!(!loading.contains(&tr("en", &app("No transcripts found"))), "empty is not shown while loading");
    let more = text_of(&jobs(&fixtures::history_loading_more(), &ui(), "en", &DEFAULT_DARK));
    assert!(more.contains(&tr("en", &app("Loading more..."))));
}

#[test]
fn a_list_error_is_drawn_with_the_error_token() {
    let vm = fixtures::history_error();
    let buffer = jobs(&vm, &ui(), "en", &DEFAULT_DARK);
    let message = tr("en", vm.error.as_ref().unwrap());
    assert_eq!(fg_at(&buffer, &message), DEFAULT_DARK.error);
    assert_eq!(fg_at(&buffer, vm.error_detail.as_deref().unwrap()), DEFAULT_DARK.error);
}

#[test]
fn the_delete_confirmation_names_the_job_in_an_error_surface() {
    let vm = fixtures::history_confirm_delete();
    let buffer = jobs(&vm, &ui(), "en", &DEFAULT_DARK);
    let question = tr("en", &common("Are you sure you want to delete this file?"));
    assert!(text_of(&buffer).contains(&question));
    assert_eq!(bg_at(&buffer, &question), DEFAULT_DARK.error_bg);
    let target = vm.rows.iter().find(|r| Some(r.id) == vm.confirm_delete).unwrap();
    let name = target.display_name.as_deref().unwrap();
    let (_, question_row) = find(&buffer, &question);
    assert_eq!(rows_of(&buffer)[usize::from(question_row) + 1].matches(name).count(), 1, "the name sits under the question");
}

#[test]
fn a_missing_name_duration_and_language_use_the_models_own_words() {
    let mut vm = fixtures::history_sample();
    vm.rows[0].display_name = None;
    vm.rows[0].duration = None;
    vm.rows[0].language = None;
    let screen = text_of(&jobs(&vm, &ui(), "en", &DEFAULT_DARK));
    for msg in [&vm.untitled, &vm.not_available, &vm.auto_detect] {
        assert!(screen.contains(&tr("en", msg)), "{msg:?}");
    }
}

#[test]
fn a_long_name_is_cut_with_an_ellipsis() {
    let mut vm = fixtures::history_sample();
    vm.rows[0].display_name = Some("a-very-long-recording-name-that-does-not-fit-the-column.mp3".into());
    let screen = text_of(&jobs(&vm, &ui(), "en", &DEFAULT_DARK));
    assert!(screen.contains("…"));
    assert!(!screen.contains("does-not-fit-the-column"));
}

// ---- transcript ------------------------------------------------------------------------------

#[test]
fn a_completed_transcript_shows_its_lines_times_and_translated_speakers() {
    let mut vm = fixtures::transcript_completed();
    vm.show_time = true;
    vm.show_speakers = true;
    let screen = text_of(&transcript(&vm, &ui(), "en", &DEFAULT_DARK));
    assert!(screen.contains(&vm.title));
    for row in &vm.rows {
        assert!(screen.contains(&row.text), "{}", row.text);
        assert!(screen.contains(&row.time_text), "{}", row.time_text);
    }
    assert!(screen.contains(&format!("{} 1", tr("en", &common("Speaker")))));
    assert!(screen.contains(&tr("en", &vm.status_badge.as_ref().unwrap().label)));
    vm.show_time = false;
    vm.show_speakers = false;
    let hidden = text_of(&transcript(&vm, &ui(), "en", &DEFAULT_DARK));
    assert!(!hidden.contains(&vm.rows[0].time_text) && !hidden.contains(&format!("{} 1", tr("en", &common("Speaker")))));
    assert!(hidden.contains(&vm.rows[0].text));
}

#[test]
fn the_status_badge_takes_the_tone_of_the_status() {
    let vm = fixtures::transcript_completed();
    let badge = vm.status_badge.as_ref().unwrap();
    let buffer = transcript(&vm, &ui(), "en", &DEFAULT_DARK);
    assert_eq!(fg_at(&buffer, &tr("en", &badge.label)), DEFAULT_DARK.tone(status_tone(badge.status)));
    assert_eq!(status_tone(badge.status), Tone::Success);
}

#[test]
fn a_running_transcript_shows_the_loading_message_in_the_progress_colour() {
    let vm = fixtures::transcript_running();
    let buffer = transcript(&vm, &ui(), "en", &DEFAULT_DARK);
    if let Some(message) = &vm.loading_message {
        assert_eq!(fg_at(&buffer, &tr("en", message)), DEFAULT_DARK.progress);
    }
    if let Some(empty) = &vm.empty {
        assert!(text_of(&buffer).contains(&tr("en", empty)));
    }
    assert!(vm.loading_message.is_some() || vm.empty.is_some());
}

#[test]
fn translation_tabs_show_every_language_and_underline_the_selected_one() {
    let vm = two_tabs();
    let buffer = transcript(&vm, &ui(), "en", &DEFAULT_DARK);
    let screen = text_of(&buffer);
    for tab in &vm.tabs {
        assert!(screen.contains(&tab.name), "{}", tab.name);
    }
    let at = vm.selected.as_ref().and_then(|s| vm.tabs.iter().position(|o| &o.tab == s)).unwrap_or(0);
    let (x, y) = find(&buffer, &vm.tabs[at].name);
    assert!(buffer[(x, y)].modifier.contains(Modifier::UNDERLINED));
    assert_eq!(buffer[(x, y)].fg, DEFAULT_DARK.accent);
    let other = &vm.tabs[(at + 1) % vm.tabs.len()].name;
    let (x, y) = find(&buffer, other);
    assert!(!buffer[(x, y)].modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn a_right_to_left_transcript_aligns_to_the_right_edge() {
    let vm = fixtures::transcript_rtl();
    assert!(vm.rtl);
    let buffer = transcript(&vm, &ui(), "en", &DEFAULT_DARK);
    let (x, _) = find(&buffer, "هل يمكننا");
    assert!(x > 0, "the line does not start at the left edge");
    let ltr = transcript(&fixtures::transcript_completed(), &ui(), "en", &DEFAULT_DARK);
    let (x, _) = find(&ltr, "Welcome to the show.");
    assert_eq!(x, 0);
}

#[test]
fn scrolling_skips_the_rows_above() {
    let vm = fixtures::transcript_completed();
    let mut at = ui();
    at.scroll = 1;
    let screen = text_of(&transcript(&vm, &at, "en", &DEFAULT_DARK));
    assert!(!screen.contains(&vm.rows[0].text));
    assert!(screen.contains(&vm.rows[1].text));
}

#[test]
fn freemium_rows_fade_toward_the_background_and_full_rows_do_not() {
    let vm = fixtures::transcript_freemium();
    assert!(!vm.fades.is_empty(), "the fixture fades rows");
    let buffer = draw(100, 80, |frame| render_transcript(frame, frame.area(), &vm, &ui(), &Cx { locale: "en", tokens: &DEFAULT_DARK }));
    let (index, percent) = vm.fades[0];
    assert!(percent < 100);
    let faded_color = fg_at(&buffer, &vm.rows[index].text);
    assert_ne!(faded_color, DEFAULT_DARK.fg, "a faded row is not the plain foreground");
    let unfaded = (0..vm.rows.len()).find(|i| vm.fades.iter().all(|(f, _)| f != i)).expect("an unfaded row");
    assert_eq!(fg_at(&buffer, &vm.rows[unfaded].text), DEFAULT_DARK.fg);
}

#[test]
fn faded_blends_rgb_and_dims_other_colours() {
    let blended = faded(Style::new(), Color::Rgb(100, 100, 100), Color::Rgb(0, 0, 0), 50);
    assert_eq!(blended.fg, Some(Color::Rgb(50, 50, 50)));
    let dimmed = faded(Style::new(), Color::Red, Color::Black, 50);
    assert!(dimmed.add_modifier.contains(Modifier::DIM));
}

// ---- upload ----------------------------------------------------------------------------------

fn prompt_ui() -> UiState {
    UiState { screen: Screen::Upload, focus: Focus::Path, ..UiState::default() }
}

#[test]
fn the_queue_lists_files_with_their_translated_chip_and_a_tone_colour() {
    for (vm, expected) in [(fixtures::queue_ready(), DEFAULT_DARK.success), (fixtures::queue_upload_failed(), DEFAULT_DARK.error), (fixtures::queue_uploading(), DEFAULT_DARK.progress)] {
        let buffer = queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK);
        let row = &vm.rows[0];
        let label = tr("en", &row.label);
        assert!(text_of(&buffer).contains(&row.name));
        assert!(text_of(&buffer).contains(&label));
        assert_eq!(fg_at(&buffer, &label), expected, "{label}");
    }
}

#[test]
fn the_progress_bar_fills_with_the_percentage() {
    assert_eq!(bar(0, 10), "░░░░░░░░░░   0%");
    assert_eq!(bar(50, 10), "█████░░░░░  50%");
    assert_eq!(bar(100, 10), "██████████ 100%");
}

#[test]
fn the_start_button_label_and_the_path_prompt_are_translated() {
    let vm = fixtures::queue_ready();
    let screen = text_of(&queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK));
    assert!(screen.contains(&tr("en", &start_label(vm.mode))));
    assert!(screen.contains(&tr("en", &common("Upload File"))));
    assert!(screen.contains(&tr("en", &mode_label(vm.mode))));
    let mut typing = prompt_ui();
    typing.path = "/tmp/talk.mp3".into();
    let buffer = queue(&vm, &typing, "en", &DEFAULT_DARK);
    assert!(text_of(&buffer).contains("/tmp/talk.mp3▏"), "the prompt shows the typed path and the caret");
    let menu = text_of(&queue(&vm, &UiState { focus: Focus::Menu, path: "/tmp/talk.mp3".into(), ..prompt_ui() }, "en", &DEFAULT_DARK));
    assert!(!menu.contains("▏"), "no caret while the prompt does not have the focus");
}

#[test]
fn a_disabled_start_is_muted_and_an_enabled_one_is_accented() {
    let ready = fixtures::queue_ready();
    assert!(!ready.start_disabled);
    let enabled = queue(&ready, &prompt_ui(), "en", &DEFAULT_DARK);
    let label = format!("[ {} ]", tr("en", &start_label(ready.mode)));
    assert_eq!(fg_at(&enabled, &label), DEFAULT_DARK.accent);
    let empty = fixtures::queue_empty();
    assert!(empty.start_disabled);
    let disabled = queue(&empty, &prompt_ui(), "en", &DEFAULT_DARK);
    assert_eq!(fg_at(&disabled, &format!("[ {} ]", tr("en", &start_label(empty.mode)))), DEFAULT_DARK.muted);
}

#[test]
fn a_starting_queue_says_starting() {
    let vm = fixtures::queue_starting();
    assert!(vm.transcribing);
    let screen = text_of(&queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK));
    assert!(screen.contains(&tr("en", &common("Starting..."))));
}

#[test]
fn translation_mode_shows_the_source_and_target_languages() {
    let vm = fixtures::queue_translation();
    let screen = text_of(&queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK));
    assert!(screen.contains(&format!("{} → {}", vm.source_language.unwrap().name(), vm.target_language.unwrap().name())));
    assert!(screen.contains(&tr("en", &mode_label(Mode::Translation))));
}

#[test]
fn every_alert_title_and_paragraph_is_translated_in_the_error_colour() {
    for vm in [fixtures::queue_balance_alert(), fixtures::queue_language_alert(), fixtures::queue_processing_timeout()] {
        assert!(!vm.alerts.is_empty());
        let buffer = queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK);
        for alert in &vm.alerts {
            let title = tr("en", &alert.title());
            assert_eq!(fg_at(&buffer, &title), DEFAULT_DARK.error, "{title}");
            let first = tr("en", &alert.paragraphs()[0]);
            assert!(text_of(&buffer).replace('\n', " ").contains(first.split(' ').next().unwrap()));
        }
    }
}

#[test]
fn a_youtube_failure_reason_is_shown() {
    let vm = fixtures::queue_youtube_error();
    let screen = text_of(&queue(&vm, &prompt_ui(), "en", &DEFAULT_DARK));
    let message = tr("en", vm.youtube_error.as_ref().expect("a reason"));
    let inside_the_frame = screen.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(inside_the_frame.contains(&message), "the long reason wraps instead of being clipped:\n{screen}");
}

#[test]
fn wrapped_rows_counts_word_wrapped_lines() {
    assert_eq!(wrapped_rows("", 10), 1);
    assert_eq!(wrapped_rows("one two", 10), 1);
    assert_eq!(wrapped_rows("one two three", 7), 2);
    assert_eq!(wrapped_rows("abcdefghij", 4), 3);
    assert_eq!(wrapped_rows("ab cdefghij", 4), 3);
}

// ---- sign in ---------------------------------------------------------------------------------

fn sign_in_ui(focus: Focus) -> UiState {
    UiState { focus, ..UiState::default() }
}

#[test]
fn the_sign_in_form_shows_translated_labels_and_never_the_password() {
    let vm = auth_fixture("busy");
    assert!(vm.password.len() > 3);
    let buffer = auth_screen(&vm, &sign_in_ui(Focus::Password), "en", &DEFAULT_DARK);
    let screen = text_of(&buffer);
    assert!(screen.contains(&tr("en", &auth("sign_in.email_label"))));
    assert!(screen.contains(&tr("en", &auth("sign_in.password_label"))));
    assert!(screen.contains(&vm.email));
    assert!(!screen.contains(&vm.password), "the password is masked");
    assert!(screen.contains(&MASK.to_string().repeat(vm.password.chars().count())));
    assert!(screen.contains(&tr("en", &auth("sign_in.loading_button_label"))), "a busy form shows the loading label");
}

#[test]
fn the_focused_field_label_is_accented_and_the_other_is_muted() {
    let vm = auth_fixture("sign_in");
    let email = tr("en", &auth("sign_in.email_label"));
    let password = tr("en", &auth("sign_in.password_label"));
    let on_email = auth_screen(&vm, &sign_in_ui(Focus::Email), "en", &DEFAULT_DARK);
    assert_eq!((fg_at(&on_email, &email), fg_at(&on_email, &password)), (DEFAULT_DARK.accent, DEFAULT_DARK.muted));
    let on_password = auth_screen(&vm, &sign_in_ui(Focus::Password), "en", &DEFAULT_DARK);
    assert_eq!((fg_at(&on_password, &email), fg_at(&on_password, &password)), (DEFAULT_DARK.muted, DEFAULT_DARK.accent));
}

#[test]
fn a_sign_in_error_is_translated_and_drawn_with_the_error_token() {
    let vm = auth_fixture("error");
    let message = tr("en", vm.error.as_ref().unwrap());
    let buffer = auth_screen(&vm, &sign_in_ui(Focus::Menu), "en", &DEFAULT_DARK);
    assert_eq!(fg_at(&buffer, &message), DEFAULT_DARK.error);
}

#[test]
fn a_sign_in_notice_gets_the_notice_surface() {
    let vm = auth_fixture("notice");
    let message = tr("en", vm.notice.as_ref().unwrap());
    let buffer = auth_screen(&vm, &sign_in_ui(Focus::Menu), "en", &DEFAULT_DARK);
    assert_eq!(bg_at(&buffer, &message), DEFAULT_DARK.notice_bg);
}

#[test]
fn the_google_line_names_the_provider_and_is_translated() {
    let vm = auth_fixture("sign_in");
    let line = tr("en", &auth("sign_in.social_provider_text").text("provider", GOOGLE));
    assert!(line.contains("Google"));
    assert!(text_of(&auth_screen(&vm, &sign_in_ui(Focus::Menu), "en", &DEFAULT_DARK)).contains(&line));
}

// ---- whole screens, hints and notices --------------------------------------------------------

fn full(vm: &AppVm, ui: &UiState, locale: &str, width: u16, height: u16) -> Buffer {
    draw(width, height, |frame| render(frame, vm, ui, locale, &DEFAULT_DARK))
}

#[test]
fn the_restoring_and_erased_views_draw_the_models_message() {
    let restoring = text_of(&full(&app_fixture("restoring"), &UiState::default(), "en", 80, 10));
    assert!(restoring.contains(&tr("en", &common("Loading..."))));
    let AppVm::Erased { message, .. } = app_fixture("erased") else { panic!("erased") };
    let erased = text_of(&full(&app_fixture("erased"), &UiState::default(), "en", 80, 10));
    assert!(erased.contains(&tr("en", &message)));
}

#[test]
fn the_signed_in_header_shows_the_email_and_the_translated_balance() {
    let AppVm::SignedIn(inn) = app_fixture("desktop_new") else { panic!("signed in") };
    let buffer = full(&AppVm::SignedIn(inn.clone()), &ui(), "en", 120, 20);
    let first = rows_of(&buffer).remove(0);
    assert!(first.contains(&inn.profile.as_ref().unwrap().email));
    assert!(first.contains(&tr("en", &inn.balance)));
}

#[test]
fn a_session_error_replaces_the_header_and_is_drawn_in_the_error_colour() {
    let AppVm::SignedIn(mut inn) = app_fixture("desktop_new") else { panic!("signed in") };
    inn.session_error = Some(transgate_core::presentation::app::SESSION_UNAVAILABLE);
    let buffer = full(&AppVm::SignedIn(inn), &ui(), "en", 120, 20);
    let message = tr("en", &transgate_core::presentation::app::SESSION_UNAVAILABLE);
    assert_eq!(fg_at(&buffer, &message), DEFAULT_DARK.error);
}

#[test]
fn the_footer_carries_the_hints_with_translated_labels_where_the_catalog_has_them() {
    let vm = app_fixture("desktop_new");
    let buffer = full(&vm, &ui(), "en", 140, 20);
    let footer = rows_of(&buffer).pop().unwrap();
    for h in hints(&vm, &ui()) {
        assert!(footer.contains(h.glyph), "{}", h.glyph);
        if let Some(label) = &h.label {
            assert!(footer.contains(&tr("en", label)), "{label:?}");
        }
    }
    assert_eq!(fg_at(&buffer, "q"), DEFAULT_DARK.accent);
}

#[test]
fn hints_follow_the_screen_and_the_models_flags() {
    let AppVm::SignedIn(mut inn) = app_fixture("desktop_new") else { panic!("signed in") };
    let glyphs = |vm: &SignedInVm, ui: &UiState| hints(&AppVm::SignedIn(Box::new(vm.clone())), ui).into_iter().map(|h| h.glyph).collect::<Vec<_>>();
    inn.jobs = fixtures::history_sample();
    inn.jobs.can_load_more = false;
    assert!(!glyphs(&inn, &ui()).contains(&"L"));
    inn.jobs.can_load_more = true;
    assert!(glyphs(&inn, &ui()).contains(&"L"));
    inn.jobs = fixtures::history_confirm_delete();
    assert_eq!(glyphs(&inn, &ui()), vec!["y", "n"]);
    let mut transcript_ui = ui();
    transcript_ui.screen = Screen::Transcript;
    let mut t = fixtures::transcript_completed();
    t.download_enabled = false;
    t.show_tabs = false;
    t.time_toggle = false;
    t.speaker_toggle = false;
    inn.transcript = Some(t.clone());
    assert_eq!(glyphs(&inn, &transcript_ui), vec!["Esc", "↑↓", "q"]);
    t.download_enabled = true;
    t.show_tabs = true;
    t.time_toggle = true;
    t.speaker_toggle = true;
    inn.transcript = Some(t);
    assert_eq!(glyphs(&inn, &transcript_ui), vec!["Esc", "↑↓", "e", "E", "[ ]", "t", "s", "q"]);
    let mut upload_ui = ui();
    upload_ui.screen = Screen::Upload;
    inn.queue = fixtures::queue_empty();
    assert!(!glyphs(&inn, &upload_ui).contains(&"⏎"), "start is not offered while the model disables it");
    inn.queue = fixtures::queue_ready();
    assert!(glyphs(&inn, &upload_ui).contains(&"⏎"));
    upload_ui.focus = Focus::Path;
    assert_eq!(glyphs(&inn, &upload_ui), vec!["⏎", "Esc"]);
}

#[test]
fn a_notice_or_link_is_drawn_above_the_footer_in_the_error_colour() {
    let vm = app_fixture("desktop_new");
    let mut at = ui();
    at.notice = Some("/tmp/out/podcast.srt".into());
    let buffer = full(&vm, &at, "en", 120, 20);
    assert_eq!(fg_at(&buffer, "/tmp/out/podcast.srt"), DEFAULT_DARK.error);
    let (_, y) = find(&buffer, "/tmp/out/podcast.srt");
    assert_eq!(y, 18, "the line above the footer");
    let mut linked = ui();
    linked.link = Some("https://accounts.example/o/oauth2".into());
    assert!(text_of(&full(&vm, &linked, "en", 120, 20)).contains("https://accounts.example/o/oauth2"));
}

#[test]
fn the_whole_screen_background_is_the_background_token() {
    let buffer = full(&app_fixture("desktop_new"), &ui(), "en", 80, 12);
    assert_eq!(buffer[(79, 5)].bg, DEFAULT_DARK.bg);
    let light = draw(80, 12, |frame| render(frame, &app_fixture("desktop_new"), &ui(), "en", &DEFAULT_LIGHT));
    assert_eq!(light[(79, 5)].bg, DEFAULT_LIGHT.bg);
}

#[test]
fn a_tiny_terminal_does_not_panic_on_any_screen() {
    for (_, vm) in fixtures::app_fixtures() {
        for screen in [Screen::SignIn, Screen::Jobs, Screen::Transcript, Screen::Upload] {
            let mut at = UiState { screen, ..UiState::default() };
            at.notice = Some("n".into());
            for (w, h) in [(1, 1), (10, 3), (30, 6)] {
                let _ = full(&vm, &at, "en", w, h);
            }
        }
    }
}

// ---- copy is translated ----------------------------------------------------------------------

#[test]
fn copy_is_translated_not_english() {
    let jobs_vm = fixtures::history_confirm_delete();
    let question = common("Are you sure you want to delete this file?");
    let (english, german, turkish) = (tr("en", &question), tr("de", &question), tr("tr", &question));
    assert_ne!(english, german);
    assert_ne!(english, turkish);
    let de = text_of(&jobs(&jobs_vm, &ui(), "de", &DEFAULT_DARK));
    assert!(de.contains(&german));
    assert!(!de.contains(&english), "no English next to the German");
    let tr_screen = text_of(&jobs(&jobs_vm, &ui(), "tr", &DEFAULT_DARK));
    assert!(tr_screen.contains(&turkish));

    let signed_in = app_fixture("desktop_new");
    let footer = rows_of(&full(&signed_in, &ui(), "de", 160, 20)).pop().unwrap();
    assert!(footer.contains(&tr("de", &common("Upload File"))));
    assert_ne!(tr("de", &common("Upload File")), tr("en", &common("Upload File")));

    let form = text_of(&auth_screen(&auth_fixture("sign_in"), &sign_in_ui(Focus::Menu), "de", &DEFAULT_DARK));
    assert!(form.contains(&tr("de", &auth("sign_in.email_label"))));
    assert_ne!(tr("de", &auth("sign_in.password_label")), tr("en", &auth("sign_in.password_label")));
}

#[test]
fn an_unknown_locale_or_key_is_shown_as_a_visible_gap_not_as_english() {
    let missing = common("This key does not exist");
    assert_eq!(say("en", &missing), "⟦common.This key does not exist⟧");
    assert!(say("xx", &common("Delete")).starts_with('⟦'));
}

fn every_fixture_screen(locale: &str) -> Vec<(String, Buffer)> {
    let mut screens = Vec::new();
    let mut at = ui();
    at.path = "/tmp/a.mp3".into();
    for (name, vm) in [
        ("history_sample", fixtures::history_sample()),
        ("history_empty", fixtures::history_empty()),
        ("history_loading", fixtures::history_loading()),
        ("history_loading_more", fixtures::history_loading_more()),
        ("history_error", fixtures::history_error()),
        ("history_confirm_delete", fixtures::history_confirm_delete()),
        ("history_downloading", fixtures::history_downloading()),
    ] {
        screens.push((name.to_string(), jobs(&vm, &at, locale, &DEFAULT_DARK)));
    }
    for (name, vm) in [
        ("transcript_running", fixtures::transcript_running()),
        ("transcript_speaker_labeling", fixtures::transcript_speaker_labeling()),
        ("transcript_completed", fixtures::transcript_completed()),
        ("transcript_rtl", fixtures::transcript_rtl()),
        ("transcript_long_strings", fixtures::transcript_long_strings()),
        ("transcript_editing", fixtures::transcript_editing()),
        ("transcript_freemium", fixtures::transcript_freemium()),
        ("transcript_translation", fixtures::transcript_translation()),
        ("transcript_youtube", fixtures::transcript_youtube()),
        ("transcript_no_audio", fixtures::transcript_no_audio()),
    ] {
        screens.push((name.to_string(), transcript(&vm, &at, locale, &DEFAULT_DARK)));
    }
    for (name, vm) in [
        ("queue_empty", fixtures::queue_empty()),
        ("queue_uploading", fixtures::queue_uploading()),
        ("queue_preparing", fixtures::queue_preparing()),
        ("queue_upload_failed", fixtures::queue_upload_failed()),
        ("queue_ready", fixtures::queue_ready()),
        ("queue_translation", fixtures::queue_translation()),
        ("queue_starting", fixtures::queue_starting()),
        ("queue_language_alert", fixtures::queue_language_alert()),
        ("queue_balance_alert", fixtures::queue_balance_alert()),
        ("queue_processing_timeout", fixtures::queue_processing_timeout()),
        ("queue_max_files", fixtures::queue_max_files()),
        ("queue_youtube", fixtures::queue_youtube()),
        ("queue_youtube_error", fixtures::queue_youtube_error()),
    ] {
        screens.push((name.to_string(), queue(&vm, &at, locale, &DEFAULT_DARK)));
    }
    for (name, vm) in fixtures::auth_fixtures() {
        screens.push((format!("auth_{name}"), auth_screen(&vm, &sign_in_ui(Focus::Email), locale, &DEFAULT_DARK)));
    }
    for (name, vm) in fixtures::app_fixtures() {
        for screen in [Screen::Jobs, Screen::Transcript, Screen::Upload, Screen::SignIn] {
            let state = UiState { screen, focus: Focus::Menu, ..UiState::default() };
            screens.push((format!("app_{name}_{screen:?}"), full(&vm, &state, locale, 200, 30)));
        }
    }
    screens
}

#[test]
fn every_message_the_fixture_screens_draw_translates_in_all_sixteen_locales() {
    assert_eq!(transgate_i18n::LOCALES.len(), 16);
    let mut gaps = Vec::new();
    for locale in transgate_i18n::LOCALES {
        for msg in chrome_messages() {
            if transgate_i18n::translate(locale, &msg).is_err() {
                gaps.push(format!("{locale}: chrome {}.{}", msg.ns.file(), msg.key));
            }
        }
        for (name, buffer) in every_fixture_screen(locale) {
            if text_of(&buffer).contains('⟦') {
                gaps.push(format!("{locale}: {name}"));
            }
        }
    }
    assert!(gaps.is_empty(), "untranslated copy:\n{}", gaps.join("\n"));
}

#[test]
fn every_hint_label_of_every_screen_is_a_catalog_key_in_all_locales() {
    let mut labels: Vec<Msg> = Vec::new();
    for (_, vm) in fixtures::app_fixtures() {
        for screen in [Screen::Jobs, Screen::Transcript, Screen::Upload, Screen::SignIn] {
            for focus in [Focus::Menu, Focus::Email, Focus::Path] {
                labels.extend(hints(&vm, &UiState { screen, focus, ..UiState::default() }).into_iter().filter_map(|h| h.label));
            }
        }
    }
    // The auto-detect hint is labelled by the queue model itself.
    let mut known = hint_messages();
    for (_, vm) in fixtures::app_fixtures() {
        if let AppVm::SignedIn(inn) = vm {
            known.push(inn.queue.auto_detect_label.clone());
        }
    }
    for label in &labels {
        assert!(known.contains(label), "hint_messages() lacks {label:?}");
    }
    for locale in transgate_i18n::LOCALES {
        for label in known.iter().chain(labels.iter()) {
            tr(locale, label);
        }
    }
}
