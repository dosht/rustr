//! Real-PG support outbox and trusted directory; external mail outcomes are controlled.

use std::collections::HashSet;
use transgate_core::account::{Account, AccountServices, SupportId};
use transgate_core::jobs::events::SUPPORT_ADDRESS;
use transgate_core::jobs::ports::JobQueries;
use billing_core::ports::{Customer, CustomerDirectory};

use super::*;

#[derive(Clone)]
struct Delivery { to: String, key: String, subject: String }
#[derive(Default)]
struct MailState {
    fail_to: Option<String>,
    attempts: Vec<Delivery>,
    sent: Vec<Delivery>,
    delivered: HashSet<String>,
    gate: Option<(Arc<Notify>, Arc<Notify>)>,
}
#[derive(Default)]
struct Mailbox(Mutex<MailState>);
impl Mailer for Mailbox {
    fn send<'a>(&'a self, message: &'a MailMessage, key: &'a str) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            let delivery = Delivery { to: message.to.clone(), key: key.into(), subject: message.subject.clone() };
            let (fail, gate) = {
                let mut state = self.0.lock().unwrap();
                state.attempts.push(delivery.clone());
                (state.fail_to.as_deref() == Some(message.to.as_str()), state.gate.take())
            };
            if fail { return Err(MailError("controlled mail failure".into())); }
            if let Some((entered, release)) = gate {
                entered.notify_one();
                release.notified().await;
            }
            let mut state = self.0.lock().unwrap();
            if state.delivered.insert(key.into()) { state.sent.push(delivery); }
            Ok(())
        })
    }
}

struct Fixture {
    workers: Arc<Workers>,
    clock: Arc<Injected>,
    account: AccountId,
    request: SupportId,
    mail: Arc<Mailbox>,
}

async fn fixture(pool: &PgPool) -> Fixture {
    let clock = Arc::new(Injected(AtomicI64::new(1_950_000_000 * SECOND)));
    let account = AccountId(Uuid::new_v4());
    let store = PgJobStore::new(pool.clone());
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, clock.now()).await.unwrap();
    uow.commit().await.unwrap();
    let directory = PgCustomerDirectory::new(pool.clone());
    directory.register(&Customer { account, email: "owner@example.test".into(), full_name: Some("Directory Ada".into()), created_at: clock.now() }).await.unwrap();
    let request = Account::new(AccountServices { store: Arc::new(store), clock: clock.clone() })
        .request_support(account, Some(transgate_core::JobId(7)), "Help with this job".into()).await.unwrap();
    let mail = Arc::new(Mailbox::default());
    let provider = Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new())));
    let mut workers = replica(pool, clock.clone(), provider);
    Arc::get_mut(&mut workers).unwrap().mailer = mail.clone();
    Fixture { workers, clock, account, request, mail }
}

async fn advance_to_retry(f: &Fixture) {
    let at: i64 = sqlx::query_scalar("SELECT transgate_jobs.ts_to_us(claimed_until) FROM transgate_jobs.support_requests WHERE id = $1")
        .bind(f.request.0).fetch_one(f.workers.store.pool()).await.unwrap();
    f.clock.0.store(at, Ordering::SeqCst);
}
async fn remaining(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.support_requests WHERE id = $1")
        .bind(f.request.0).fetch_one(f.workers.store.pool()).await.unwrap()
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn support_failure_preserves_order_and_only_both_successful_mails_ack_the_durable_request() {
    let (pool, _database) = isolated().await;
    let f = fixture(&pool).await;
    f.mail.0.lock().unwrap().fail_to = Some(SUPPORT_ADDRESS.into());
    assert_eq!(f.workers.deliver_support().await.unwrap(), 1);
    assert_eq!(remaining(&f).await, 1);
    assert_eq!(f.mail.0.lock().unwrap().attempts.iter().map(|d| d.to.as_str()).collect::<Vec<_>>(), vec![SUPPORT_ADDRESS]);
    assert_eq!(f.workers.deliver_support().await.unwrap(), 0, "a failed claim is not retried before its persisted retry time");

    advance_to_retry(&f).await;
    f.mail.0.lock().unwrap().fail_to = Some("owner@example.test".into());
    assert_eq!(f.workers.deliver_support().await.unwrap(), 1);
    assert_eq!(remaining(&f).await, 1, "support delivery alone is not acknowledgement completion");
    assert_eq!(f.mail.0.lock().unwrap().sent.iter().map(|d| d.to.as_str()).collect::<Vec<_>>(), vec![SUPPORT_ADDRESS]);

    advance_to_retry(&f).await;
    f.mail.0.lock().unwrap().fail_to = None;
    assert_eq!(f.workers.deliver_support().await.unwrap(), 1);
    assert_eq!(remaining(&f).await, 0);
    let state = f.mail.0.lock().unwrap();
    assert_eq!(state.attempts.iter().map(|d| d.to.as_str()).collect::<Vec<_>>(), vec![SUPPORT_ADDRESS, SUPPORT_ADDRESS, "owner@example.test", SUPPORT_ADDRESS, "owner@example.test"]);
    assert_eq!(state.sent.iter().map(|d| d.to.as_str()).collect::<Vec<_>>(), vec![SUPPORT_ADDRESS, "owner@example.test"]);
    assert_eq!(state.attempts[0].key, state.attempts[1].key);
    assert_eq!(state.attempts[1].key, state.attempts[3].key, "partial delivery retries use the same logical support-mail key");
    assert_eq!(state.attempts[2].key, state.attempts[4].key);
    assert_ne!(state.attempts[1].key, state.attempts[2].key, "support and acknowledgement are distinct logical sends");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn erasure_during_the_first_support_mail_prevents_the_owner_ack_and_never_restores_the_outbox() {
    let (pool, _database) = isolated().await;
    let f = fixture(&pool).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    f.mail.0.lock().unwrap().gate = Some((entered.clone(), release.clone()));
    let workers = f.workers.clone();
    let running = tokio::spawn(async move { workers.deliver_support().await });
    tokio::time::timeout(Duration::from_secs(5), entered.notified()).await.expect("first mail entered provider I/O");
    f.workers.privacy.request_erasure(f.account).await.unwrap();
    assert!(f.workers.store.barrier(f.account).await.unwrap());
    assert_eq!(remaining(&f).await, 0, "the committed cascade removes the queued request");
    release.notify_one();
    assert_eq!(running.await.unwrap().unwrap(), 1);
    let recipients: Vec<String> = f.mail.0.lock().unwrap().attempts.iter().map(|d| d.to.clone()).collect();
    assert_eq!(recipients, vec![SUPPORT_ADDRESS], "no new dispatch after the barrier is observed");
    assert_eq!(remaining(&f).await, 0, "a late worker ack cannot recreate erased content");
}

async fn emit(f: &Fixture, event: transgate_core::jobs::events::JobEvent) {
    let mut uow = f.workers.store.begin().await.unwrap();
    uow.emit(&event).await.unwrap();
    uow.commit().await.unwrap();
}

async fn unpublished(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.events WHERE account_id = $1 AND published_at IS NULL")
        .bind(f.account.0).fetch_one(f.workers.store.pool()).await.unwrap()
}

// EML-004: a failed job mails the user, then support. A mail that fails keeps the event unpublished; a
// later try, even from another process, sends again under the same per-target keys, so the user's
// mail (already sent) is not a second email, and only then is the event acknowledged.
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn eml_004_a_failed_job_mails_the_user_then_support_and_a_retry_never_repeats_a_sent_mail() {
    use transgate_core::jobs::events::{FAILED_FILE_SUBJECT, JOB_FAILED_SUBJECT, JobEvent};
    let (pool, _database) = isolated().await;
    let f = fixture(&pool).await;
    let now = f.clock.now();
    emit(&f, JobEvent::Failed {
        job: transgate_core::JobId(7), account: f.account, language: transgate_core::languages::Language::from_name("English"),
        error: Some(serde_json::json!([{"stage": "transcription", "category": "fatal", "message": "No transcription segments found"}])), at: now,
    }).await;

    // Support is down: the user is mailed first and the event stays unpublished.
    f.mail.0.lock().unwrap().fail_to = Some(SUPPORT_ADDRESS.into());
    assert_eq!(f.workers.deliver_events().await.unwrap(), 1);
    assert_eq!(unpublished(&f).await, 1, "a partial failure does not acknowledge the event");
    {
        let state = f.mail.0.lock().unwrap();
        assert_eq!(state.attempts.iter().map(|d| (d.to.as_str(), d.subject.as_str())).collect::<Vec<_>>(),
            vec![("owner@example.test", FAILED_FILE_SUBJECT), (SUPPORT_ADDRESS, JOB_FAILED_SUBJECT)], "the user first, then support");
        assert_eq!(state.sent.len(), 1);
    }

    // The retry runs in a second worker process with its own state; it shares only the database.
    let second = replica(&pool, f.clock.clone(), Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new()))));
    let mut second = second;
    Arc::get_mut(&mut second).unwrap().mailer = f.mail.clone();
    assert_eq!(second.deliver_events().await.unwrap(), 0, "a released event is not retried before its persisted retry time");
    f.clock.0.store(now.0 + 61 * SECOND, Ordering::SeqCst);
    f.mail.0.lock().unwrap().fail_to = None;
    assert_eq!(second.deliver_events().await.unwrap(), 1);
    assert_eq!(unpublished(&f).await, 0);
    let state = f.mail.0.lock().unwrap();
    assert_eq!(state.sent.iter().map(|d| d.to.as_str()).collect::<Vec<_>>(), vec!["owner@example.test", SUPPORT_ADDRESS], "each target is sent exactly once");
    let user_keys: Vec<&str> = state.attempts.iter().filter(|d| d.to == "owner@example.test").map(|d| d.key.as_str()).collect();
    assert!(user_keys.len() == 2 && user_keys[0] == user_keys[1], "the resend carries the same per-target key: {user_keys:?}");
    let support_keys: Vec<&str> = state.attempts.iter().filter(|d| d.to == SUPPORT_ADDRESS).map(|d| d.key.as_str()).collect();
    assert!(support_keys.len() == 2 && support_keys[0] == support_keys[1]);
    assert_ne!(user_keys[0], support_keys[0], "user and support are distinct targets");
}

// EML-005: a job that completed with a non-null error mails support only, with the partial-error subject;
// a clean completion mails nobody. The mail survives a failed first try the same way.
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn eml_005_a_completion_with_an_error_mails_support_only_and_a_clean_completion_mails_nobody() {
    use transgate_core::jobs::events::{JOB_PARTIAL_ERRORS_SUBJECT, JobEvent};
    let (pool, _database) = isolated().await;
    let f = fixture(&pool).await;
    let now = f.clock.now();
    let completed = |job: i64, error: Option<serde_json::Value>| JobEvent::Completed {
        job: transgate_core::JobId(job), account: f.account, provider: None,
        language: transgate_core::languages::Language::from_name("English"), error, callback_url: None, at: now,
    };
    emit(&f, completed(8, None)).await;
    assert_eq!(f.workers.deliver_events().await.unwrap(), 1);
    assert!(f.mail.0.lock().unwrap().attempts.is_empty(), "a clean completion sends nothing");
    assert_eq!(unpublished(&f).await, 0);

    emit(&f, completed(9, Some(serde_json::json!([{"stage": "transcription", "category": "partial", "message": "One segment needed review"}])))).await;
    f.mail.0.lock().unwrap().fail_to = Some(SUPPORT_ADDRESS.into());
    assert_eq!(f.workers.deliver_events().await.unwrap(), 1);
    assert_eq!(unpublished(&f).await, 1);
    f.clock.0.store(now.0 + 61 * SECOND, Ordering::SeqCst);
    f.mail.0.lock().unwrap().fail_to = None;
    assert_eq!(f.workers.deliver_events().await.unwrap(), 1);
    assert_eq!(unpublished(&f).await, 0);
    let state = f.mail.0.lock().unwrap();
    assert_eq!(state.sent.iter().map(|d| (d.to.as_str(), d.subject.as_str())).collect::<Vec<_>>(), vec![(SUPPORT_ADDRESS, JOB_PARTIAL_ERRORS_SUBJECT)], "support only; the user is never mailed");
    assert!(state.attempts.iter().all(|d| d.to == SUPPORT_ADDRESS));
    assert_eq!(state.attempts.len(), 2);
    assert_eq!(state.attempts[0].key, state.attempts[1].key, "the resend keeps its per-target key");
}
