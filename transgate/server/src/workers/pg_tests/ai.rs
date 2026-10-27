//! The actual combined worker task: held provider I/O must not hold a different
//! completed job's mail. Own scratch DB isolates global schedule/outbox scans.
use std::sync::atomic::AtomicBool;

use llm_core::{CompletionRequest, CompletionStream, ErrorKind, Llm, LlmError, RemoteOutcome};
use transgate_core::ai::{Ai, AiConfig, AiServices, Bounds, ChatLimits, ModelSettings, SummaryOutcome};
use transgate_core::ai::live::LiveHub;
use transgate_core::jobs::events::{JOB_PARTIAL_ERRORS_SUBJECT, SUPPORT_ADDRESS};
use transgate_core::transcript::NewLine;

use super::*;


#[derive(Default)]
struct HeldLlm { opened: Notify, release: Notify, returned: AtomicBool }
impl Llm for HeldLlm {
    fn open(&self, _: CompletionRequest) -> llm_core::BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>> {
        Box::pin(async move {
            self.opened.notify_one();
            self.release.notified().await;
            self.returned.store(true, Ordering::SeqCst);
            Err(LlmError::new(ErrorKind::Protocol, RemoteOutcome::Unknown))
        })
    }
}
#[derive(Default)]
struct Mailbox { sent: Mutex<Vec<(String, String, String)>>, delivered: Notify }
impl Mailer for Mailbox {
    fn send<'a>(&'a self, message: &'a MailMessage, _: &'a str) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            self.sent.lock().unwrap().push((message.to.clone(), message.subject.clone(), message.text.clone()));
            self.delivered.notify_one();
            Ok(())
        })
    }
}
struct WorkerTask(tokio::task::JoinHandle<()>);
impl Drop for WorkerTask { fn drop(&mut self) { self.0.abort(); } }

async fn completed(store: &PgJobStore, account: AccountId, now: Micros, partial: bool) -> transgate_core::JobId {
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let mut job = uow.insert_job(&NewJob {
        account, status: JobStatus::Completed, name: Some("meeting.mp3".into()),
        source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
        language: Language::from_code("en"), translate_to: vec![], speakers: true, fix_grammer: None,
        duration_minutes: Some(1), size_bytes: Some(10), callback_url: None, whisper_params: None, created_at: now,
    }).await.unwrap();
    uow.insert_lines(&job, &[NewLine {
        start_time:"00:00:00".into(), end_time:"00:00:20".into(),
        line:"Ada will finish accessibility review on Monday; Sam will prepare the release notes before Friday.".into(),
        speaker:Some("Ada".into()), corrected_text:None, split_on_speaker_change:None, language:None, translation_status:None,
    }]).await.unwrap();
    if partial {
        job.error = Some(serde_json::json!([{"stage":"transcription","category":"partial","message":"One segment needed review"}]));
        uow.save_job(&job, job.revision).await.unwrap();
        uow.emit(&JobEvent::for_transition(JobStatus::Running, &job, now).unwrap()).await.unwrap();
    } else {
        Ai::request_summary(uow.as_mut(), account, job.id, now).await.unwrap();
    }
    uow.commit().await.unwrap();
    job.id
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn a_held_then_failed_ai_summary_cannot_hold_partial_error_mail_or_fail_completed_transcription() {
    let (pool, _database) = isolated().await;
    let now = Micros(1_900_000_000 * SECOND);
    let clock: Arc<dyn Clock> = Arc::new(Injected(AtomicI64::new(now.0)));
    let account = AccountId(Uuid::new_v4());
    let store = PgJobStore::new(pool.clone());
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, now).await.unwrap();
    uow.commit().await.unwrap();
    let provider = Arc::new(HeldLlm::default());
    let ai = Arc::new(Ai::new(AiServices {
        store:Arc::new(store.clone()), clock:clock.clone(), llm:Some(provider.clone()),
        sleeper:Arc::new(TokioSleeper), live:Arc::new(LiveHub::new(64*1024,1024*1024)),
    }, AiConfig { features_enabled:true, chat_enabled:true, chat_validation:true, limits:ChatLimits::default(), model:ModelSettings::source(), bounds:Bounds::default() }));
    ai.record_consent(account, true).await.unwrap();
    let summarized = completed(&store, account, now, false).await;
    let transport = Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new())));
    let baseline = replica(&pool, clock.clone(), transport);
    let mail = Arc::new(Mailbox::default());
    let workers = Arc::new(Workers { jobs:baseline.jobs.clone(), privacy:baseline.privacy.clone(), ai:ai.clone(), store:store.clone(),
        pool:pool.clone(), mailer:mail.clone(), directory:PgCustomerDirectory::new(pool.clone()), clock, locks:baseline.locks.clone(), callbacks: None });
    let _task = WorkerTask(super::super::spawn(workers, Duration::from_millis(10)));
    tokio::time::timeout(Duration::from_secs(10), provider.opened.notified()).await.expect("summary worker reached controlled pending provider I/O");
    // Enqueue the other completion only AFTER the AI loop is known to be held.
    let mailed = completed(&store, account, now, true).await;
    if let Err(elapsed) = tokio::time::timeout(Duration::from_secs(10), mail.delivered.notified()).await {
        // The delivery deadline has already failed. Read only this fixture's event errors,
        // best-effort and bounded, so a schema/recipient failure is not hidden by Elapsed.
        let events = tokio::time::timeout(Duration::from_secs(1), sqlx::query_as::<_, (i64, i32, Option<String>)>(
            "SELECT id, attempts, last_error FROM transgate_jobs.events WHERE account_id = $1 ORDER BY id",
        ).bind(account.0).fetch_all(&pool)).await;
        panic!("mail must be sent while the provider remains held: {elapsed}; persisted events (id, attempts, last_error): {events:?}");
    }
    assert!(!provider.returned.load(Ordering::SeqCst));
    {
        let sent = mail.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].0.as_str(), sent[0].1.as_str()), (SUPPORT_ADDRESS, JOB_PARTIAL_ERRORS_SUBJECT));
        assert!(sent[0].2.contains(&format!("Job {mailed} completed with errors")));
    }
    provider.release.notify_one();
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut observer = tokio::time::interval(Duration::from_millis(10));
        loop {
            observer.tick().await;
            if matches!(ai.summary_outcome(summarized).await.unwrap(), Some(SummaryOutcome::Failed { .. })) { break; }
        }
    }).await.expect("the durable summary outcome must record failure, independently of transcription");
    let status: String = sqlx::query_scalar("SELECT status FROM transgate_jobs.jobs WHERE id=$1 AND account_id=$2")
        .bind(summarized.0).bind(account.0).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "completed");
}
