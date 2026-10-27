use std::sync::{Arc, Mutex};

use super::*;
use crate::flags::FlagRule;
use crate::ids::ErasureId;
use crate::jobs::domain::JobStatus;
use crate::jobs::ports::JobStore;
use crate::jobs::testing::{MemStore, TestClock};
use crate::presentation::fixtures;
use crate::privacy::erasure::{ErasureRecord, ErasureState};

const NOW: Micros = Micros(1_800_000_000_000_000);

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

fn other() -> AccountId {
    AccountId(Uuid::from_u128(0xbeef))
}

/// The fixture account owns job 1; `other()` owns job 2.
fn world() -> (MemStore, Account) {
    let store = MemStore::default();
    store.with(|d| {
        d.accounts.insert(fixtures::account());
        d.accounts.insert(other());
        d.jobs.insert(1, fixtures::job(1, JobStatus::Completed, "a.mp3"));
        let mut theirs = fixtures::job(2, JobStatus::Completed, "b.mp3");
        theirs.account = other();
        d.jobs.insert(2, theirs);
        d.next_job = 2;
    });
    let account = Account::new(AccountServices { store: Arc::new(store.clone()), clock: Arc::new(TestClock(Mutex::new(NOW))) });
    (store, account)
}

fn bar(store: &MemStore, account: AccountId) {
    let record = ErasureRecord {
        id: ErasureId::new(),
        account,
        requested_at: NOW,
        state: ErasureState::Requested,
        local_erased_at: None,
        identity_deleted_at: None,
        closed_at: None,
    };
    store.with(|d| {
        d.erasures.insert(record.id, record);
    });
}

#[test]
fn feedback_on_an_owned_job_is_stored_with_its_source_value() {
    let (store, account) = world();
    let saved = run(account.submit_feedback(fixtures::account(), JobId(1), FeedbackRating::Satisfied, String::new())).unwrap();
    assert_eq!((saved.job, saved.rating.value(), saved.created_at), (JobId(1), 5, NOW));
    run(account.submit_feedback(fixtures::account(), JobId(1), FeedbackRating::Outstanding, "great".into())).unwrap();
    store.with(|d| {
        let ratings: Vec<(i16, &str)> = d.feedback.iter().map(|f| (f.rating.value(), f.comment.as_str())).collect();
        assert_eq!(ratings, vec![(5, ""), (10, "great")], "the source keeps every submission");
    });
}

#[test]
fn feedback_on_another_accounts_or_a_missing_job_is_not_found_and_writes_nothing() {
    let (store, account) = world();
    for job in [JobId(2), JobId(99)] {
        let e = run(account.submit_feedback(fixtures::account(), job, FeedbackRating::Disappointed, "x".into())).unwrap_err();
        assert!(matches!(e, JobsError::NotFound), "{job:?}: {e:?}");
    }
    store.with(|d| assert!(d.feedback.is_empty()));
}

#[test]
fn an_erasing_account_can_neither_rate_nor_ask_support() {
    let (store, account) = world();
    bar(&store, fixtures::account());
    let e = run(account.submit_feedback(fixtures::account(), JobId(1), FeedbackRating::Satisfied, String::new())).unwrap_err();
    assert!(matches!(e, JobsError::ErasureInProgress), "{e:?}");
    let e = run(account.request_support(fixtures::account(), None, "help".into())).unwrap_err();
    assert!(matches!(e, JobsError::ErasureInProgress), "{e:?}");
    store.with(|d| assert!(d.feedback.is_empty() && d.support.is_empty()));
}

#[test]
fn ratings_other_than_zero_five_and_ten_do_not_exist() {
    assert_eq!(serde_json::to_value(FeedbackRating::Disappointed).unwrap(), serde_json::json!(0));
    assert_eq!(serde_json::from_value::<FeedbackRating>(serde_json::json!(10)).unwrap(), FeedbackRating::Outstanding);
    assert!(serde_json::from_value::<FeedbackRating>(serde_json::json!(3)).is_err());
    assert!(FeedbackRating::try_from(-5).is_err());
}

#[test]
fn a_support_request_is_queued_as_typed_with_the_open_job_as_context() {
    let (store, account) = world();
    let id = run(account.request_support(fixtures::account(), Some(JobId(1)), "  ".into())).unwrap();
    store.with(|d| {
        assert_eq!(d.support.len(), 1);
        let row = &d.support[0].request;
        assert_eq!((row.id, row.account, row.job, row.message.as_str(), row.created_at), (id, fixtures::account(), Some(JobId(1)), "  ", NOW));
    });
}

#[test]
fn support_mails_go_to_support_first_then_acknowledge_the_owner() {
    let request = SupportRequest { id: SupportId(Uuid::from_u128(7)), account: fixtures::account(), job: Some(JobId(42)), message: "m".into(), created_at: NOW };
    assert_eq!(SupportMail::ORDER, [SupportMail::ToSupport, SupportMail::Acknowledgement]);
    assert_eq!(SupportMail::ToSupport.recipient(Some("ada@example.test")).as_deref(), Some(SUPPORT_ADDRESS));
    assert_eq!(SupportMail::ToSupport.subject(&request, Some("ada@example.test")), "SUPPORT: ada@example.test, jobId=42");
    let no_job = SupportRequest { job: None, ..request.clone() };
    assert_eq!(SupportMail::ToSupport.subject(&no_job, None), "SUPPORT: undefined, jobId=undefined");
    assert_eq!(SupportMail::Acknowledgement.recipient(Some("ada@example.test")).as_deref(), Some("ada@example.test"));
    assert_eq!(SupportMail::Acknowledgement.recipient(Some(" ")), None, "no address, no acknowledgement");
    assert_eq!(SupportMail::Acknowledgement.subject(&request, None), SUPPORT_ACK_SUBJECT);
    assert_ne!(SupportMail::ToSupport.idempotency_key(request.id), SupportMail::Acknowledgement.idempotency_key(request.id));
    assert_eq!(acknowledgement_name(Some("Ada")), "Ada");
    assert_eq!(acknowledgement_name(None), "there");
}

#[test]
fn erasure_removes_feedback_and_undelivered_support_and_a_hard_delete_removes_its_feedback() {
    let (store, account) = world();
    run(account.submit_feedback(fixtures::account(), JobId(1), FeedbackRating::Satisfied, String::new())).unwrap();
    run(account.submit_feedback(other(), JobId(2), FeedbackRating::Satisfied, String::new())).unwrap();
    run(account.request_support(fixtures::account(), None, "help".into())).unwrap();
    run(account.request_support(other(), None, "help".into())).unwrap();

    let mut uow = run(store.begin()).unwrap();
    run(uow.hard_delete_job(JobId(2))).unwrap();
    run(uow.erase_account_rows(fixtures::account())).unwrap();
    run(uow.commit()).unwrap();

    store.with(|d| {
        assert!(d.feedback.is_empty(), "job 2 hard deleted, the fixture account erased");
        let left: Vec<AccountId> = d.support.iter().map(|s| s.request.account).collect();
        assert_eq!(left, vec![other()], "a hard delete keeps the support request; erasure removes the erased account's");
    });
}

#[test]
fn the_session_profile_uses_the_confirmed_date_the_display_rule_and_effective_capabilities() {
    let confirmed = Micros(NOW.0 - 1_000);
    let profile = VerifiedProfile { account: fixtures::account(), email: "ada.l@example.test".into(), full_name: None, email_confirmed_at: Some(confirmed) };
    let flags = FlagConfig { show_original_transcript: FlagRule { allowed_locales: vec!["tr".into()], ..FlagRule::off() }, ..FlagConfig::default() };
    let session = session_profile(&profile, &flags, Some("tr"), false);
    assert_eq!(session.display_name, "ada.l");
    assert_eq!(session.joined_on, Some(confirmed));
    assert!(session.capabilities.show_original && session.capabilities.ai && !session.capabilities.ai_chat);
    let json = serde_json::to_value(&session).unwrap();
    assert!(json.get("allowed_emails").is_none() && !json.to_string().contains("allowed"), "no allow-list leaves the host");
    assert_eq!(display_name(Some("Ada Lovelace"), "ada@example.test"), "Ada Lovelace");
    assert_eq!(display_name(Some(""), "ada@example.test"), "ada");
}
