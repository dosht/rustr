//! P3 clause reconciliation, the entries that belong to the client driver (p22):
//! UPL-013/014 (a failed PUT), EXP-015/EXP-016 (the phone ZIP download and who
//! records `transcript_downloaded`), PRV-010 (sign-out and deletion reset the
//! analytics identity). UPL-006 (measurement) and the "only HTTP 200" half of
//! UPL-013 are browser-capability rules with their own Vitest suites
//! (`platform/media/measure.test.ts`, `put.test.ts`).
mod common;

use std::time::Duration;

use common::*;
use transgate_core::JobId;
use transgate_core::presentation::app::{AppIntent, AppVm, ManageDataIntent, TranscriptIntent, UploadIntent};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::route::{HomeView, Route};
use transgate_core::presentation::upload_queue::{Chip, PickedFile};

fn picked() -> AppIntent {
    AppIntent::Upload(UploadIntent::Picked { files: vec![PickedFile { handle: 1, name: "talk.mp3".into(), size: 4_200_000, mime: "audio/mpeg".into() }] })
}

fn chips(driver: &transgate_client::driver::AppDriver) -> Vec<Chip> {
    match driver.current() {
        AppVm::SignedIn(vm) => vm.queue.rows.iter().map(|r| r.chip.clone()).collect(),
        _ => vec![],
    }
}

/// UPL-013/014: one attempt. The PUT reported progress, then failed: the draft row
/// stays, marked "Upload Failed", and nothing retries it, not on a later timer either.
#[test]
fn a_failed_put_keeps_the_draft_marked_failed_and_is_never_retried() {
    local(async {
        let calls = Calls { upload_ticks: vec![25, 60], upload_ok: false, ..Calls::default() };
        let rig = rig_with(Viewport::default(), calls).await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(picked());
        until("the upload to fail", || chips(&rig.driver) == vec![Chip::UploadFailed]).await;

        // Progress was shown while it ran: a published view carried 60 percent.
        let showed_progress = rig.views.borrow().iter().any(|view| match view {
            AppVm::SignedIn(vm) => vm.queue.rows.iter().any(|r| r.chip == Chip::Uploading { progress: 60 }),
            _ => false,
        });
        assert!(showed_progress, "the row showed the PUT's progress");

        // The draft was created once and the PUT went to the signed URL, once.
        assert_eq!(rig.calls.borrow().uploads, vec![(1, "https://signed.example/put".to_string())]);
        // No retry later: fire every timer and wait.
        fire_all(&rig.calls);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(rig.calls.borrow().uploads.len(), 1, "one attempt, no automatic retry");
        assert_eq!(chips(&rig.driver), vec![Chip::UploadFailed]);
    });
}

/// The other half of the contract: a PUT the platform reports as 200 is uploaded.
#[test]
fn a_put_the_platform_reports_as_200_marks_the_draft_uploaded() {
    local(async {
        let calls = Calls { upload_ticks: vec![50], upload_ok: true, ..Calls::default() };
        let rig = rig_with(Viewport::default(), calls).await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(picked());
        until("the upload to finish", || chips(&rig.driver) == vec![Chip::Ready]).await;
        assert_eq!(rig.calls.borrow().uploads.len(), 1);
    });
}

/// EXP-015 (phone ZIP download without Web Share): the transcript has no lines at
/// all. The download is the server's ZIP, saved through the platform; the share
/// sheet is not involved, and the client records no analytics: the server writes
/// `transcript_downloaded` (EXP-016; Y asserts its delivery to PostHog).
#[test]
fn a_phone_export_of_a_transcript_with_no_lines_downloads_the_server_zip() {
    local(async {
        let calls = Calls { share_outcome: transgate_client::driver::ShareOutcome::Unsupported, ..Calls::default() };
        let rig = rig_with(Viewport { touch: true, width: 412 }, calls).await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(AppIntent::Navigate { route: Route::Home(HomeView::Job(JobId(1))) });
        until("the transcript to open", || matches!(rig.driver.current(), AppVm::SignedIn(vm) if vm.transcript.is_some())).await;
        match rig.driver.current() {
            AppVm::SignedIn(vm) => assert!(vm.transcript.unwrap().rows.is_empty(), "the transcript has no lines"),
            _ => unreachable!(),
        }

        rig.driver.dispatch(AppIntent::Transcript(TranscriptIntent::DownloadPressed { all_formats: true }));
        until("the ZIP to be saved", || !rig.calls.borrow().downloads.is_empty()).await;

        let calls = rig.calls.borrow();
        assert_eq!(calls.downloads, vec![(4, "empty.zip".to_string(), "application/zip".to_string())]);
        assert_eq!(calls.shares, 0, "no share sheet for the ZIP");
        assert!(calls.analytics.is_empty(), "the server, not the client, records transcript_downloaded: {:?}", calls.analytics);
    });
}

/// PRV-010: signing out resets the analytics identity (`loggedOut`, then PostHog reset).
/// The identity service is unreachable here, which does not matter: core ordered the
/// reset before the sign-out request.
#[test]
fn signing_out_resets_the_analytics_identity() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(AppIntent::SignOutRequested);
        until("the reset", || rig.calls.borrow().resets == 1).await;
        assert!(rig.calls.borrow().analytics.contains(&"loggedOut".to_string()));
    });
}

/// PRV-010: after the account-deletion call the browser signs the user out the same way.
#[test]
fn account_deletion_resets_the_analytics_identity_after_the_server_answers() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        until("the settings to load", || rig.host.seen.lock().unwrap().iter().any(|(path, _)| path == "/api/settings")).await;

        rig.driver.dispatch(AppIntent::ManageData(ManageDataIntent::DeleteAccountPressed));
        assert_eq!(rig.calls.borrow().resets, 0, "asking to delete resets nothing");
        rig.driver.dispatch(AppIntent::ManageData(ManageDataIntent::Confirmed));

        until("the reset after the erasure answer", || rig.calls.borrow().resets == 1).await;
        assert!(rig.host.seen.lock().unwrap().iter().any(|(path, _)| path == "/api/delete-account"), "the server was called first");
        assert!(rig.calls.borrow().analytics.contains(&"loggedOut".to_string()));
    });
}
