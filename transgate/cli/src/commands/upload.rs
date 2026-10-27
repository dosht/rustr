//! `upload` and `youtube` (WEB-053/054, 057/058): fill the shared queue, press start.
//! Core orders the steps (draft, measure, signed PUT, options, start), applies the
//! balance and language rules, and says why a start was refused; the shell reads
//! the queue's view model and prints it.
use std::collections::BTreeSet;

use transgate_core::presentation::app::{AppIntent, AppVm, UploadIntent};
use transgate_core::presentation::upload_queue::{Chip, Choice, Mode, UploadQueueVm};

use super::{WORK_WAIT, note, out, paragraphs, presenter};
use crate::args::{Upload, Youtube};
use crate::context::{Context, Session};
use crate::exit::{Exit, Failure, Outcome};

fn queue(session: &Session, timeout: std::time::Duration, mut check: impl FnMut(&UploadQueueVm, &BTreeSet<i64>, Option<i64>) -> bool) -> Option<(UploadQueueVm, BTreeSet<i64>, Option<i64>)> {
    session.shell.wait(timeout, |view| match view {
        AppVm::SignedIn(vm) => {
            let jobs: BTreeSet<i64> = vm.jobs.rows.iter().map(|r| r.id.0).collect();
            let opened = vm.route.job().map(|j| j.0);
            check(&vm.queue, &jobs, opened).then(|| (vm.queue.clone(), jobs, opened))
        }
        _ => None,
    })
}

fn send(session: &Session, intent: UploadIntent) -> Outcome {
    session.shell.dispatch(AppIntent::Upload(intent)).map(drop).map_err(presenter)
}

/// The queue once the drafts of an earlier session have been restored, and the job ids
/// the list already holds (its first page must have loaded: what the start adds is a difference).
fn restored(ctx: &Context, session: &Session) -> Outcome<BTreeSet<i64>> {
    session
        .shell
        .wait(ctx.timeout, |view| match view {
            AppVm::SignedIn(vm) if !vm.queue.initializing && !vm.jobs.initial_loading => Some(vm.jobs.rows.iter().map(|r| r.id.0).collect()),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())
}

/// A refusal the queue shows: its alerts, else the YouTube reason.
fn refusal(ctx: &Context, q: &UploadQueueVm) -> Option<Failure> {
    if let Some(alert) = q.alerts.first() {
        let mut body = alert.paragraphs();
        body.extend(q.youtube_error.clone());
        return Some(Failure::new(Exit::Refused, paragraphs(ctx, &alert.title(), &body)));
    }
    q.youtube_error.as_ref().map(|error| Failure::new(Exit::Refused, ctx.say(error)))
}

pub fn files(ctx: &Context, args: &Upload) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let before = restored(ctx, &session)?;

    // The options go first: a draft is created with the language and speaker toggle current at that moment.
    match (args.language, args.translate_to) {
        (Some(source), Some(target)) => {
            send(&session, UploadIntent::TabChanged { mode: Mode::Translation })?;
            send(&session, UploadIntent::SourceLanguageChosen { language: Some(source) })?;
            send(&session, UploadIntent::TargetLanguageChosen { language: Some(target) })?;
        }
        (Some(language), None) => send(&session, UploadIntent::LanguageChosen { choice: Some(Choice::Language(language)) })?,
        (None, _) => send(&session, UploadIntent::LanguageChosen { choice: Some(Choice::AutoDetect) })?,
    }
    send(&session, UploadIntent::SpeakersToggled { enabled: args.speakers })?;

    let mut picked = Vec::new();
    for path in &args.files {
        picked.push(session.ports.pick(path).map_err(|e| Failure::usage(format!("{}: {e}", path.display())))?);
    }
    let count = picked.len();
    send(&session, UploadIntent::Picked { files: picked })?;

    let mut shown = Vec::new();
    let ready = queue(&session, WORK_WAIT, |q, _, _| {
        for row in &q.rows {
            let mark = (row.id, row.progress, row.label.key.to_string());
            if !shown.contains(&mark) && row.progress % 10 == 0 {
                note(&format!("{}: {} {}%", row.name, ctx.say(&row.label), row.progress));
                shown.push(mark);
            }
        }
        q.rows.len() == count && (q.files_ready || q.rows.iter().any(|r| matches!(r.chip, Chip::UploadFailed))) || !q.alerts.is_empty()
    })
    .ok_or_else(|| ctx.unavailable())?
    .0;
    if let Some(failed) = ready.rows.iter().find(|r| matches!(r.chip, Chip::UploadFailed)) {
        return Err(Failure::new(Exit::Network, format!("{}: {}", failed.name, ctx.say(&failed.label))));
    }
    if let Some(failure) = refusal(ctx, &ready) {
        return Err(failure);
    }
    start(ctx, &session, &before, count)
}

pub fn youtube(ctx: &Context, args: &Youtube) -> Outcome {
    let (session, _) = ctx.signed_in()?;
    let before = restored(ctx, &session)?;
    let choice = args.language.map_or(Choice::AutoDetect, Choice::Language);
    send(&session, UploadIntent::LanguageChosen { choice: Some(choice) })?;
    send(&session, UploadIntent::YouTubeDialogOpened)?;
    send(&session, UploadIntent::YouTubeUrlChanged { url: args.url.clone() })?;
    send(&session, UploadIntent::YouTubeSubmitted)?;
    // The dialog closes once the video is in the queue, and stays open with its error otherwise.
    let (added, _, _) = queue(&session, ctx.timeout, |q, _, _| !q.youtube_dialog.busy && (q.youtube_dialog.error.is_some() || !q.youtube_dialog.open && !q.rows.is_empty()))
        .ok_or_else(|| ctx.unavailable())?;
    if let Some(error) = &added.youtube_dialog.error {
        return Err(Failure::new(Exit::Refused, ctx.say(error)));
    }
    start(ctx, &session, &before, 1)
}

/// Presses start and waits for the job page core navigates to, or for the refusal it shows.
fn start(ctx: &Context, session: &Session, before: &BTreeSet<i64>, count: usize) -> Outcome {
    send(session, UploadIntent::StartPressed)?;
    let (done, jobs, opened) = queue(session, WORK_WAIT, |q, _, opened| opened.is_some() || !q.alerts.is_empty() || q.youtube_error.is_some())
        .ok_or_else(|| ctx.unavailable())?;
    if let Some(failure) = refusal(ctx, &done) {
        return Err(failure);
    }
    // The started jobs reach the list through core's own poll; whatever has arrived is printed.
    let (_, jobs, _) = queue(session, ctx.timeout.min(std::time::Duration::from_secs(10)), |_, jobs, _| jobs.difference(before).count() >= count).unwrap_or((done, jobs, opened));
    let mut started: Vec<i64> = jobs.difference(before).copied().collect();
    started.extend(opened.filter(|id| !started.contains(id)));
    started.sort_unstable();
    for id in started {
        out(&id.to_string());
    }
    Ok(())
}
