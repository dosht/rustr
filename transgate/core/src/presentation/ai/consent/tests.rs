use super::*;
use crate::ids::Micros;

fn granted() -> ConsentView {
    ConsentView {
        has_consented: true,
        consent_version: "1.0".into(),
        consent_date: Some(Micros(1_700_000_000_000_000)),
        revoked_date: None,
        privacy_policy_url: "/legal/privacy-policy-en".into(),
        provider_name: "deepseek".into(),
        can_revoke: true,
        cleanup_pending: false,
    }
}

fn refused() -> ConsentView {
    ConsentView { has_consented: false, consent_date: None, revoked_date: Some(Micros(1_700_000_100_000_000)), can_revoke: false, ..granted() }
}

fn failure(status: u16) -> ApiFailure {
    ApiFailure::http(status, Some("unavailable"))
}

#[test]
fn the_gate_waits_for_the_first_read_then_follows_the_server_answer() {
    let mut s = ConsentPanelState::default();
    assert_eq!(s.update(Event::Opened), vec![Effect::LoadConsent]);
    assert_eq!(s.view().gate, Gate::Loading, "AIFeatureWrapper shows a spinner until consent is known");
    assert!(s.update(Event::Opened).is_empty(), "a second open while loading reads once");
    s.update(Event::Loaded { result: Ok(refused()) });
    assert_eq!(s.view().gate, Gate::Required);
    s.update(Event::Loaded { result: Ok(granted()) });
    assert_eq!(s.view().gate, Gate::Granted);
}

#[test]
fn a_failed_read_shows_the_introduction_not_the_content() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Opened);
    s.update(Event::Loaded { result: Err(failure(503)) });
    let vm = s.view();
    assert_eq!(vm.gate, Gate::Required, "source context: a failed read leaves hasConsented false");
    assert!(vm.load_error.is_some(), "the settings tab shows the read error");
}

#[test]
fn enabling_from_the_introduction_or_settings_records_a_grant_once() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(refused()) });
    assert_eq!(s.update(Event::EnableRequested), vec![Effect::RecordConsent { granted: true }]);
    assert_eq!(s.view().busy, Some(Decision::Enable));
    assert!(s.update(Event::EnableRequested).is_empty(), "isProcessing disables a second click");
    s.update(Event::DecisionSaved { result: Ok(granted()) });
    let vm = s.view();
    assert_eq!(vm.gate, Gate::Granted);
    assert_eq!(vm.busy, None);
    assert!(vm.action_error.is_none());
}

#[test]
fn declining_in_the_learn_more_modal_records_a_refusal() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(refused()) });
    assert_eq!(s.update(Event::DeclineRequested), vec![Effect::RecordConsent { granted: false }]);
}

/// D22: the learn-more refusal is the nondestructive `POST false`; settings
/// "turn off AI and delete saved AI data" is the revoking `DELETE`, whose
/// account-side purge is reported as pending until the server says it is done.
#[test]
fn turning_off_in_settings_is_the_revoking_delete_distinct_from_the_refusal() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(granted()) });
    assert_eq!(s.update(Event::DisableConfirmed), vec![Effect::RevokeConsent]);
    assert_eq!(s.view().busy, Some(Decision::Disable));
    assert!(s.update(Event::DeclineRequested).is_empty(), "one consent write at a time");
    s.update(Event::DecisionSaved { result: Err(failure(503)) });
    let vm = s.view();
    assert_eq!(vm.gate, Gate::Granted, "a failed DELETE never shows consent as withdrawn");
    assert!(!vm.cleanup_pending, "nor any deletion as started");
    assert_eq!(vm.action_error, Some(app("Failed to disable AI features. Please try again.")));
    s.update(Event::ErrorDismissed);
    assert!(s.view().action_error.is_none());

    s.update(Event::DisableConfirmed);
    s.update(Event::DecisionSaved { result: Ok(ConsentView { cleanup_pending: true, ..refused() }) });
    let vm = s.view();
    assert_eq!((vm.gate, vm.cleanup_pending), (Gate::Required, true), "a pending purge is never shown as done");
    s.update(Event::Loaded { result: Ok(refused()) });
    assert!(!s.view().cleanup_pending, "done only when the server reports it");
}

#[test]
fn a_failed_grant_reports_the_source_message() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(refused()) });
    s.update(Event::EnableRequested);
    s.update(Event::DecisionSaved { result: Err(failure(500)) });
    assert_eq!(s.view().action_error, Some(app("Failed to enable AI features. Please try again.")));
    assert_eq!(s.view().gate, Gate::Required);
}

/// D21: a failed consent WRITE stays in place with an inline action error and
/// the last server-confirmed state; only the initial LOAD failure is the
/// tab-level read error.
#[test]
fn a_failed_write_is_an_inline_action_error_never_a_load_error() {
    for (start, event, expected) in [
        (granted(), Event::DisableConfirmed, "Failed to disable AI features. Please try again."),
        (refused(), Event::EnableRequested, "Failed to enable AI features. Please try again."),
        (refused(), Event::DeclineRequested, "Failed to record consent decision"),
    ] {
        let mut s = ConsentPanelState::default();
        s.update(Event::Loaded { result: Ok(start.clone()) });
        s.update(event);
        s.update(Event::DecisionSaved { result: Err(ApiFailure::network()) });
        let vm = s.view();
        assert_eq!(vm.load_error, None, "the settings controls stay");
        assert_eq!(vm.action_error, Some(app(expected)));
        assert_eq!(vm.has_consented, start.has_consented, "no false success: the confirmed state is kept");
        assert_eq!(vm.busy, None, "the control is usable again");
    }

    let mut s = ConsentPanelState::default();
    s.update(Event::Opened);
    s.update(Event::Loaded { result: Err(failure(503)) });
    let vm = s.view();
    assert_eq!(vm.load_error, Some(app("Failed to load consent preferences")));
    assert_eq!(vm.action_error, None, "the read error is distinct from an action error");
}

#[test]
fn revoking_is_only_offered_when_the_server_allows_it_and_cleanup_stays_visible() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(ConsentView { can_revoke: false, ..granted() }) });
    assert!(s.update(Event::DisableConfirmed).is_empty(), "the source disables the button when !canRevoke");
    s.update(Event::Loaded { result: Ok(ConsentView { cleanup_pending: true, ..refused() }) });
    assert!(s.view().cleanup_pending, "D3: a DELETE whose purge is pending is not shown as completed cleanup");
}

#[test]
fn the_settings_details_carry_the_server_record() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(granted()) });
    let vm = s.view();
    assert_eq!(vm.consent_date, Some(Micros(1_700_000_000_000_000)));
    assert_eq!(vm.provider_name.as_deref(), Some("deepseek"));
    assert_eq!(vm.consent_version.as_deref(), Some("1.0"));
    assert_eq!(vm.privacy_policy_url, "/legal/privacy-policy-en");
    assert!(vm.can_revoke);
}

#[test]
fn signing_out_forgets_the_previous_account_consent() {
    let mut s = ConsentPanelState::default();
    s.update(Event::Loaded { result: Ok(granted()) });
    s.update(Event::SessionEnded);
    assert_eq!(s.view().gate, Gate::Loading);
    assert_eq!(s.update(Event::Opened), vec![Effect::LoadConsent]);
}
