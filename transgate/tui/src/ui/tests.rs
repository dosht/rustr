//! The key map, key by key: what each key asks for on each screen, and that a key that
//! does not belong to the screen asks for nothing. Enablement is read from the view
//! model fields the tests set, the same way the presenter would.
use transgate_core::presentation::app::SignedOutVm;
use transgate_core::presentation::fixtures;

use super::*;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ch(c: char) -> KeyEvent {
    press(KeyCode::Char(c))
}

fn app(name: &str) -> AppVm {
    fixtures::app_fixtures().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("fixture {name}")).1
}

fn signed_in() -> SignedInVm {
    match app("desktop_new") {
        AppVm::SignedIn(vm) => *vm,
        _ => panic!("signed in"),
    }
}

fn signed_out() -> SignedOutVm {
    match app("signed_out") {
        AppVm::SignedOut(vm) => vm,
        _ => panic!("signed out"),
    }
}

/// A signed-in view with the given pieces; a downloadable transcript by default.
fn view(jobs: JobListVm, transcript: Option<transgate_core::presentation::transcript::TranscriptVm>, queue: UploadQueueVm) -> AppVm {
    let mut vm = signed_in();
    vm.jobs = jobs;
    vm.transcript = transcript;
    vm.queue = queue;
    AppVm::SignedIn(Box::new(vm))
}

fn on(screen: Screen) -> UiState {
    UiState { screen, focus: Focus::Menu, ..UiState::default() }
}

fn jobs_view() -> AppVm {
    view(fixtures::history_sample(), None, fixtures::queue_ready())
}

fn transcript_view(transcript: transgate_core::presentation::transcript::TranscriptVm) -> AppVm {
    view(fixtures::history_sample(), Some(transcript), fixtures::queue_ready())
}

fn upload_view(queue: UploadQueueVm) -> AppVm {
    view(fixtures::history_sample(), None, queue)
}

fn jobs_of(vm: &AppVm) -> &JobListVm {
    match vm {
        AppVm::SignedIn(inn) => &inn.jobs,
        _ => panic!("signed in"),
    }
}

fn one(intent: impl Wrap) -> Vec<Action> {
    super::intent(intent)
}

/// Every key the map knows, plus a few it does not, on one screen.
fn all_keys() -> Vec<KeyEvent> {
    let mut keys: Vec<KeyEvent> = super::KEYS.iter().map(|(code, _)| press(*code)).collect();
    keys.extend(['x', 'z', '1', ' '].map(ch));
    keys
}

// ---- global ----------------------------------------------------------------------------------

#[test]
fn control_c_quits_from_every_screen_and_other_control_keys_do_nothing() {
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    for (vm, screen) in [(AppVm::SignedOut(signed_out()), Screen::SignIn), (jobs_view(), Screen::Jobs), (jobs_view(), Screen::Transcript), (jobs_view(), Screen::Upload), (app("restoring"), Screen::SignIn)] {
        let mut ui = on(screen);
        assert_eq!(ui.update(&vm, ctrl('c')), vec![Action::Quit]);
        assert!(ui.update(&vm, ctrl('d')).is_empty());
    }
    let mut typing = UiState::default();
    assert_eq!(typing.update(&AppVm::SignedOut(signed_out()), ctrl('c')), vec![Action::Quit], "even while typing");
}

#[test]
fn key_releases_are_ignored() {
    let mut ui = on(Screen::Jobs);
    let mut release = ch('q');
    release.kind = KeyEventKind::Release;
    assert!(ui.update(&jobs_view(), release).is_empty());
}

#[test]
fn a_keypress_clears_the_notice() {
    let mut ui = on(Screen::Jobs);
    ui.notice = Some("saved".into());
    ui.update(&jobs_view(), ch('x'));
    assert_eq!(ui.notice, None);
}

#[test]
fn restoring_checkout_and_erased_views_only_quit() {
    for name in ["restoring", "erased", "confirmation_succeeded"] {
        let vm = app(name);
        let mut ui = UiState::default();
        for key in all_keys() {
            let expected = if key.code == KeyCode::Char('q') { vec![Action::Quit] } else { vec![] };
            assert_eq!(ui.update(&vm, key), expected, "{name} {:?}", key.code);
        }
    }
}

// ---- following the view ----------------------------------------------------------------------

#[test]
fn sync_moves_between_sign_in_and_the_jobs_screen() {
    let mut ui = UiState::default();
    ui.sync(&jobs_view());
    assert_eq!((ui.screen, ui.focus), (Screen::Jobs, Focus::Menu));
    ui.password = "x".into();
    ui.screen = Screen::Transcript;
    ui.sync(&AppVm::SignedOut(signed_out()));
    assert_eq!((ui.screen, ui.focus), (Screen::SignIn, Focus::Email));
    assert_eq!(ui.email, signed_out().auth.email, "the fields follow the model when signing out");
}

#[test]
fn sync_forgets_the_password_when_signing_in() {
    let mut ui = UiState { password: "secret".into(), ..UiState::default() };
    ui.sync(&jobs_view());
    assert!(ui.password.is_empty());
}

#[test]
fn sync_opens_the_transcript_when_the_upload_becomes_a_job_route() {
    let AppVm::SignedIn(mut vm) = jobs_view() else { panic!("signed in") };
    vm.route = Route::Home(HomeView::Job(vm.jobs.rows[0].id));
    let mut ui = on(Screen::Upload);
    ui.scroll = 5;
    ui.sync(&AppVm::SignedIn(vm.clone()));
    assert_eq!((ui.screen, ui.scroll), (Screen::Transcript, 0));
    let mut jobs = on(Screen::Jobs);
    jobs.sync(&AppVm::SignedIn(vm));
    assert_eq!(jobs.screen, Screen::Jobs, "only the upload screen follows a started job");
}

#[test]
fn sync_keeps_the_cursor_and_the_scroll_inside_the_rows() {
    let vm = transcript_view(fixtures::transcript_completed());
    let mut ui = on(Screen::Transcript);
    ui.cursor = 99;
    ui.scroll = 99;
    ui.sync(&vm);
    assert_eq!(ui.cursor, jobs_of(&vm).rows.len() - 1);
    assert_eq!(ui.scroll, 2);
    let empty = view(fixtures::history_empty(), None, fixtures::queue_empty());
    ui.sync(&empty);
    assert_eq!(ui.cursor, 0);
}

// ---- sign in ---------------------------------------------------------------------------------

#[test]
fn typing_fills_the_email_then_the_password_and_sends_the_text() {
    let vm = AppVm::SignedOut(signed_out());
    let mut ui = UiState::default();
    assert_eq!(ui.update(&vm, ch('a')), one(AuthIntent::EmailChanged { text: "a".into() }));
    assert_eq!(ui.update(&vm, ch('q')), one(AuthIntent::EmailChanged { text: "aq".into() }), "q is text in a field");
    assert_eq!(ui.update(&vm, press(KeyCode::Backspace)), one(AuthIntent::EmailChanged { text: "a".into() }));
    assert!(ui.update(&vm, press(KeyCode::Enter)).is_empty(), "enter in the email moves on");
    assert_eq!(ui.focus, Focus::Password);
    assert_eq!(ui.update(&vm, ch('p')), one(AuthIntent::PasswordChanged { text: "p".into() }));
    assert_eq!(ui.update(&vm, press(KeyCode::Enter)), one(AuthIntent::Submitted));
}

#[test]
fn tab_and_escape_move_focus_on_the_sign_in_screen() {
    let vm = AppVm::SignedOut(signed_out());
    let mut ui = UiState::default();
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Password);
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Menu);
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Email);
    ui.update(&vm, press(KeyCode::Esc));
    assert_eq!(ui.focus, Focus::Menu);
    assert!(ui.update(&vm, press(KeyCode::Left)).is_empty());
}

#[test]
fn the_sign_in_menu_submits_starts_google_and_quits() {
    let vm = AppVm::SignedOut(signed_out());
    let mut ui = UiState { focus: Focus::Menu, ..UiState::default() };
    assert_eq!(ui.update(&vm, press(KeyCode::Enter)), one(AuthIntent::Submitted));
    assert_eq!(ui.update(&vm, ch('g')), vec![Action::GoogleSignIn]);
    assert_eq!(ui.update(&vm, ch('q')), vec![Action::Quit]);
    for key in [ch('j'), ch('d'), ch('u'), ch('e'), ch('r')] {
        assert!(ui.update(&vm, key).is_empty(), "{:?} is not a sign-in command", key.code);
    }
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Email);
}

#[test]
fn signed_in_screens_ignore_every_key_while_the_screen_is_sign_in() {
    let mut ui = UiState::default();
    for key in all_keys() {
        assert!(ui.update(&jobs_view(), key).is_empty());
    }
}

// ---- jobs ------------------------------------------------------------------------------------

#[test]
fn jobs_cursor_moves_with_j_k_and_arrows_and_stops_at_the_ends() {
    let vm = jobs_view();
    let last = jobs_of(&vm).rows.len() - 1;
    let mut ui = on(Screen::Jobs);
    for code in [KeyCode::Char('k'), KeyCode::Up] {
        assert!(ui.update(&vm, press(code)).is_empty());
        assert_eq!(ui.cursor, 0, "no row above the first");
    }
    for code in [KeyCode::Char('j'), KeyCode::Down] {
        assert!(ui.update(&vm, press(code)).is_empty());
    }
    assert_eq!(ui.cursor, 2.min(last));
    for _ in 0..10 {
        ui.update(&vm, ch('j'));
    }
    assert_eq!(ui.cursor, last);
    ui.update(&vm, ch('k'));
    assert_eq!(ui.cursor, last - 1);
}

#[test]
fn enter_opens_the_row_under_the_cursor_as_a_job_route() {
    let vm = jobs_view();
    let mut ui = on(Screen::Jobs);
    ui.cursor = 2;
    let id = jobs_of(&vm).rows[2].id;
    assert_eq!(ui.update(&vm, press(KeyCode::Enter)), one(AppIntent::Navigate { route: Route::Home(HomeView::Job(id)) }));
    assert_eq!((ui.screen, ui.scroll), (Screen::Transcript, 0));
}

#[test]
fn rows_commands_need_a_row() {
    let vm = view(fixtures::history_empty(), None, fixtures::queue_empty());
    let mut ui = on(Screen::Jobs);
    for key in [press(KeyCode::Enter), ch('d'), ch('e')] {
        assert!(ui.update(&vm, key).is_empty());
    }
    assert_eq!(ui.screen, Screen::Jobs);
    assert_eq!(ui.update(&vm, ch('r')), one(JobsIntent::Refresh), "refresh needs no row");
}

#[test]
fn refresh_load_more_delete_and_download_forward_their_intents() {
    let vm = jobs_view();
    let rows = &jobs_of(&vm).rows;
    let completed = rows.iter().position(|r| r.can_download).expect("a completed row");
    let running = rows.iter().position(|r| !r.can_download).expect("a row without a download");
    let mut ui = on(Screen::Jobs);
    assert_eq!(ui.update(&vm, ch('r')), one(JobsIntent::Refresh));
    ui.cursor = completed;
    assert_eq!(ui.update(&vm, ch('d')), one(JobsIntent::DeleteRequested { job: rows[completed].id }));
    assert_eq!(ui.update(&vm, ch('e')), one(JobsIntent::DownloadRequested { job: rows[completed].id }));
    ui.cursor = running;
    assert!(ui.update(&vm, ch('e')).is_empty(), "the row says it cannot be downloaded");
    assert_eq!(ui.update(&vm, ch('d')), one(JobsIntent::DeleteRequested { job: rows[running].id }));
}

#[test]
fn load_more_follows_the_model_flag() {
    let AppVm::SignedIn(mut vm) = jobs_view() else { panic!("signed in") };
    let mut ui = on(Screen::Jobs);
    vm.jobs.can_load_more = false;
    assert!(ui.update(&AppVm::SignedIn(vm.clone()), ch('l')).is_empty());
    vm.jobs.can_load_more = true;
    for key in ['l', 'L'] {
        assert_eq!(ui.update(&AppVm::SignedIn(vm.clone()), ch(key)), one(JobsIntent::LoadMore));
    }
}

#[test]
fn a_pending_delete_owns_the_keyboard() {
    let vm = view(fixtures::history_confirm_delete(), None, fixtures::queue_ready());
    let mut ui = on(Screen::Jobs);
    assert_eq!(ui.update(&vm, ch('y')), one(JobsIntent::DeleteConfirmed));
    assert_eq!(ui.update(&vm, ch('n')), one(JobsIntent::DeleteCanceled));
    assert_eq!(ui.update(&vm, press(KeyCode::Esc)), one(JobsIntent::DeleteCanceled));
    for key in [ch('j'), ch('q'), ch('u'), ch('r'), ch('d'), press(KeyCode::Enter)] {
        assert!(ui.update(&vm, key).is_empty(), "{:?} is swallowed by the confirmation", key.code);
    }
    assert_eq!(ui.cursor, 0);
    assert_eq!(ui.screen, Screen::Jobs);
}

#[test]
fn y_and_n_do_nothing_without_a_confirmation_and_u_opens_the_path_prompt() {
    let vm = jobs_view();
    let mut ui = on(Screen::Jobs);
    assert!(ui.update(&vm, ch('y')).is_empty());
    assert!(ui.update(&vm, ch('n')).is_empty());
    assert!(ui.update(&vm, ch('u')).is_empty());
    assert_eq!((ui.screen, ui.focus), (Screen::Upload, Focus::Path));
}

#[test]
fn the_jobs_screen_ignores_transcript_and_upload_keys() {
    let vm = jobs_view();
    for code in ['s', 't', '[', ']', '<', '>', 'a', 'E', 'g', 'x'] {
        let mut ui = on(Screen::Jobs);
        assert!(ui.update(&vm, ch(code)).is_empty(), "{code}");
        assert_eq!(ui.screen, Screen::Jobs);
    }
    let mut ui = on(Screen::Jobs);
    assert!(ui.update(&vm, press(KeyCode::Esc)).is_empty());
    assert_eq!(ui.update(&vm, ch('q')), vec![Action::Quit]);
}

// ---- transcript ------------------------------------------------------------------------------

fn completed() -> transgate_core::presentation::transcript::TranscriptVm {
    let mut t = fixtures::transcript_completed();
    t.download_enabled = true;
    t
}

#[test]
fn escape_leaves_the_transcript_for_the_default_route() {
    let vm = transcript_view(completed());
    let mut ui = on(Screen::Transcript);
    assert_eq!(ui.update(&vm, press(KeyCode::Esc)), one(AppIntent::Navigate { route: Route::Home(HomeView::Default) }));
    assert_eq!(ui.screen, Screen::Jobs);
}

#[test]
fn escape_and_quit_work_while_the_transcript_is_loading_and_nothing_else_does() {
    let vm = view(fixtures::history_sample(), None, fixtures::queue_ready());
    let mut ui = on(Screen::Transcript);
    for key in all_keys() {
        let expected = match key.code {
            KeyCode::Esc => one(AppIntent::Navigate { route: Route::Home(HomeView::Default) }),
            KeyCode::Char('q') => vec![Action::Quit],
            _ => vec![],
        };
        ui.screen = Screen::Transcript;
        assert_eq!(ui.update(&vm, key), expected, "{:?}", key.code);
    }
}

#[test]
fn the_transcript_scrolls_inside_its_rows() {
    let vm = transcript_view(completed());
    let mut ui = on(Screen::Transcript);
    ui.update(&vm, ch('k'));
    assert_eq!(ui.scroll, 0);
    for code in [KeyCode::Char('j'), KeyCode::Down, KeyCode::Down, KeyCode::Down] {
        ui.update(&vm, press(code));
    }
    assert_eq!(ui.scroll, 2, "three rows, the last one is the end");
    ui.update(&vm, press(KeyCode::Up));
    assert_eq!(ui.scroll, 1);
}

#[test]
fn export_keys_follow_download_enabled() {
    let mut ui = on(Screen::Transcript);
    let vm = transcript_view(completed());
    assert_eq!(ui.update(&vm, ch('e')), one(TranscriptIntent::DownloadPressed { all_formats: false }));
    assert_eq!(ui.update(&vm, ch('E')), one(TranscriptIntent::DownloadPressed { all_formats: true }));
    let mut disabled = completed();
    disabled.download_enabled = false;
    let vm = transcript_view(disabled);
    assert!(ui.update(&vm, ch('e')).is_empty());
    assert!(ui.update(&vm, ch('E')).is_empty());
}

#[test]
fn toggles_follow_their_model_flags_and_send_the_opposite_state() {
    let mut ui = on(Screen::Transcript);
    let mut t = completed();
    t.time_toggle = true;
    t.speaker_toggle = true;
    t.show_time = false;
    t.show_speakers = true;
    let vm = transcript_view(t.clone());
    assert_eq!(ui.update(&vm, ch('t')), one(TranscriptIntent::ShowTimeToggled { on: true }));
    assert_eq!(ui.update(&vm, ch('s')), one(TranscriptIntent::ShowSpeakersToggled { on: false }));
    t.time_toggle = false;
    t.speaker_toggle = false;
    let vm = transcript_view(t);
    assert!(ui.update(&vm, ch('t')).is_empty());
    assert!(ui.update(&vm, ch('s')).is_empty());
}

#[test]
fn brackets_cycle_the_translation_tabs_and_wrap() {
    let mut t = fixtures::transcript_translation();
    let mut second = t.tabs[0].clone();
    second.tab.language = transgate_core::languages::Language::from_name("French").expect("French");
    t.tabs.push(second);
    assert!(t.show_tabs && t.tabs.len() >= 2, "the fixture has tabs");
    let vm = transcript_view(t.clone());
    let at = t.selected.as_ref().and_then(|s| t.tabs.iter().position(|o| &o.tab == s)).unwrap_or(0);
    let mut ui = on(Screen::Transcript);
    let next = t.tabs[(at + 1) % t.tabs.len()].tab.clone();
    let prev = t.tabs[(at + t.tabs.len() - 1) % t.tabs.len()].tab.clone();
    assert_eq!(ui.update(&vm, ch(']')), one(TranscriptIntent::TabSelected { tab: next }));
    assert_eq!(ui.update(&vm, ch('[')), one(TranscriptIntent::TabSelected { tab: prev }));
    let mut hidden = t;
    hidden.show_tabs = false;
    assert!(ui.update(&transcript_view(hidden), ch(']')).is_empty());
}

#[test]
fn the_transcript_screen_ignores_job_list_and_upload_keys() {
    let vm = transcript_view(completed());
    for code in ['r', 'l', 'L', 'd', 'u', 'y', 'n', 'a', '<', '>', 'g'] {
        let mut ui = on(Screen::Transcript);
        assert!(ui.update(&vm, ch(code)).is_empty(), "{code}");
        assert_eq!(ui.screen, Screen::Transcript);
    }
    let mut ui = on(Screen::Transcript);
    assert!(ui.update(&vm, press(KeyCode::Enter)).is_empty());
    assert_eq!(ui.update(&vm, ch('q')), vec![Action::Quit]);
}

// ---- upload ----------------------------------------------------------------------------------

#[test]
fn the_path_prompt_collects_text_and_picks_the_trimmed_path_on_enter() {
    let vm = upload_view(fixtures::queue_empty());
    let mut ui = UiState { screen: Screen::Upload, focus: Focus::Path, ..UiState::default() };
    for c in " /tmp/a b.mp3 ".chars() {
        assert!(ui.update(&vm, ch(c)).is_empty());
    }
    ui.update(&vm, press(KeyCode::Backspace));
    ui.update(&vm, ch(' '));
    assert_eq!(ui.update(&vm, press(KeyCode::Enter)), vec![Action::PickFile(PathBuf::from("/tmp/a b.mp3"))]);
    assert!(ui.path.is_empty());
    assert_eq!(ui.focus, Focus::Path, "the prompt stays for the next file");
    assert!(ui.update(&vm, press(KeyCode::Enter)).is_empty(), "an empty path picks nothing");
}

#[test]
fn q_is_text_in_the_path_prompt_and_escape_or_tab_returns_to_the_menu() {
    let vm = upload_view(fixtures::queue_empty());
    let mut ui = UiState { screen: Screen::Upload, focus: Focus::Path, ..UiState::default() };
    assert!(ui.update(&vm, ch('q')).is_empty());
    assert_eq!(ui.path, "q");
    ui.update(&vm, press(KeyCode::Esc));
    assert_eq!(ui.focus, Focus::Menu);
    ui.focus = Focus::Path;
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Menu);
}

#[test]
fn the_upload_menu_starts_only_when_the_model_enables_it() {
    let mut ui = on(Screen::Upload);
    let ready = upload_view(fixtures::queue_ready());
    assert_eq!(ui.update(&ready, press(KeyCode::Enter)), one(UploadIntent::StartPressed));
    let AppVm::SignedIn(mut vm) = ready else { panic!("signed in") };
    vm.queue.start_disabled = true;
    assert!(ui.update(&AppVm::SignedIn(vm), press(KeyCode::Enter)).is_empty());
}

#[test]
fn speakers_and_mode_keys_send_the_opposite_value() {
    let mut ui = on(Screen::Upload);
    let vm = upload_view(fixtures::queue_ready());
    let AppVm::SignedIn(inn) = &vm else { panic!("signed in") };
    assert_eq!(ui.update(&vm, ch('s')), one(UploadIntent::SpeakersToggled { enabled: !inn.queue.speakers }));
    assert_eq!(ui.update(&vm, ch('t')), one(UploadIntent::TabChanged { mode: Mode::Translation }));
    let vm = upload_view(fixtures::queue_translation());
    assert_eq!(ui.update(&vm, ch('t')), one(UploadIntent::TabChanged { mode: Mode::Transcription }));
}

#[test]
fn brackets_cycle_the_source_language_in_translation_and_the_language_in_transcription() {
    let mut ui = on(Screen::Upload);
    let transcription = upload_view(fixtures::queue_ready());
    let AppVm::SignedIn(inn) = &transcription else { panic!("signed in") };
    let items: Vec<_> = inn.queue.available.iter().map(|o| o.language).collect();
    let current = match inn.queue.language {
        Some(Choice::Language(l)) => Some(l),
        _ => None,
    };
    let expect = |forward| one(UploadIntent::LanguageChosen { choice: cycle(&items, current, forward).map(Choice::Language) });
    assert!(!items.is_empty());
    assert_eq!(ui.update(&transcription, ch(']')), expect(true));
    assert_eq!(ui.update(&transcription, ch('[')), expect(false));

    let translation = upload_view(fixtures::queue_translation());
    let AppVm::SignedIn(inn) = &translation else { panic!("signed in") };
    let sources: Vec<_> = inn.queue.source_languages.iter().map(|o| o.language).collect();
    let here = sources.iter().position(|l| Some(*l) == inn.queue.source_language).expect("a chosen source");
    assert_eq!(ui.update(&translation, ch(']')), one(UploadIntent::SourceLanguageChosen { language: Some(sources[(here + 1) % sources.len()]) }));
    assert_eq!(ui.update(&translation, ch('[')), one(UploadIntent::SourceLanguageChosen { language: Some(sources[(here + sources.len() - 1) % sources.len()]) }));
}

#[test]
fn angle_brackets_cycle_the_target_language_only_in_translation() {
    let mut ui = on(Screen::Upload);
    let translation = upload_view(fixtures::queue_translation());
    let AppVm::SignedIn(inn) = &translation else { panic!("signed in") };
    let targets: Vec<_> = inn.queue.target_languages.iter().map(|o| o.language).collect();
    let here = targets.iter().position(|l| Some(*l) == inn.queue.target_language).expect("a chosen target");
    assert_eq!(ui.update(&translation, ch('>')), one(UploadIntent::TargetLanguageChosen { language: Some(targets[(here + 1) % targets.len()]) }));
    assert_eq!(ui.update(&translation, ch('<')), one(UploadIntent::TargetLanguageChosen { language: Some(targets[(here + targets.len() - 1) % targets.len()]) }));
    let transcription = upload_view(fixtures::queue_ready());
    assert!(ui.update(&transcription, ch('>')).is_empty());
    assert!(ui.update(&transcription, ch('<')).is_empty());
}

#[test]
fn auto_detect_follows_the_offer_flag() {
    let mut ui = on(Screen::Upload);
    let AppVm::SignedIn(mut vm) = upload_view(fixtures::queue_ready()) else { panic!("signed in") };
    vm.queue.auto_detect_offered = true;
    assert_eq!(ui.update(&AppVm::SignedIn(vm.clone()), ch('a')), one(UploadIntent::LanguageChosen { choice: Some(Choice::AutoDetect) }));
    vm.queue.auto_detect_offered = false;
    assert!(ui.update(&AppVm::SignedIn(vm), ch('a')).is_empty());
}

#[test]
fn the_upload_menu_returns_refocuses_and_quits() {
    let vm = upload_view(fixtures::queue_ready());
    let mut ui = on(Screen::Upload);
    ui.update(&vm, press(KeyCode::Tab));
    assert_eq!(ui.focus, Focus::Path);
    ui.focus = Focus::Menu;
    ui.update(&vm, ch('u'));
    assert_eq!(ui.focus, Focus::Path);
    ui.focus = Focus::Menu;
    assert_eq!(ui.update(&vm, ch('q')), vec![Action::Quit]);
    assert!(ui.update(&vm, press(KeyCode::Esc)).is_empty());
    assert_eq!(ui.screen, Screen::Jobs);
}

#[test]
fn the_upload_menu_ignores_job_list_and_transcript_keys() {
    let vm = upload_view(fixtures::queue_ready());
    for code in ['j', 'k', 'r', 'l', 'L', 'd', 'e', 'E', 'y', 'n', 'g', 'x'] {
        let mut ui = on(Screen::Upload);
        assert!(ui.update(&vm, ch(code)).is_empty(), "{code}");
        assert_eq!(ui.screen, Screen::Upload);
    }
}

// ---- helpers ---------------------------------------------------------------------------------

#[test]
fn cycle_wraps_and_starts_at_the_ends_when_nothing_is_chosen() {
    assert_eq!(cycle::<u8>(&[], None, true), None);
    assert_eq!(cycle(&[1, 2, 3], None, true), Some(1));
    assert_eq!(cycle(&[1, 2, 3], None, false), Some(3));
    assert_eq!(cycle(&[1, 2, 3], Some(3), true), Some(1));
    assert_eq!(cycle(&[1, 2, 3], Some(1), false), Some(3));
    assert_eq!(cycle(&[1, 2, 3], Some(9), true), Some(1));
}
