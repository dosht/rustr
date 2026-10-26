use serde_json::json;

use super::*;

fn payload(value: serde_json::Value) -> Option<ConfirmPayload> {
    Some(serde_json::from_value(value).unwrap())
}

fn ticks(state: &mut ConfirmPageState, n: u32) -> Vec<ConfirmEffect> {
    (0..n).flat_map(|_| state.update(ConfirmEvent::Tick)).collect()
}

// confirm.tsx, BIL-072: a changed successful confirmation reports its intents once and
// goes to the locale root after five seconds, once.
#[test]
fn a_success_reports_once_and_goes_home_after_its_countdown() {
    let (mut state, effects) = ConfirmPageState::new(payload(json!({
        "outcome": "confirmed",
        "view": {"payment_id": 7, "status": "completed", "success": true, "changed": true,
                 "analytics": ["PaymentCompleted", {"Purchase": {"value_usd": "14.90", "currency": "USD"}}],
                 "redirect_after_seconds": 5}
    })));
    assert_eq!(
        effects,
        vec![ConfirmEffect::Track(vec![
            ConfirmAnalytic::PaymentCompleted,
            ConfirmAnalytic::Purchase { value_usd: Decimal::parse("14.90").unwrap(), currency: "USD".into() },
        ])]
    );
    assert_eq!(state.view(), ConfirmVm { kind: ConfirmKind::Succeeded, countdown: Some(5), payment_id: Some(7) });
    assert!(ticks(&mut state, 4).is_empty());
    assert_eq!(state.update(ConfirmEvent::Tick), vec![ConfirmEffect::NavigateHome]);
    assert!(ticks(&mut state, 3).is_empty(), "home only once");
    assert_eq!(state.view().countdown, Some(0));
}

// A repeat confirmation changed nothing, so nothing is reported again.
#[test]
fn a_repeat_confirmation_reports_nothing() {
    let (_, effects) = ConfirmPageState::new(payload(json!({
        "outcome": "confirmed",
        "view": {"payment_id": 7, "status": "completed", "success": true, "changed": false, "analytics": [], "redirect_after_seconds": 5}
    })));
    assert!(effects.is_empty());
}

// P5 D8: a decline or an unknown checkout is definitive and counts down; provider or storage
// uncertainty is neither success nor decline and never counts down.
#[test]
fn definitive_failures_count_down_and_uncertainty_stays_unconfirmed() {
    let (declined, effects) = ConfirmPageState::new(payload(json!({
        "outcome": "confirmed",
        "view": {"payment_id": 8, "status": "failed", "success": false, "changed": true, "analytics": ["PaymentFailed"], "redirect_after_seconds": 5}
    })));
    assert_eq!(effects, vec![ConfirmEffect::Track(vec![ConfirmAnalytic::PaymentFailed])]);
    assert_eq!((declined.view().kind, declined.view().countdown), (ConfirmKind::Failed, Some(5)));

    let (pending, _) = ConfirmPageState::new(payload(json!({
        "outcome": "confirmed",
        "view": {"payment_id": 9, "status": "pending", "success": false, "changed": false, "analytics": [], "redirect_after_seconds": 5}
    })));
    assert_eq!((pending.view().kind, pending.view().countdown, pending.view().payment_id), (ConfirmKind::Unconfirmed, None, Some(9)));

    for (status, code, kind) in [
        (404, "not_found", ConfirmKind::Failed),
        (502, "checkout_failed", ConfirmKind::Failed),
        (504, "provider_uncertain", ConfirmKind::Unconfirmed),
        (503, "storage", ConfirmKind::Unconfirmed),
    ] {
        let (mut state, effects) = ConfirmPageState::new(payload(json!({"outcome": "failed", "status": status, "code": code, "message": "x"})));
        assert!(effects.is_empty());
        assert_eq!(state.view().kind, kind, "{code}");
        let home = ticks(&mut state, 10);
        assert_eq!(home.len(), usize::from(kind == ConfirmKind::Failed), "{code}: only a definitive result goes home");
    }

    let (get, _) = ConfirmPageState::new(None);
    assert_eq!((get.view().kind, get.view().countdown), (ConfirmKind::Failed, Some(CONFIRM_COUNTDOWN_SECONDS)), "a GET shows the failure page");
}
