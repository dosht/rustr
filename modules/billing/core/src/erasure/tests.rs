use super::*;

fn target(customer: Option<&str>) -> TerminationTarget {
    TerminationTarget {
        gateway: "iyzico".into(),
        environment: "sandbox".into(),
        subscription_ref: "sub-1".into(),
        customer_ref: customer.map(str::to_string),
    }
}

fn detail(subscription: &str, customer: Option<&str>, status: Option<&str>) -> TerminationEvidence {
    TerminationEvidence::Detail {
        subscription_ref: subscription.into(),
        customer_ref: customer.map(str::to_string),
        status: status.map(str::to_string),
    }
}

#[test]
fn d10_a_documented_cancel_success_confirms_termination() {
    assert!(termination_confirmed(&target(Some("cust-1")), &TerminationEvidence::CancelSucceeded));
    assert!(termination_confirmed(&target(None), &TerminationEvidence::CancelSucceeded));
}

#[test]
fn d10_a_bound_detail_that_no_longer_renews_confirms_termination() {
    for status in ["CANCELED", "EXPIRED"] {
        assert!(termination_confirmed(&target(Some("cust-1")), &detail("sub-1", Some("cust-1"), Some(status))), "{status}");
    }
}

#[test]
fn d10_anything_else_keeps_the_erasure_step_pending() {
    let bound = target(Some("cust-1"));
    for (evidence, why) in [
        (detail("sub-1", Some("cust-1"), Some("ACTIVE")), "still renewing"),
        (detail("sub-1", Some("cust-1"), Some("PENDING")), "pending is not stopped"),
        (detail("sub-1", Some("cust-1"), Some("UNPAID")), "unpaid can still renew"),
        (detail("sub-1", Some("cust-1"), Some("canceled")), "statuses are compared exactly"),
        (detail("sub-1", Some("cust-1"), None), "no status"),
        (detail("sub-2", Some("cust-1"), Some("CANCELED")), "another subscription"),
        (detail("sub-1", Some("cust-2"), Some("CANCELED")), "another customer"),
        (detail("sub-1", None, Some("CANCELED")), "the answer names no customer"),
    ] {
        assert!(!termination_confirmed(&bound, &evidence), "{why}");
    }
    // A legacy binding without a customer cannot be confirmed by a detail answer.
    assert!(!termination_confirmed(&target(None), &detail("sub-1", None, Some("CANCELED"))));
}

fn closure() -> CheckoutClosure {
    CheckoutClosure {
        resolved_by: "ops@example.test".into(),
        checkout_cannot_complete: true,
        no_renewing_subscription: true,
        evidence_ref: "IYZ-TICKET-42".into(),
        rationale: "provider record shows the token expired and no subscription for the customer".into(),
    }
}

// D10: a closure is only a verified statement of both facts with retained evidence;
// an unpaid snapshot, a bare rationale or either attestation alone is refused.
#[test]
fn d10_a_checkout_closure_needs_both_verified_attestations_and_evidence() {
    assert_eq!(closure().refusal(), None);
    let bare = CheckoutClosure { checkout_cannot_complete: false, no_renewing_subscription: false, evidence_ref: String::new(), ..closure() };
    assert!(bare.refusal().is_some(), "never paid, nothing verified");
    for refused in [
        CheckoutClosure { checkout_cannot_complete: false, ..closure() },
        CheckoutClosure { no_renewing_subscription: false, ..closure() },
        CheckoutClosure { evidence_ref: "  ".into(), ..closure() },
        CheckoutClosure { rationale: String::new(), ..closure() },
        CheckoutClosure { resolved_by: String::new(), ..closure() },
    ] {
        assert!(refused.refusal().is_some(), "{refused:?}");
    }
}
