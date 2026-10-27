use super::*;
use crate::presentation::fixtures::invoice_context_sample;

fn loaded(is_admin: bool) -> InvoicesState {
    let (mut state, effects) = InvoicesState::new(is_admin);
    assert_eq!(effects, vec![InvoicesEffect::LoadOwn]);
    assert!(state.view().loading);
    state.update(InvoicesEvent::Loaded(Ok(invoice_context_sample())));
    state
}

fn customer_reply() -> AdminLookupReply {
    let mut context = invoice_context_sample();
    context.invoices.truncate(1);
    context.bill_to.name = Some("Grace Customer".into());
    AdminLookupReply { customer: LookedUpCustomer { email: "grace@example.test".into() }, invoice_context: context }
}

// WEB-037/038, BIL-075: the owner sees their own invoices with two-decimal amounts, and
// preview or download hands the billing facts and Bill To to the document, unchanged.
#[test]
fn the_owner_previews_and_downloads_their_own_invoice_facts() {
    let mut state = loaded(false);
    let vm = state.view();
    assert!(!vm.loading && vm.admin.is_none());
    assert_eq!(
        vm.rows.iter().map(|r| (r.invoice_id.as_str(), r.amount_text.as_str())).collect::<Vec<_>>(),
        vec![("TRAN00000042", "120.00 TRY"), ("TRAN00000007", "14.90 USD")]
    );

    let context = invoice_context_sample();
    let preview = state.update(InvoicesEvent::Preview { payment_id: 42 });
    assert_eq!(
        preview,
        vec![InvoicesEffect::Document {
            facts: Box::new(context.invoices[0].clone()),
            bill_to: context.bill_to.clone(),
            temporary_bill_to: String::new(),
            purpose: DocumentPurpose::Preview
        }]
    );
    assert_eq!(state.view().preview, Some(42));
    state.update(InvoicesEvent::ClosePreview);
    assert_eq!(state.view().preview, None);
    assert!(matches!(
        state.update(InvoicesEvent::Download { payment_id: 7 }).as_slice(),
        [InvoicesEffect::Document { purpose: DocumentPurpose::Download, .. }]
    ));
    assert!(state.update(InvoicesEvent::Preview { payment_id: 99 }).is_empty(), "an unknown invoice opens nothing");
    assert_eq!(state.view().preview, None);
}

// BIL-076: only a server-verified administrator looks customers up or types a temporary
// Bill To; for anyone else those events do nothing.
#[test]
fn administrator_events_from_an_owner_do_nothing() {
    let mut state = loaded(false);
    state.update(InvoicesEvent::LookupEmailTyped("grace@example.test".into()));
    assert!(state.update(InvoicesEvent::LookupRequested).is_empty());
    state.update(InvoicesEvent::TemporaryBillToTyped("Somewhere else".into()));
    assert!(matches!(
        state.update(InvoicesEvent::Preview { payment_id: 42 }).as_slice(),
        [InvoicesEffect::Document { temporary_bill_to, .. }] if temporary_bill_to.is_empty()
    ));
    assert!(state.view().admin.is_none());
}

// WEB-BIL-015..017: the administrator looks a customer up once per request, sees that
// customer's invoices, adds a temporary Bill To that only the document receives, and
// goes back to their own invoices.
#[test]
fn the_administrator_looks_up_a_customer_and_the_temporary_bill_to_reaches_only_the_document() {
    let mut state = loaded(true);
    assert!(state.update(InvoicesEvent::LookupRequested).is_empty(), "no lookup without an email");
    state.update(InvoicesEvent::LookupEmailTyped("  grace@example.test ".into()));
    assert_eq!(state.update(InvoicesEvent::LookupRequested), vec![InvoicesEffect::LookUp { email: "grace@example.test".into() }]);
    assert!(state.update(InvoicesEvent::LookupRequested).is_empty(), "one lookup at a time");
    assert!(state.view().admin.unwrap().looking_up);

    let reply = customer_reply();
    state.update(InvoicesEvent::LookupAnswered(Ok(reply.clone())));
    let vm = state.view();
    let admin = vm.admin.unwrap();
    assert_eq!((admin.looking_up, admin.viewing_customer.as_deref()), (false, Some("grace@example.test")));
    assert_eq!(vm.rows.len(), 1);

    state.update(InvoicesEvent::TemporaryBillToTyped("Grace Ltd\nLondon".into()));
    assert_eq!(
        state.update(InvoicesEvent::Download { payment_id: 42 }),
        vec![InvoicesEffect::Document {
            facts: Box::new(reply.invoice_context.invoices[0].clone()),
            bill_to: reply.invoice_context.bill_to.clone(),
            temporary_bill_to: "Grace Ltd\nLondon".into(),
            purpose: DocumentPurpose::Download
        }]
    );
    assert!(state.update(InvoicesEvent::Download { payment_id: 7 }).is_empty(), "only the shown customer's invoices");

    state.update(InvoicesEvent::LookupCleared);
    let vm = state.view();
    assert_eq!((vm.rows.len(), vm.admin.unwrap().viewing_customer), (2, None));
}

// A 404 lookup says no such customer and shows the administrator's own invoices; other
// failures surface without claiming the customer is missing; a stray answer is ignored.
#[test]
fn a_missing_customer_and_a_failed_lookup_stay_distinct() {
    let mut state = loaded(true);
    state.update(InvoicesEvent::LookupAnswered(Ok(customer_reply())));
    assert_eq!(state.view().admin.unwrap().viewing_customer, None, "an answer nobody asked for is ignored");

    state.update(InvoicesEvent::LookupEmailTyped("nobody@example.test".into()));
    state.update(InvoicesEvent::LookupRequested);
    state.update(InvoicesEvent::LookupAnswered(Err(PurchaseFailure::Refused { code: "not_found".into() })));
    let vm = state.view();
    assert!(vm.admin.unwrap().not_found && vm.failure.is_none());
    assert_eq!(vm.rows.len(), 2);

    state.update(InvoicesEvent::LookupRequested);
    assert!(!state.view().admin.unwrap().not_found, "a new lookup clears the old answer");
    state.update(InvoicesEvent::LookupAnswered(Err(PurchaseFailure::Unavailable)));
    let vm = state.view();
    assert_eq!((vm.admin.unwrap().not_found, vm.failure), (false, Some(PurchaseFailure::Unavailable)));

    let (mut failed, _) = InvoicesState::new(false);
    failed.update(InvoicesEvent::Loaded(Err(PurchaseFailure::Erased)));
    let vm = failed.view();
    assert_eq!((vm.loading, vm.rows.len(), vm.failure), (false, 0, Some(PurchaseFailure::Erased)));
}

// A document that cannot be built, previewed or saved never reads as a success: the
// preview it was for closes, the failure is visible beside usable rows, and the next
// document request clears it. A failed download leaves another open preview alone.
#[test]
fn a_failed_document_closes_its_preview_and_shows_the_failure_until_the_next_request() {
    let mut state = loaded(false);
    state.update(InvoicesEvent::Preview { payment_id: 42 });
    assert!(state.update(InvoicesEvent::DocumentFailed { payment_id: 42, failure: PurchaseFailure::Unavailable }).is_empty());
    let vm = state.view();
    assert_eq!((vm.preview, vm.document_failure, vm.failure, vm.rows.len()), (None, Some(PurchaseFailure::Unavailable), None, 2));

    state.update(InvoicesEvent::Preview { payment_id: 7 });
    assert_eq!((state.view().preview, state.view().document_failure), (Some(7), None), "a new request clears the old failure");
    state.update(InvoicesEvent::DocumentFailed { payment_id: 42, failure: PurchaseFailure::Unavailable });
    assert_eq!((state.view().preview, state.view().document_failure), (Some(7), Some(PurchaseFailure::Unavailable)));
}
