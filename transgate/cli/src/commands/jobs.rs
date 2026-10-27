//! `jobs`, `show`, `transcript`, `export`, `delete` (WEB-061/062/065/066).
use transgate_core::JobId;
use transgate_core::presentation::app::{AppIntent, AppVm, JobsIntent, TranscriptIntent};
use transgate_core::presentation::job_list::{JobListVm, JobRow};
use transgate_core::presentation::msg::common;
use transgate_core::presentation::route::{HomeView, Route};
use transgate_core::presentation::transcript::{LineRow, NO_LINES, TranscriptVm};
use transgate_core::transcript::SpeakerLabel;

use super::{note, out, presenter};
use crate::args::{Export, Jobs, Transcript};
use crate::context::{Context, Session};
use crate::exit::{Exit, Failure, Outcome};

/// The list once its first page (or its error) has arrived.
fn first_page(ctx: &Context, session: &Session) -> Outcome<JobListVm> {
    let list = session
        .shell
        .wait(ctx.timeout, |view| match view {
            AppVm::SignedIn(vm) if !vm.jobs.initial_loading && (!vm.jobs.rows.is_empty() || vm.jobs.empty || vm.jobs.error.is_some()) => Some(vm.jobs.clone()),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())?;
    refuse_error(ctx, list)
}

fn refuse_error(ctx: &Context, list: JobListVm) -> Outcome<JobListVm> {
    match &list.error {
        Some(error) => Err(Failure::new(Exit::Network, ctx.say(error))),
        None => Ok(list),
    }
}

pub fn list(ctx: &Context, args: &Jobs) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let mut list = first_page(ctx, &session)?;
    while args.all && list.can_load_more && args.limit.is_none_or(|n| list.rows.len() < n) {
        let before = list.rows.len();
        session.shell.dispatch(AppIntent::Jobs(JobsIntent::LoadMore)).map_err(presenter)?;
        let next = session
            .shell
            .wait(ctx.timeout, |view| match view {
                AppVm::SignedIn(vm) if !vm.jobs.loading_more && (vm.jobs.rows.len() > before || !vm.jobs.can_load_more || vm.jobs.error.is_some()) => Some(vm.jobs.clone()),
                _ => None,
            })
            .ok_or_else(|| ctx.unavailable())?;
        list = refuse_error(ctx, next)?;
    }
    let rows: Vec<&JobRow> = list.rows.iter().take(args.limit.unwrap_or(usize::MAX)).collect();
    if args.json {
        out(&serde_json::to_string(&rows).map_err(|e| Failure::new(Exit::Refused, e.to_string()))?);
    } else if rows.is_empty() {
        note(&ctx.say(&list.recent_empty));
    } else {
        for row in rows {
            out(&job_line(ctx, &list, row));
        }
    }
    Ok(())
}

/// `id`, name, status, duration, language, date: tab separated, one job per line.
fn job_line(ctx: &Context, list: &JobListVm, row: &JobRow) -> String {
    let name = row.display_name.clone().unwrap_or_else(|| ctx.say(&list.untitled));
    let duration = row.duration.clone().unwrap_or_else(|| ctx.say(&list.not_available));
    let language = row.language.clone().unwrap_or_else(|| ctx.say(&list.auto_detect));
    [row.id.0.to_string(), name, ctx.say(&row.status_label), duration, language, row.date_text.clone()].join("\t")
}

/// Opens the job's route and waits for its transcript (lines included) to load.
fn open(ctx: &Context, session: &Session, id: i64) -> Outcome<TranscriptVm> {
    let job = JobId(id);
    session.shell.dispatch(AppIntent::Navigate { route: Route::Home(HomeView::Job(job)) }).map_err(presenter)?;
    let found = session
        .shell
        .wait(ctx.timeout, |view| match view {
            AppVm::SignedIn(vm) => vm.transcript.as_ref().filter(|t| t.job == Some(job) || t.load_error.is_some()).cloned(),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())?;
    if let Some(error) = &found.load_error {
        let detail = found.load_error_detail.as_deref().unwrap_or("");
        return Err(Failure::new(Exit::Network, format!("{} {detail}", ctx.say(error)).trim().to_string()));
    }
    Ok(found)
}

pub fn show(ctx: &Context, id: i64) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let t = open(ctx, &session, id)?;
    out(&t.title);
    if let Some(minutes) = t.duration_minutes {
        out(&format!("{minutes} {}", ctx.say(&t.minutes_unit)));
    }
    if let Some(badge) = &t.status_badge {
        out(&ctx.say(&badge.label));
    }
    if let Some(waiting) = &t.loading_message {
        note(&ctx.say(waiting));
    }
    Ok(())
}

pub fn transcript(ctx: &Context, args: &Transcript) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let mut t = open(ctx, &session, args.id)?;
    if let Some(waiting) = &t.loading_message {
        return Err(Failure::new(Exit::Refused, ctx.say(waiting)));
    }
    if let Some(language) = args.translation {
        let tab = t.tabs.iter().find(|option| option.tab.language == language).map(|option| option.tab.clone());
        let tab = tab.ok_or_else(|| Failure::new(Exit::Refused, ctx.say(&NO_LINES)))?;
        if t.selected.as_ref() != Some(&tab) {
            session.shell.dispatch(AppIntent::Transcript(TranscriptIntent::TabSelected { tab: tab.clone() })).map_err(presenter)?;
            t = session
                .shell
                .wait(ctx.timeout, |view| match view {
                    AppVm::SignedIn(vm) => vm.transcript.as_ref().filter(|t| t.selected.as_ref() == Some(&tab)).cloned(),
                    _ => None,
                })
                .ok_or_else(|| ctx.unavailable())?;
        }
    }
    if args.json {
        out(&serde_json::to_string(&t.rows).map_err(|e| Failure::new(Exit::Refused, e.to_string()))?);
    } else if t.rows.is_empty() {
        let message = t.empty.as_ref().map_or_else(|| ctx.say(&NO_LINES), |m| ctx.say(m));
        return Err(Failure::new(Exit::Refused, message));
    } else {
        for row in &t.rows {
            out(&line_text(ctx, &t, row));
        }
    }
    Ok(())
}

/// `[time] speaker: text`, with the time and speaker shown as the view's toggles say.
fn line_text(ctx: &Context, t: &TranscriptVm, row: &LineRow) -> String {
    let mut text = String::new();
    if t.show_time {
        text.push_str(&format!("[{}] ", row.time_text));
    }
    if t.show_speakers {
        match &row.speaker {
            Some(SpeakerLabel::Numbered { number }) => text.push_str(&format!("{} {number}: ", ctx.say(&common("Speaker")))),
            Some(SpeakerLabel::Named { name }) => text.push_str(&format!("{name}: ")),
            Some(SpeakerLabel::None) | None => {}
        }
    }
    text.push_str(&row.text);
    text
}

pub fn export(ctx: &Context, args: &Export) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let t = open(ctx, &session, args.id)?;
    if !t.download_enabled {
        return Err(Failure::new(Exit::Refused, ctx.say(t.loading_message.as_ref().or(t.empty.as_ref()).unwrap_or(&NO_LINES))));
    }
    session.shell.dispatch(AppIntent::Transcript(TranscriptIntent::DownloadPressed { all_formats: args.all_formats })).map_err(presenter)?;
    let saved = session.shell.signal().wait_for(ctx.timeout, || {
        session.ports.take_save_error().map(Err).or_else(|| session.ports.saved().first().cloned().map(Ok))
    });
    match saved {
        Some(Ok(path)) => {
            out(&path.display().to_string());
            Ok(())
        }
        Some(Err(problem)) => Err(Failure::new(Exit::Refused, problem)),
        None => Err(ctx.unavailable()),
    }
}

pub fn delete(ctx: &Context, id: i64) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    first_page(ctx, &session)?;
    let job = JobId(id);
    session.shell.dispatch(AppIntent::Jobs(JobsIntent::DeleteRequested { job })).map_err(presenter)?;
    session
        .shell
        .wait(ctx.timeout, |view| match view {
            AppVm::SignedIn(vm) => (vm.jobs.confirm_delete == Some(job)).then_some(()),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())?;
    session.shell.dispatch(AppIntent::Jobs(JobsIntent::DeleteConfirmed)).map_err(presenter)?;
    // Core removes the row at once and gives no completion or failure signal for the host
    // call (JOB-036), so the shell cannot tell a finished delete from one still in flight.
    // Reloading the list asks the host itself: the row stays gone only once the host
    // has deleted it. A call that never lands (or is refused) ends at the deadline.
    let deadline = std::time::Instant::now() + ctx.timeout;
    while std::time::Instant::now() < deadline {
        session.shell.dispatch(AppIntent::Jobs(JobsIntent::Refresh)).map_err(presenter)?;
        let reloaded = session
            .shell
            .wait(ctx.timeout, |view| match view {
                AppVm::SignedIn(vm) if !vm.jobs.initial_loading => Some(vm.jobs.clone()),
                _ => None,
            })
            .ok_or_else(|| ctx.unavailable())?;
        let reloaded = refuse_error(ctx, reloaded)?;
        if !reloaded.rows.iter().any(|r| r.id == job) {
            out(&id.to_string());
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err(ctx.unavailable())
}
