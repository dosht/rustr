//! The shared effect driver on a native, non-browser `Platform` (P8-IMPL-1, D1):
//! the real `AppDriver`, the real `HttpApi` and `GoTrue` against a loopback host,
//! and a recording platform whose timers the test fires by hand. These pin what
//! the web shell relies on without a browser: epoch and token fences, the keyed
//! timer chain, dispose, and the refused settings write.
mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use transgate_client::auth::{AuthStorage, SESSION_KEY};
use transgate_client::driver::AppDriver;
use transgate_core::presentation::app::{AppIntent, AppVm, ManageDataIntent};

#[test]
fn a_stored_session_restores_and_the_owned_list_loads_through_the_driver() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let seen = rig.host.seen.lock().unwrap().clone();
        assert!(seen.iter().any(|(path, bearer)| path == "/api/session" && bearer == "token-ada"), "{seen:?}");
    });
}

/// The token fence: once another process signs in a different account, nothing is
/// sent with the first account's token, not even by effects that were already queued.
#[test]
fn after_an_account_switch_no_request_carries_the_old_accounts_token() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let before = rig.driver.epoch();

        rig.storage.set(SESSION_KEY, &session(BOB, "token-bob"));
        rig.driver.session_changed_elsewhere();
        assert_ne!(rig.driver.epoch().session, before.session, "core began a new session");
        let mark = rig.host.seen.lock().unwrap().len();

        until("requests of the new account", || rig.host.seen.lock().unwrap().len() > mark + 1).await;
        let after: Vec<_> = rig.host.seen.lock().unwrap()[mark..].to_vec();
        assert!(after.iter().all(|(_, bearer)| bearer == "token-bob"), "only the new account's token is used: {after:?}");
    });
}

/// D11 through the driver: the keyed `jobs.poll` timer asks the platform to sleep,
/// firing it polls the host, and a job created elsewhere shows without a reload.
#[test]
fn the_list_poll_chain_runs_on_platform_timers() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;

        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: true });
        // The page starts visible, so this arms the chain: the platform is asked for a timer.
        until("the platform to be asked for a timer", || !rig.calls.borrow().sleeps.is_empty()).await;
        settle(&rig.calls).await;
        rig.host.jobs.lock().unwrap().insert(0, 2);
        assert!(fire_all(&rig.calls) > 0);
        until("the job found by the first timer-driven poll", || rows(&rig.driver) == Some(vec![2, 1])).await;

        // The chain re-armed itself: another tick finds the next job.
        until("the platform to be asked for the next timer", || !rig.calls.borrow().sleeps.is_empty()).await;
        settle(&rig.calls).await;
        rig.host.jobs.lock().unwrap().insert(0, 3);
        assert!(fire_all(&rig.calls) > 0);
        until("the job found by the second poll", || rows(&rig.driver) == Some(vec![3, 2, 1])).await;
    });
}

/// Dispose aborts every keyed timer (their futures are dropped), silences the
/// listener, and a late timer fires into nothing.
#[test]
fn dispose_drops_timers_and_a_late_fire_changes_nothing() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: true });
        until("the platform to be asked for a timer", || !rig.calls.borrow().sleeps.is_empty()).await;

        let notified = Rc::new(Cell::new(0));
        let counter = notified.clone();
        rig.driver.set_listener(Some(Rc::new(move |_| counter.set(counter.get() + 1))));
        assert!(rig.driver.dispose());
        assert!(!rig.driver.dispose(), "dispose is once");

        let pending: Vec<_> = std::mem::take(&mut rig.calls.borrow_mut().sleeps);
        assert!(!pending.is_empty());
        let mark = rig.host.seen.lock().unwrap().len();
        rig.host.jobs.lock().unwrap().insert(0, 9);
        for (_, fire) in pending {
            let _ = fire.send(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(rig.host.seen.lock().unwrap().len(), mark, "no request after dispose");
        assert_eq!(notified.get(), 0, "no view after dispose");
        assert_eq!(rows(&rig.driver), Some(vec![1]), "state is frozen at dispose");
        // An intent after dispose returns the current view and runs nothing.
        rig.driver.dispatch(AppIntent::VisibilityChanged { visible: false });
        assert_eq!(rig.host.seen.lock().unwrap().len(), mark);
    });
}

/// A refused settings write returns the switch to its stored value with the save
/// error shown (core's `SaveFailed`), and the driver does not reread the settings.
#[test]
fn a_refused_settings_write_answers_save_failed_without_a_reread() {
    local(async {
        let rig = rig().await;
        until("the signed-in list", || rows(&rig.driver) == Some(vec![1])).await;
        let reads = || rig.host.seen.lock().unwrap().iter().filter(|(path, _)| path == "/api/settings").count();
        until("the settings to load", || reads() >= 1).await;
        let before = reads();

        rig.driver.dispatch(AppIntent::ManageData(ManageDataIntent::DeleteMonthlyToggled { on: false }));
        let settled = |driver: &AppDriver| match driver.current() {
            AppVm::SignedIn(vm) => (vm.manage_data.delete_monthly_on, vm.manage_data.save_error.is_some()),
            _ => (false, false),
        };
        until("the refused write to be answered", || settled(&rig.driver) == (true, true)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        // The PATCH hit the same path once; no GET followed the refusal.
        assert_eq!(reads(), before + 1, "one PATCH and no reread");
    });
}
