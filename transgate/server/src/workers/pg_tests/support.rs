//! Real-PG support outbox and trusted directory; external mail outcomes are controlled.

use std::collections::HashSet;
use transgate_core::account::{Account, AccountServices, SupportId};
use transgate_core::jobs::events::SUPPORT_ADDRESS;
use transgate_core::jobs::ports::JobQueries;
use billing_core::ports::{Customer, CustomerDirectory};

use super::*;

#[derive(Clone)]
struct Delivery { to: String, key: String }
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
            let delivery = Delivery { to: message.to.clone(), key: key.into() };
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
