//! The audio-work use cases: enrollment, upload, shared two-provider
//! submission with fallback, callback intake, lease and stuck-work recovery,
//! retention and account erasure (including billing erasure).
//!
//! Every state change goes through `Service::update`: the account lock first
//! (which also checks the erasure barrier), then the work row under
//! `FOR UPDATE`, a pure decision on the aggregate, a compare-and-swap save. No
//! network call is ever made while a transaction is open.

use std::sync::Arc;

use billing_core::erasure::{SubscriptionTermination, open_account_erasure, terminate_subscriptions};
use billing_core::types::{AccountId, Micros as BillingMicros};
use billing_postgres::PgBillingStore;
use media_ports::{
    AccessMethod, CallbackBinding, CallbackError, CallbackSpec, DeleteOutcome, Headers, ObjectRef, ObjectStore, Provider, ProviderObservation,
    RemoteIdentity, SecretString, SensitiveUrl, Submission, TemporaryAccess, Transcriber, TranscriptionRequest, TransportError,
};
use uuid::Uuid;

use crate::correlate::{self, Expected};
use crate::outcome::{Outcome, outcome, result_digest};
use crate::store::{EraseDataError, Enrollment, NewSecret, Store, StoreError, parse_provider, provider_str, reference_for};
use crate::work::{Applied, Micros, PROVIDERS, Policy, Status, Work, WorkError};

const SECOND: i64 = 1_000_000;
const DAY: i64 = 86_400 * SECOND;
/// The default name of the daily retention schedule in `media.schedule_slots`.
pub const RETENTION_SCHEDULE: &str = "taskr-work-retention";
const RETENTION_ABORT_ABOVE: usize = 100_000;
/// How long a refusal without a deadline waits before the provider copy is tried again.
const RETAINED_RETRY_MICROS: i64 = 15 * 60 * SECOND;
/// What the upload sweep says once the issued links have expired. A PUT accepted before its link
/// expired can still commit afterwards, and no provider-documented bound or conditional write fence
/// that would rule it out is configured here, so an absent object proves nothing: the erasure stays
/// open and the key is swept again. A mechanism that proves finality may close it; a delay alone
/// does not.
const UPLOAD_MAY_STILL_COMPLETE: &str = "an upload accepted before its link expired may still complete: the key stays under sweep until finality is proved";
const UPLOAD_COMPLETED_LATE: &str = "an upload completed after its link expired and was deleted: the key stays under sweep until finality is proved";

/// The later of the provider's deadline (or the retry delay when it gave none) and the floor
/// already promised, such as the upload-link hold.
fn retry_at(now: Micros, wanted: Option<Micros>, floor: Option<Micros>) -> Micros {
    Micros(wanted.map_or(now.0 + RETAINED_RETRY_MICROS, |at| at.0).max(floor.map_or(0, |at| at.0)))
}

pub type Clock = Arc<dyn Fn() -> Micros + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0);
        Micros(now)
    })
}

#[derive(Clone, Debug)]
pub struct Config {
    /// The public HTTPS origin providers call back, without a trailing slash.
    pub callback_base: String,
    pub upload_ttl_seconds: u32,
    /// How long the provider may use the audio link it is given.
    pub audio_ttl_seconds: u32,
    pub retention_days: i64,
    pub policy: Policy,
    pub batch: i64,
    pub soniox_model: String,
    pub deepgram_model: String,
    /// Names the retention slot series; one per deployment.
    pub retention_schedule: String,
}

impl Config {
    pub fn new(callback_base: impl Into<String>) -> Config {
        Config {
            callback_base: callback_base.into().trim_end_matches('/').to_string(),
            upload_ttl_seconds: 900,
            audio_ttl_seconds: 3600,
            retention_days: 30,
            policy: Policy::default(),
            batch: 50,
            soniox_model: "stt-async-v5".into(),
            deepgram_model: "whisper-large".into(),
            retention_schedule: RETENTION_SCHEDULE.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    NotEnrolled,
    Erased,
    NotFound,
    /// The state does not allow this (for example the object was never uploaded).
    NotReady(String),
    Invalid(String),
    Conflict,
    /// An authenticated observation named a different remote identity than the attempt's bound one.
    Mismatch,
    /// The same attempt and identity delivered a different result payload than the one applied.
    PayloadConflict,
    Unavailable(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::NotEnrolled => write!(f, "not enrolled in audio work"),
            ServiceError::Erased => write!(f, "the account is unavailable due to account erasure"),
            ServiceError::NotFound => write!(f, "no such work"),
            ServiceError::NotReady(why) => write!(f, "not ready: {why}"),
            ServiceError::Invalid(why) => write!(f, "invalid: {why}"),
            ServiceError::Conflict => write!(f, "the work changed concurrently"),
            ServiceError::Mismatch => write!(f, "the provider named a different remote identity than the one bound"),
            ServiceError::PayloadConflict => write!(f, "the provider delivered a different result for an attempt that already has one"),
            ServiceError::Unavailable(why) => write!(f, "unavailable: {why}"),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<StoreError> for ServiceError {
    fn from(e: StoreError) -> ServiceError {
        match e {
            StoreError::NotEnrolled => ServiceError::NotEnrolled,
            StoreError::Erased => ServiceError::Erased,
            StoreError::Conflict => ServiceError::Conflict,
            StoreError::Unavailable(why) => ServiceError::Unavailable(why),
        }
    }
}

fn transport(e: TransportError) -> ServiceError {
    ServiceError::Unavailable(e.to_string())
}

pub struct Created {
    pub work: Work,
    pub upload: TemporaryAccess,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PassReport {
    pub examined: usize,
    pub changed: usize,
    pub failures: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    /// Applied now.
    Applied,
    /// A repeat, or an attempt that is no longer the live one: acknowledged, nothing changed.
    Ignored,
    /// The account was erased: acknowledged, nothing stored.
    Dropped,
    Unauthorized,
    Mismatch,
    /// The same attempt and identity delivered a different result payload: held, never applied.
    Conflict,
    Malformed,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenResource {
    pub kind: String,
    pub state: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureView {
    pub complete: bool,
    pub open: Vec<OpenResource>,
}

pub struct Service {
    store: Store,
    objects: Arc<dyn ObjectStore>,
    transcribers: Vec<Arc<dyn Transcriber>>,
    billing: PgBillingStore,
    terminators: Vec<Arc<dyn SubscriptionTermination>>,
    config: Config,
    clock: Clock,
}

/// The start of the most recent 02:00 UTC at or before `now`.
pub fn latest_retention_slot(now: Micros) -> Micros {
    let two_hours = 2 * 3600 * SECOND;
    Micros((now.0 - two_hours).div_euclid(DAY) * DAY + two_hours)
}

/// A storage key the account and work ids make unique; the file name is only a readable suffix.
pub fn object_key(account: Uuid, work: Uuid, name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or("");
    let safe: String = leaf.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).take(100).collect();
    let safe = safe.trim_start_matches('.');
    format!("work/{account}/{work}/{}", if safe.is_empty() { "audio" } else { safe })
}

enum Verdict {
    Accepted(RemoteIdentity),
    Rejected(String),
    Uncertain,
}

fn verdict(result: Result<Submission, TransportError>) -> Verdict {
    match result {
        Ok(Submission::Accepted { remote }) => Verdict::Accepted(remote),
        Ok(Submission::Rejected { code, .. }) => Verdict::Rejected(code),
        Ok(Submission::Uncertain { .. }) => Verdict::Uncertain,
        // Nothing was sent: the request itself is what is wrong.
        Err(TransportError::Configuration(_) | TransportError::Unsupported(_) | TransportError::UnsafeDestination | TransportError::LimitExceeded(_)) => {
            Verdict::Rejected("not_submitted".into())
        }
        Err(TransportError::Rejected { code, .. }) => Verdict::Rejected(code),
        // A lost or unreadable answer proves nothing about acceptance.
        Err(TransportError::Unavailable { .. } | TransportError::Malformed(_)) => Verdict::Uncertain,
    }
}

impl Service {
    /// Everything runs on `store`'s pool, including billing: the combined erasure is one
    /// transaction, so billing is reached through this same pool and can only be the same actual
    /// database. There is no second connection to configure or to get wrong.
    pub fn new(
        store: Store,
        objects: Arc<dyn ObjectStore>,
        transcribers: Vec<Arc<dyn Transcriber>>,
        terminators: Vec<Arc<dyn SubscriptionTermination>>,
        config: Config,
        clock: Clock,
    ) -> Service {
        let billing = PgBillingStore::new(store.pool().clone());
        Service { store, objects, transcribers, billing, terminators, config, clock }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    fn now(&self) -> Micros {
        (self.clock)()
    }

    fn available(&self) -> Vec<Provider> {
        self.transcribers.iter().map(|t| t.provider()).collect()
    }

    fn transcriber(&self, provider: Provider) -> Option<&Arc<dyn Transcriber>> {
        self.transcribers.iter().find(|t| t.provider() == provider)
    }

    /// One locked, compare-and-swap change of one work.
    async fn update<T>(&self, account: Uuid, id: Uuid, secrets: &[NewSecret<'_>], change: impl FnOnce(&mut Work) -> T) -> Result<T, ServiceError> {
        self.update_enrolled(account, id, secrets, |_, work| change(work)).await
    }

    /// The same, for a change that also depends on the enrollment read under the account lock.
    async fn update_enrolled<T>(
        &self,
        account: Uuid,
        id: Uuid,
        secrets: &[NewSecret<'_>],
        change: impl FnOnce(&Enrollment, &mut Work) -> T,
    ) -> Result<T, ServiceError> {
        let mut tx = self.store.begin().await?;
        let enrollment = tx.lock_account(account).await?;
        let Some(mut work) = tx.load_work(id).await? else { return Err(ServiceError::NotFound) };
        if work.owner != account {
            return Err(ServiceError::NotFound);
        }
        let before = work.revision;
        let result = change(&enrollment, &mut work);
        if work.revision != before {
            tx.save_work(&work, before, secrets).await?;
        }
        tx.commit().await?;
        Ok(result)
    }

    // ---- accounts ------------------------------------------------------------------

    pub async fn enroll(&self, account: Uuid, email: &str) -> Result<Enrollment, ServiceError> {
        let mut tx = self.store.begin().await?;
        let enrollment = tx.enroll(account, email, self.now()).await?;
        tx.commit().await?;
        Ok(enrollment)
    }

    pub async fn set_auto_delete(&self, account: Uuid, auto_delete: bool) -> Result<(), ServiceError> {
        let mut tx = self.store.begin().await?;
        tx.lock_account(account).await?;
        tx.set_auto_delete(account, auto_delete).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list(&self, account: Uuid) -> Result<Vec<Work>, ServiceError> {
        self.require_live(account).await?;
        Ok(self.store.works_of(account).await?)
    }

    pub async fn get(&self, account: Uuid, id: Uuid) -> Result<Work, ServiceError> {
        self.require_live(account).await?;
        self.store.work(account, id).await?.ok_or(ServiceError::NotFound)
    }

    async fn require_live(&self, account: Uuid) -> Result<(), ServiceError> {
        let mut tx = self.store.begin().await?;
        tx.lock_account(account).await?;
        tx.rollback().await?;
        Ok(())
    }

    // ---- upload --------------------------------------------------------------------

    /// Records the work and signs the upload while the account lock is held, so the link is
    /// always issued strictly before any erasure barrier.
    pub async fn create(&self, account: Uuid, name: &str, language: Option<String>, requested: Option<Provider>) -> Result<Created, ServiceError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(ServiceError::Invalid("a name of 1 to 255 characters is required".into()));
        }
        if requested.is_some_and(|p| !self.available().contains(&p)) {
            return Err(ServiceError::Invalid("that provider is not configured".into()));
        }
        let now = self.now();
        let id = Uuid::new_v4();
        let object = ObjectRef { store: self.objects.kind(), bucket: self.objects.bucket().to_string(), key: object_key(account, id, name), version: None };
        let work = Work::create(id, account, object, name.to_string(), language, requested, now);
        let mut tx = self.store.begin().await?;
        tx.lock_account(account).await?;
        let upload = self.objects.sign(&work.object, AccessMethod::Put, now, self.config.upload_ttl_seconds).await.map_err(transport)?;
        tx.insert_work(&work).await?;
        tx.commit().await?;
        Ok(Created { work, upload })
    }

    /// A fresh upload link for work whose object has not arrived (the first one expired).
    pub async fn reissue_upload(&self, account: Uuid, id: Uuid) -> Result<TemporaryAccess, ServiceError> {
        let now = self.now();
        let mut tx = self.store.begin().await?;
        tx.lock_account(account).await?;
        let work = tx.load_work(id).await?.filter(|w| w.owner == account).ok_or(ServiceError::NotFound)?;
        if work.status != Status::AwaitingUpload {
            return Err(ServiceError::NotReady("the audio was already uploaded".into()));
        }
        let upload = self.objects.sign(&work.object, AccessMethod::Put, now, self.config.upload_ttl_seconds).await.map_err(transport)?;
        tx.rollback().await?;
        Ok(upload)
    }

    /// The client says the upload is done: the object is looked up, never trusted.
    pub async fn confirm_upload(&self, account: Uuid, id: Uuid) -> Result<Work, ServiceError> {
        let work = self.get(account, id).await?;
        if work.status != Status::AwaitingUpload {
            return Ok(work);
        }
        let metadata = self.objects.head(&work.object).await.map_err(transport)?;
        let Some(metadata) = metadata.filter(|m| m.size > 0) else {
            return Err(ServiceError::NotReady("the audio has not been uploaded".into()));
        };
        let now = self.now();
        self.update(account, id, &[], |w| {
            w.object = metadata.object.clone();
            w.uploaded(now)
        })
        .await?
        .map_err(|_| ServiceError::NotReady("the work can no longer accept an upload".into()))?;
        self.get(account, id).await
    }

    // ---- submission ----------------------------------------------------------------

    /// One pass over work that is ready: claim, send, record the answer; fall back on rejection.
    pub async fn dispatch_pass(&self) -> Result<PassReport, ServiceError> {
        let mut report = PassReport::default();
        for (work, account) in self.store.dispatchable(self.config.batch).await? {
            report.examined += 1;
            match self.dispatch_work(account, work).await {
                Ok(true) => report.changed += 1,
                Ok(false) => {}
                Err(e) => report.failures.push(format!("{work}: {e}")),
            }
        }
        Ok(report)
    }

    /// Claims, sends and records one work's next attempt, falling back on a definite refusal.
    /// Returns whether anything changed.
    pub async fn dispatch_work(&self, account: Uuid, work_id: Uuid) -> Result<bool, ServiceError> {
        let mut progressed = false;
        for _ in 0..PROVIDERS.len() {
            let Some(work) = self.store.work(account, work_id).await? else { return Ok(progressed) };
            if !matches!(work.status, Status::Uploaded | Status::Pending) {
                return Ok(progressed);
            }
            let available = self.available();
            let Some(provider) = work.candidates(&available).first().copied() else {
                let applied = self.update(account, work_id, &[], |w| w.exhausted(&available)).await?;
                return Ok(progressed || applied == Applied::Changed);
            };
            let Some(transcriber) = self.transcriber(provider) else { return Ok(progressed) };
            let now = self.now();
            let audio = self.objects.sign(&work.object, AccessMethod::Get, now, self.config.audio_ttl_seconds).await.map_err(transport)?;

            let attempt = Uuid::new_v4();
            let secret = SecretString::new(format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()));
            let project = transcriber.project().to_string();
            let policy = self.config.policy.clone();
            let claimed = self
                .update(account, work_id, &[NewSecret { attempt, secret: &secret, project: &project }], |w| w.claim(provider, attempt, now, &policy))
                .await?;
            if claimed.is_err() {
                // Another process claimed it first.
                return Ok(progressed);
            }
            progressed = true;

            let callback = format!("{}/api/audio-work/callback/{}?work={}&attempt={}", self.config.callback_base, provider_str(provider), work_id, attempt);
            let request = TranscriptionRequest {
                attempt,
                provider,
                project,
                model: match provider {
                    Provider::Soniox => self.config.soniox_model.clone(),
                    Provider::Deepgram => self.config.deepgram_model.clone(),
                },
                language: work.language.clone(),
                translation_target: None,
                diarize: true,
                audio,
                client_reference: reference_for(work_id),
                callback: CallbackSpec { url: SensitiveUrl::new(callback), secret },
            };
            let decided = verdict(transcriber.submit(&request).await);
            let rejected = matches!(decided, Verdict::Rejected(_));
            let remote_for_cleanup = match &decided {
                Verdict::Accepted(remote) => Some(remote.clone()),
                _ => None,
            };
            let now = self.now();
            let applied = self
                .update(account, work_id, &[], |w| match decided {
                    // `observed`, not `accepted`: an acknowledgement that arrives after the attempt was
                    // given up still names remote work, and that identity must be learned for erasure.
                    Verdict::Accepted(remote) => w.observed(attempt, remote, now),
                    Verdict::Rejected(reason) => w.rejected(attempt, reason, now),
                    Verdict::Uncertain => w.uncertain(attempt),
                })
                .await;
            match applied {
                Ok(_) => {}
                Err(ServiceError::Erased | ServiceError::NotEnrolled) => {
                    // Erased between claim and answer: the provider holds work no row names. It goes
                    // into the erasure manifest at once, where the saga deletes it and keeps retrying.
                    if let Some(remote) = remote_for_cleanup {
                        self.store.record_late_remote(account, attempt, &remote).await?;
                    }
                    return Ok(progressed);
                }
                Err(e) => return Err(e),
            }
            if !rejected {
                return Ok(progressed);
            }
        }
        Ok(progressed)
    }

    // ---- callbacks -----------------------------------------------------------------

    /// A provider callback. The stored binding is loaded by the attempt in the query string and
    /// verified before anything is read from the body; the result is applied before the answer.
    pub async fn callback(&self, provider: Provider, work: Uuid, attempt: Uuid, headers: &Headers, body: &[u8]) -> CallbackOutcome {
        let binding = match self.store.binding(attempt).await {
            Ok(Some(binding)) => binding,
            Ok(None) => return CallbackOutcome::Unauthorized,
            Err(_) => return CallbackOutcome::Unavailable,
        };
        if binding.provider != provider || binding.work != work {
            return CallbackOutcome::Mismatch;
        }
        let Some(transcriber) = self.transcriber(provider) else { return CallbackOutcome::Unavailable };
        // The remote identity may not be stored yet (the callback beat the submission's answer):
        // the adapter then authenticates by the secret alone and reports the identity it read.
        let stored = CallbackBinding { attempt, provider, project: binding.project.clone(), expected_remote_id: binding.remote_id.clone(), secret: binding.secret.clone() };
        let observation = match transcriber.verify_callback(&stored, headers, body) {
            Ok(observation) => observation,
            Err(CallbackError::Unauthorized) => return CallbackOutcome::Unauthorized,
            Err(CallbackError::IdentityMismatch) => return CallbackOutcome::Mismatch,
            Err(CallbackError::Malformed) => return CallbackOutcome::Malformed,
        };
        let observation = match transcriber.complete_callback(observation).await {
            Ok(observation) => observation,
            Err(_) => return CallbackOutcome::Unavailable,
        };
        match self.apply_observation(binding.account, binding.work, attempt, &observation).await {
            Ok(Applied::Changed) => CallbackOutcome::Applied,
            Ok(Applied::Unchanged) => CallbackOutcome::Ignored,
            Err(ServiceError::Mismatch) => CallbackOutcome::Mismatch,
            Err(ServiceError::PayloadConflict) => CallbackOutcome::Conflict,
            // Erased between the binding and this answer: the authenticated call still names remote
            // work the erasure manifest may not know. It is registered durably before the 2xx, or
            // the provider is told to retry.
            Err(ServiceError::Erased | ServiceError::NotEnrolled | ServiceError::NotFound) => {
                match self.store.record_late_remote(binding.account, attempt, &observation.remote).await {
                    Ok(()) => CallbackOutcome::Dropped,
                    Err(_) => CallbackOutcome::Unavailable,
                }
            }
            Err(_) => CallbackOutcome::Unavailable,
        }
    }

    /// Applies an authenticated observation under the account lock. The identity it names is
    /// compared with the attempt as stored *now*, before anything else, whether the attempt is
    /// live, finished or given up: a callback that beat the submission answer adopts it (or, for a
    /// given-up attempt, records it for erasure), a different identity is refused. The digest of
    /// the normalized result, the same whether a webhook or a recovery poll delivered it, is
    /// bound once: the same attempt and identity with another result is a conflict, held and never
    /// applied, whatever state the attempt is in.
    async fn apply_observation(&self, account: Uuid, work: Uuid, attempt: Uuid, observation: &ProviderObservation) -> Result<Applied, ServiceError> {
        let verdict = outcome(&observation.payload);
        let digest = result_digest(&verdict);
        let now = self.now();
        let result = self
            .update(account, work, &[], |w| -> Result<Applied, WorkError> {
                let learned = w.observed(attempt, observation.remote.clone(), now)? == Applied::Changed;
                // The result is compared with the one already bound BEFORE any lifecycle decision: a
                // failed or finished attempt answers Stale (or Unchanged) to every lifecycle call,
                // and a different result must not be acknowledged as the delivery already applied.
                if let Some(digest) = &digest {
                    w.check_digest(attempt, digest)?;
                }
                let applied = match verdict {
                    Outcome::Transcript(text) => w.complete(attempt, text, now)?,
                    Outcome::Failed(reason) => w.fail(attempt, reason, now)?,
                    Outcome::Nothing => Applied::Unchanged,
                };
                let recorded = match &digest {
                    Some(digest) => w.record_digest(attempt, digest)? == Applied::Changed,
                    None => false,
                };
                Ok(if learned || recorded { Applied::Changed } else { applied })
            })
            .await?;
        match result {
            Ok(applied) => Ok(applied),
            Err(WorkError::IdentityMismatch) => Err(ServiceError::Mismatch),
            Err(WorkError::PayloadConflict) => Err(ServiceError::PayloadConflict),
            // A stale answer (a superseded attempt, a finished work) is acknowledged and ignored.
            Err(_) => Ok(Applied::Unchanged),
        }
    }

    // ---- recovery ------------------------------------------------------------------

    /// Reconciles one work: an expired claim first, then a stuck accepted attempt.
    pub async fn recover_work(&self, account: Uuid, work: Uuid) -> Result<bool, ServiceError> {
        if self.reconcile_expired(account, work).await? {
            return Ok(true);
        }
        self.reconcile_stuck(account, work).await
    }

    /// Reconciles attempts whose lease ran out and work that has run past the stuck window.
    pub async fn recovery_pass(&self) -> Result<PassReport, ServiceError> {
        let mut report = PassReport::default();
        let now = self.now();
        for (work, account) in self.store.with_expired_attempts(now, self.config.batch).await? {
            report.examined += 1;
            match self.reconcile_expired(account, work).await {
                Ok(true) => report.changed += 1,
                Ok(false) => {}
                Err(e) => report.failures.push(format!("{work}: {e}")),
            }
        }
        let cutoff = now.0 - self.config.policy.stuck_after_micros;
        for (work, account) in self.store.running_since_before(cutoff, self.config.batch).await? {
            report.examined += 1;
            match self.reconcile_stuck(account, work).await {
                Ok(true) => report.changed += 1,
                Ok(false) => {}
                Err(e) => report.failures.push(format!("{work}: {e}")),
            }
        }
        Ok(report)
    }

    /// A claim with no recorded answer. The provider's listing by reference decides, and only a
    /// record that matches everything the attempt sent is adopted. No match and several matches
    /// are no proof either way: the attempt stays as it is, unresolved and visible, and the next
    /// pass looks again. Only a provider with no listing at all (Deepgram) can be given up for the
    /// other provider; that may buy duplicate remote work, which the contract accepts there only.
    async fn reconcile_expired(&self, account: Uuid, work_id: Uuid) -> Result<bool, ServiceError> {
        let now = self.now();
        let Some(work) = self.store.work(account, work_id).await? else { return Ok(false) };
        let Some(attempt) = work.expired(now).cloned() else { return Ok(false) };
        let Some(transcriber) = self.transcriber(attempt.provider) else { return Ok(false) };
        let Some(binding) = self.store.binding(attempt.id).await? else { return Ok(false) };
        let expected = Expected {
            attempt: attempt.id,
            provider: attempt.provider,
            project: binding.project,
            reference: reference_for(work_id),
            object_key: work.object.key.clone(),
            claimed_at: attempt.claimed_at,
            lease_until: attempt.lease_until,
        };
        let found = match transcriber.find_by_reference(&expected.reference).await {
            Ok(records) => {
                let mut matching: Vec<RemoteIdentity> = records.iter().filter(|r| correlate::matches(r, &expected)).map(|r| r.remote.clone()).collect();
                match matching.len() {
                    1 => matching.pop(),
                    _ => return Ok(false),
                }
            }
            Err(TransportError::Unsupported(_)) => None,
            Err(e) => return Err(transport(e)),
        };
        let applied = self
            .update(account, work_id, &[], |w| match found {
                Some(remote) => w.observed(attempt.id, remote, now),
                None => w.supersede(attempt.id, now),
            })
            .await?;
        Ok(applied.is_ok())
    }

    /// Accepted work past the stuck window: take the result if the provider has one, otherwise
    /// give the attempt up so the other provider is tried.
    async fn reconcile_stuck(&self, account: Uuid, work_id: Uuid) -> Result<bool, ServiceError> {
        let now = self.now();
        let Some(work) = self.store.work(account, work_id).await? else { return Ok(false) };
        let Some(attempt) = work.stuck(now, &self.config.policy).cloned() else { return Ok(false) };
        let (Some(remote), Some(transcriber)) = (attempt.remote.clone(), self.transcriber(attempt.provider)) else { return Ok(false) };
        match transcriber.result(&remote).await {
            Ok(observation) => match outcome(&observation.payload) {
                Outcome::Nothing => {}
                _ => return Ok(self.apply_observation(account, work_id, attempt.id, &observation).await? == Applied::Changed),
            },
            Err(TransportError::Unsupported(_)) => {}
            Err(e) => return Err(transport(e)),
        }
        let applied = self.update(account, work_id, &[], |w| w.supersede(attempt.id, now)).await?;
        Ok(applied.is_ok())
    }

    // ---- retention -----------------------------------------------------------------

    /// The daily 02:00 UTC run: one claimable slot, the inclusive extra-day cutoff, aborted rather
    /// than run over an implausible number of rows.
    pub async fn retention_pass(&self) -> Result<PassReport, ServiceError> {
        let now = self.now();
        let slot = latest_retention_slot(now);
        let claim = media_postgres::claim_slot(self.store.pool(), &self.config.retention_schedule, slot.0, now.0, 600)
            .await
            .map_err(|e| ServiceError::Unavailable(e.to_string()))?;
        let Some(claim) = claim else { return Ok(PassReport::default()) };
        let cutoff = now.0 - (self.config.retention_days + 1) * DAY;
        let candidates = self.store.retention_candidates(cutoff, RETENTION_ABORT_ABOVE as i64 + 1).await?;
        let mut report = PassReport::default();
        let finish = |outcome: &'static str| self.finish_slot(&claim, outcome);
        if candidates.len() > RETENTION_ABORT_ABOVE {
            finish("aborted").await?;
            report.failures.push("retention aborted: too many rows".into());
            return Ok(report);
        }
        for (index, (work, account)) in candidates.iter().enumerate() {
            report.examined += 1;
            if index % 100 == 99 && !media_postgres::renew_slot(self.store.pool(), &claim, self.now().0, 600).await.unwrap_or(false) {
                report.failures.push("retention lost its slot".into());
                return Ok(report);
            }
            match self.retention_purge(*account, *work).await {
                Ok(Applied::Changed) => report.changed += 1,
                Ok(Applied::Unchanged) | Err(ServiceError::Erased | ServiceError::NotFound) => {}
                Err(e) => report.failures.push(format!("{work}: {e}")),
            }
        }
        finish(if report.failures.is_empty() { "succeeded" } else { "failed" }).await?;
        Ok(report)
    }

    /// Removes one work's transcript if the work is due and its account still keeps auto-delete on,
    /// both read under the same account lock the preference is written under. An opt-out that
    /// committed after the sweep chose its candidates therefore wins.
    pub async fn retention_purge(&self, account: Uuid, work: Uuid) -> Result<Applied, ServiceError> {
        let now = self.now();
        let days = self.config.retention_days;
        self.update_enrolled(account, work, &[], |enrollment, w| {
            if enrollment.auto_delete && w.retention_due(now, days) { w.purge_transcript() } else { Applied::Unchanged }
        })
        .await
    }

    async fn finish_slot(&self, claim: &media_postgres::SlotClaim, outcome: &str) -> Result<bool, ServiceError> {
        media_postgres::finish_slot(self.store.pool(), claim, self.now().0, outcome).await.map_err(|e| ServiceError::Unavailable(e.to_string()))
    }

    // ---- erasure -------------------------------------------------------------------

    /// Starts (or repeats) the account's erasure and drives it as far as it can go now:
    /// this schema's barrier, billing's barrier, the external subscriptions (D10), then ONE
    /// transaction that erases billing, the enrolled sample resources and this schema's rows on
    /// the shared database, then the external manifest (objects and provider jobs), retried until
    /// every resource is confirmed.
    pub async fn erase_account(&self, account: Uuid) -> Result<ErasureView, ServiceError> {
        self.advance_erasure(account).await
    }

    /// One erasure advances in one place at a time: a second pass (another replica, the request that
    /// started it) finds the lock taken and reports where it stands instead of repeating provider
    /// calls. This only saves duplicate provider calls: the session lock can be lost (a broken
    /// connection ends it) and a second driver may then finish and drop resource rows while the first
    /// still awaits a listing. Correctness therefore rests on the account lock inside the store:
    /// `finish_erasure`, `record_late_remote` and `identify_resource` all take it, and the last two
    /// register what they learned even when its resource row is gone, reopening a closed erasure.
    async fn advance_erasure(&self, account: Uuid) -> Result<ErasureView, ServiceError> {
        let key = media_postgres::LockKey::named(&format!("taskr-work.erasure.{account}"));
        let held = media_postgres::try_lock(self.store.pool(), key).await.map_err(|e| ServiceError::Unavailable(e.to_string()))?;
        let Some(lock) = held else { return self.erasure_view(account).await };
        let view = self.advance_locked(account).await;
        let _ = lock.release().await;
        view
    }

    async fn advance_locked(&self, account: Uuid) -> Result<ErasureView, ServiceError> {
        let now = self.now();
        self.store.touch_erasure(account, now).await?;
        self.store.open_barrier(account, now).await?;
        let data_erased = matches!(self.store.erasure_progress(account).await?, Some((Some(_), _)));
        if !data_erased {
            let billing_account = AccountId(account);
            let at = BillingMicros(now.0);
            let unavailable = |e: billing_core::ports::StoreError| ServiceError::Unavailable(e.to_string());
            open_account_erasure(&self.billing, billing_account, at).await.map_err(unavailable)?;
            let terminators: Vec<&dyn SubscriptionTermination> = self.terminators.iter().map(|t| t.as_ref()).collect();
            let status = terminate_subscriptions(&self.billing, &terminators, billing_account, at).await.map_err(unavailable)?;
            if !status.subscriptions_stopped() {
                self.store.set_erasure_note(account, "subscriptions are not confirmed stopped").await?;
                return self.erasure_view(account).await;
            }
            let hold_until = Micros(now.0 + i64::from(self.config.upload_ttl_seconds) * SECOND);
            match self.store.erase_data(account, now, hold_until).await {
                Ok(_) => {}
                Err(EraseDataError::Blocked(why)) => {
                    self.store.set_erasure_note(account, &why).await?;
                    return self.erasure_view(account).await;
                }
                Err(EraseDataError::Store(e)) => return Err(e.into()),
            }
        }
        self.drive_erasure(account).await
    }

    /// Continues every unfinished erasure.
    pub async fn erasure_pass(&self) -> Result<PassReport, ServiceError> {
        let mut report = PassReport::default();
        for account in self.store.pending_erasures(self.config.batch).await? {
            report.examined += 1;
            match self.advance_erasure(account).await {
                Ok(view) if view.complete => report.changed += 1,
                Ok(_) => {}
                Err(e) => report.failures.push(format!("{account}: {e}")),
            }
        }
        Ok(report)
    }

    pub async fn erasure_view(&self, account: Uuid) -> Result<ErasureView, ServiceError> {
        let progress = self.store.erasure_progress(account).await?;
        let resources = self.store.erasure_resources(account).await?;
        let mut open: Vec<OpenResource> = resources
            .iter()
            .filter(|r| !matches!(r.state.as_str(), "done" | "absent"))
            .map(|r| OpenResource { kind: r.kind.clone(), state: r.state.clone(), reason: r.last_error.clone() })
            .collect();
        let complete = match &progress {
            Some((Some(_), _)) => resources.is_empty(),
            Some((None, note)) => {
                open.insert(0, OpenResource { kind: "billing".into(), state: "pending".into(), reason: note.clone() });
                false
            }
            None => false,
        };
        Ok(ErasureView { complete, open })
    }

    async fn drive_erasure(&self, account: Uuid) -> Result<ErasureView, ServiceError> {
        let now = self.now();
        for resource in self.store.erasure_resources(account).await? {
            let due = resource.blocked_until.is_none_or(|until| until <= now);
            match (resource.kind.as_str(), resource.state.as_str()) {
                (_, "done" | "absent") => {}
                ("object", _) => self.erase_object(&resource, due, now).await?,
                ("remote", "pending" | "blocked") if due => self.erase_remote(&resource, now).await?,
                ("unidentified", "pending" | "unresolved") => self.reconcile_unidentified(&resource).await?,
                ("llm", "pending") => self.store.set_resource(resource.id, "unresolved", None, Some("External summary processing may retain data; provider deletion or non-retention is not verified.")).await?,
                _ => {}
            }
        }
        self.store.finish_erasure(account, self.now()).await?;
        self.erasure_view(account).await
    }

    async fn erase_object(&self, resource: &crate::store::ErasureResource, due: bool, now: Micros) -> Result<(), ServiceError> {
        // `blocked` and `unresolved` rows carry the time they are next worth trying. A `pending` row's
        // deadline is the upload-link hold of the first sweep, which still runs at once.
        if matches!(resource.state.as_str(), "blocked" | "unresolved") && !due {
            return Ok(());
        }
        let Ok(object) = serde_json::from_value::<ObjectRef>(resource.target.clone()) else {
            return Ok(self.store.set_resource(resource.id, "unresolved", None, Some("unreadable object reference")).await?);
        };
        // The generation an upload was confirmed at is deleted by name; that is a known object and its
        // deletion is final. The key without a version is the sweep for uploads that were issued and
        // may not have finished, and nothing final can be said about it (see `UPLOAD_MAY_STILL_COMPLETE`).
        let key_sweep = object.version.is_none();
        match self.objects.delete(&object).await {
            Ok(outcome @ (DeleteOutcome::Deleted | DeleteOutcome::Absent)) => match resource.blocked_until {
                // An upload link issued before the barrier may still create the object again until
                // it expires; the object is removed once more after that.
                Some(until) if until > now => self.store.set_resource(resource.id, "blocked", Some(until), None).await?,
                // Past the expiry an absent key is not proof: an upload accepted before then may still
                // commit. The sweep stays explicit and open and is repeated.
                _ if key_sweep => {
                    let reason = if outcome == DeleteOutcome::Deleted { UPLOAD_COMPLETED_LATE } else { UPLOAD_MAY_STILL_COMPLETE };
                    self.store.set_resource(resource.id, "unresolved", Some(retry_at(now, None, None)), Some(reason)).await?
                }
                _ => self.store.set_resource(resource.id, "done", None, None).await?,
            },
            Ok(DeleteOutcome::Retained { until, reason }) => {
                // Never earlier than the upload-link hold: a shorter retention hold must not let the
                // re-sweep after the links expire be skipped.
                let at = retry_at(now, until, resource.blocked_until);
                self.store.set_resource(resource.id, "blocked", Some(at), Some(&reason)).await?
            }
            Ok(DeleteOutcome::Unsupported { reason }) => self.store.set_resource(resource.id, "unresolved", None, Some(&reason)).await?,
            Err(e) => self.store.set_resource(resource.id, "pending", resource.blocked_until, Some(&e.to_string())).await?,
        }
        Ok(())
    }

    async fn erase_remote(&self, resource: &crate::store::ErasureResource, now: Micros) -> Result<(), ServiceError> {
        let Ok(remote) = serde_json::from_value::<RemoteIdentity>(resource.target.clone()) else {
            return Ok(self.store.set_resource(resource.id, "unresolved", None, Some("unreadable remote identity")).await?);
        };
        let Some(transcriber) = self.transcriber(remote.provider) else {
            return Ok(self.store.set_resource(resource.id, "pending", None, Some("provider not configured")).await?);
        };
        match transcriber.delete_remote(&remote).await {
            Ok(DeleteOutcome::Deleted | DeleteOutcome::Absent) => self.store.set_resource(resource.id, "done", None, None).await?,
            Ok(DeleteOutcome::Retained { until, reason }) => {
                // "Not while processing" passes: keep it runnable, retried at its deadline or later.
                let at = retry_at(now, until, None);
                self.store.set_resource(resource.id, "blocked", Some(at), Some(&reason)).await?
            }
            Ok(DeleteOutcome::Unsupported { reason }) => self.store.set_resource(resource.id, "unresolved", None, Some(&reason)).await?,
            Err(e) => self.store.set_resource(resource.id, "pending", None, Some(&e.to_string())).await?,
        }
        Ok(())
    }

    /// An attempt whose answer was lost. Every listed job that matches everything the attempt sent
    /// (provider, the project it was submitted under, reference, exact callback attempt, audio object
    /// and time window) is remote work to delete: all of them become resources, none is chosen over
    /// another. No match proves nothing and leaves the resource unresolved.
    async fn reconcile_unidentified(&self, resource: &crate::store::ErasureResource) -> Result<(), ServiceError> {
        let target = &resource.target;
        let text = |key: &str| target.get(key).and_then(|v| v.as_str());
        let number = |key: &str| target.get(key).and_then(|v| v.as_i64());
        let expected = match (
            text("provider").and_then(parse_provider),
            text("attempt").and_then(|v| Uuid::parse_str(v).ok()),
            text("project"),
            text("reference"),
            text("object_key"),
            number("claimed_us"),
            number("lease_until_us"),
        ) {
            (Some(provider), Some(attempt), Some(project), Some(reference), Some(object_key), Some(claimed), Some(lease)) => Expected {
                attempt,
                provider,
                project: project.to_string(),
                reference: reference.to_string(),
                object_key: object_key.to_string(),
                claimed_at: Micros(claimed),
                lease_until: Micros(lease),
            },
            _ => return Ok(self.store.set_unidentified(resource.id, "unresolved", Some("unreadable attempt facts")).await?),
        };
        let Some(transcriber) = self.transcriber(expected.provider) else {
            return Ok(self.store.set_unidentified(resource.id, "unresolved", Some("provider not configured")).await?);
        };
        match transcriber.find_by_reference(&expected.reference).await {
            Ok(records) => {
                let matching: Vec<RemoteIdentity> = records.iter().filter(|r| correlate::matches(r, &expected)).map(|r| r.remote.clone()).collect();
                if matching.is_empty() {
                    self.store.set_unidentified(resource.id, "unresolved", Some("not found by reference: absence is not proved")).await?;
                } else {
                    self.store.identify_resource(resource.account, resource.id, &matching).await?;
                }
            }
            Err(e) => self.store.set_unidentified(resource.id, "unresolved", Some(&e.to_string())).await?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retention_slot_is_the_latest_0200_utc_at_or_before_now() {
        let at = |day: i64, hour: i64, minute: i64| Micros(day * DAY + hour * 3600 * SECOND + minute * 60 * SECOND);
        assert_eq!(latest_retention_slot(at(10, 2, 0)), at(10, 2, 0), "the slot instant itself belongs to its own slot");
        assert_eq!(latest_retention_slot(at(10, 1, 59)), at(9, 2, 0));
        assert_eq!(latest_retention_slot(at(10, 23, 59)), at(10, 2, 0));
        assert_eq!(latest_retention_slot(at(10, 0, 0)), at(9, 2, 0));
    }

    #[test]
    fn an_object_key_is_unique_per_work_and_never_trusts_the_file_name() {
        let account = Uuid::from_u128(1);
        let work = Uuid::from_u128(2);
        let key = object_key(account, work, "../../etc/pass wd.mp3");
        assert_eq!(key, format!("work/{account}/{work}/pass_wd.mp3"));
        assert_eq!(object_key(account, work, "..."), format!("work/{account}/{work}/audio"));
        assert!(!object_key(account, work, &"a".repeat(500)).ends_with(&"a".repeat(101)));
    }
}
