//! The API use cases: key authentication (API-001..003, D2), authorization and
//! the per-key limiter (D1, D3), idempotent public starts (D4), key
//! administration (API-005/006/019, API-D02) and the callback dispatcher
//! (D5..D7, D10). No HTTP: the host maps [`ApiError`] and the rate headers.

use std::sync::Arc;

use billing_core::ports::StoreError;
use billing_core::types::Clock;
use uuid::Uuid;

use crate::ai::Ai;
use crate::ai::ports::SummaryOutcome;
use crate::api::callbacks::{self, DeliveryId, DeliveryState, SendOutcome, SummaryGate};
use crate::api::idempotency::IdempotencyClaim;
use crate::api::keys::{self, ApiKey, CreatedKey, KeyView, NewKey};
use crate::api::limits::{self, ApiRate, RateLimits, RateWindows};
use crate::api::ports::{
    ApiCrypto, ApiRateLimiter, CallbackAttempt, CallbackContent, CallbackContentFacts, CallbackOutbox, CallbackRequest,
    CallbackTransport, ClaimedDelivery, KeyUse,
};
use crate::api::{Action, Actor, ApiError, KeyId, Resource, can};
use crate::ids::{AccountId, JobId, MICROS_PER_SECOND, Micros};
use crate::jobs::app::{Jobs, PublicAnswer, PublicStart};
use crate::jobs::domain::Job;
use crate::jobs::events::JobEvent;
use crate::jobs::ports::{BoxFuture, JobStore, JobUow};

/// The longest accepted `Idempotency-Key`.
pub const MAX_IDEMPOTENCY_KEY: usize = 255;

#[derive(Clone)]
pub struct ApiServices {
    pub store: Arc<dyn JobStore>,
    pub jobs: Arc<Jobs>,
    pub crypto: Arc<dyn ApiCrypto>,
    pub limiter: Arc<dyn ApiRateLimiter>,
    pub clock: Arc<dyn Clock>,
    /// RTL-001 defaults unless configured.
    pub limits: RateLimits,
}

pub struct Api {
    s: ApiServices,
}

/// A use case's own answer after authentication and the limiter admitted the
/// request: `rate` always feeds the RTL-003 headers.
#[derive(Clone, Debug, PartialEq)]
pub struct Admitted<T> {
    pub rate: ApiRate,
    pub result: Result<T, ApiError>,
}

/// Runs `body` in one transaction: commit on success, roll back otherwise.
async fn in_tx<T>(
    store: &dyn JobStore,
    body: impl AsyncFnOnce(&mut dyn JobUow) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let mut uow = store.begin().await?;
    match body(uow.as_mut()).await {
        Ok(value) => {
            uow.commit().await?;
            Ok(value)
        }
        Err(e) => {
            let _ = uow.rollback().await;
            Err(e)
        }
    }
}

impl Api {
    pub fn new(services: ApiServices) -> Api {
        Api { s: services }
    }

    fn now(&self) -> Micros {
        self.s.clock.now()
    }

    /// API-001/002/003, D2: a bearer value to a key actor. A malformed value
    /// is refused without a store read; a digest miss scans only active,
    /// unexpired, not yet upgraded legacy rows and upgrades the match.
    pub async fn authenticate(&self, bearer: &str) -> Result<Actor, ApiError> {
        if !keys::well_formed(bearer) {
            return Err(ApiError::Unauthorized(keys::INVALID_KEY));
        }
        let now = self.now();
        let digest = self.s.crypto.sha256_hex(bearer.as_bytes());
        let found = in_tx(self.s.store.as_ref(), async |uow| Ok(uow.api().key_by_digest(&digest).await?)).await?;
        let key = match found {
            Some(key) => key,
            None => self.upgrade_legacy(bearer, &digest, now).await?.ok_or(ApiError::Unauthorized(keys::INVALID_KEY))?,
        };
        keys::check(&key, now).map_err(ApiError::Unauthorized)?;
        if self.s.store.barrier(key.account).await? {
            return Err(ApiError::Erased);
        }
        // API-003: a successful verification updates `last_used_at`.
        in_tx(self.s.store.as_ref(), async |uow| {
            if let Some(mut row) = uow.api().lock_key(key.id).await? {
                row.last_used_at = Some(now);
                uow.api().save_key(&row).await?;
            }
            Ok(())
        })
        .await?;
        Ok(Actor::ApiKey { account: key.account, key: key.id, scopes: key.scopes })
    }

    /// D2: verify against each legacy candidate outside any transaction (bcrypt
    /// is slow), then write the digest and null the hash under the row lock.
    async fn upgrade_legacy(&self, bearer: &str, digest: &str, now: Micros) -> Result<Option<ApiKey>, ApiError> {
        let candidates = in_tx(self.s.store.as_ref(), async |uow| Ok(uow.api().legacy_candidates(now).await?)).await?;
        for candidate in candidates {
            let Some(hash) = candidate.legacy_bcrypt.as_deref() else { continue };
            if !self.s.crypto.verify_legacy(bearer, hash).await {
                continue;
            }
            let upgraded = in_tx(self.s.store.as_ref(), async |uow| {
                let Some(mut row) = uow.api().lock_key(candidate.id).await? else { return Ok(None) };
                if row.legacy_bcrypt.as_deref() == Some(hash) {
                    row.digest = Some(digest.to_string());
                    row.legacy_bcrypt = None;
                    row.hint = Some(keys::hint(bearer));
                    uow.api().save_key(&row).await?;
                }
                Ok(Some(row))
            })
            .await;
            return match upgraded {
                Ok(row) => Ok(row),
                // A concurrent request upgraded the same key first: read it by digest.
                Err(ApiError::Unavailable(_)) => in_tx(self.s.store.as_ref(), async |uow| Ok(uow.api().key_by_digest(digest).await?)).await,
                Err(e) => Err(e),
            };
        }
        Ok(None)
    }

    /// D1: `can`, then one unit of the key's limiter (a refused action is not
    /// counted). A limiter store error is 503 (D3); over the limit is 429.
    pub async fn authorize(&self, actor: &Actor, action: Action, resource: Resource) -> Result<ApiRate, ApiError> {
        let Actor::ApiKey { key, .. } = actor else { return Err(ApiError::Forbidden) };
        if !can(actor, action, resource) {
            return Err(ApiError::Forbidden);
        }
        let windows = RateWindows::at(self.now());
        let counts = self.s.limiter.consume(*key, windows, self.s.limits).await.map_err(|e| ApiError::Unavailable(e.to_string()))?;
        let rate = limits::decide(windows, self.s.limits, counts);
        if !rate.allowed {
            return Err(ApiError::RateLimited(rate));
        }
        Ok(rate)
    }

    /// `POST /api/v1/transcriptions` (API-007/010, D4).
    pub async fn start(&self, actor: &Actor, request: PublicStart, idempotency_key: Option<String>) -> Result<Admitted<PublicAnswer>, ApiError> {
        let account = actor.account();
        let rate = self.authorize(actor, Action::TranscriptionCreate, Resource::Account(account)).await?;
        let result = async {
            let claim = match idempotency_key {
                None => None,
                Some(key) if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY => {
                    return Err(ApiError::Invalid(format!("Idempotency-Key must be 1 to {MAX_IDEMPOTENCY_KEY} characters")));
                }
                Some(key) => {
                    let canonical = serde_json::to_vec(&request).map_err(|e| ApiError::Invalid(e.to_string()))?;
                    Some(IdempotencyClaim { key, request_digest: self.s.crypto.sha256_hex(&canonical) })
                }
            };
            Ok(self.s.jobs.start_public(account, request, claim).await?)
        }
        .await;
        Ok(Admitted { rate, result })
    }

    /// D11: one audit row per authenticated request.
    pub async fn record_use(&self, usage: &KeyUse) -> Result<(), ApiError> {
        in_tx(self.s.store.as_ref(), async |uow| Ok(uow.api().record_use(usage).await?)).await
    }

    /// API-002/019: the plaintext is returned once; only its digest is stored.
    pub async fn create_key(&self, actor: &Actor, new: NewKey) -> Result<CreatedKey, ApiError> {
        let account = actor.account();
        if !can(actor, Action::KeysManage, Resource::Account(account)) {
            return Err(ApiError::Forbidden);
        }
        let now = self.now();
        let api_key = format!("{}{}", keys::KEY_PREFIX, self.s.crypto.random_token());
        let key = ApiKey {
            id: KeyId(Uuid::new_v4()),
            account,
            name: new.name,
            created_at: now,
            expires_at: keys::expires_at(new.expiration_policy, new.custom_days, now),
            last_used_at: None,
            active: true,
            scopes: new.scopes.iter().map(|s| s.as_str().to_string()).collect(),
            expiration_policy: new.expiration_policy,
            metadata: serde_json::json!({}),
            digest: Some(self.s.crypto.sha256_hex(api_key.as_bytes())),
            legacy_bcrypt: None,
            hint: Some(keys::hint(&api_key)),
        };
        in_tx(self.s.store.as_ref(), async |uow| {
            // Account writes share the erasure barrier and the billing account lock.
            if uow.lock_barrier(account).await?.is_some() {
                return Err(ApiError::Erased);
            }
            uow.billing().await?.lock_account(account).await?;
            Ok(uow.api().insert_key(&key).await?)
        })
        .await?;
        Ok(CreatedKey { key: KeyView::from(&key), api_key })
    }

    /// API-005: the session owner's keys, newest first.
    pub async fn list_keys(&self, actor: &Actor) -> Result<Vec<KeyView>, ApiError> {
        let account = actor.account();
        if !can(actor, Action::KeysManage, Resource::Account(account)) {
            return Err(ApiError::Forbidden);
        }
        let keys = in_tx(self.s.store.as_ref(), async |uow| Ok(uow.api().list_keys(account).await?)).await?;
        Ok(keys.iter().map(KeyView::from).collect())
    }

    /// API-006 with API-D02: only the owner revokes; another account's key is not found.
    pub async fn revoke_key(&self, actor: &Actor, id: KeyId) -> Result<KeyView, ApiError> {
        if !matches!(actor, Actor::Session { .. }) {
            return Err(ApiError::Forbidden);
        }
        in_tx(self.s.store.as_ref(), async |uow| {
            let Some(mut key) = uow.api().lock_key(id).await? else { return Err(ApiError::NotFound) };
            if !can(actor, Action::KeysManage, Resource::Key { id, owner: key.account }) {
                return Err(ApiError::NotFound);
            }
            key.active = false;
            uow.api().save_key(&key).await?;
            Ok(KeyView::from(&key))
        })
        .await
    }
}

/// What one dispatch pass did, for logs and tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallbackPass {
    pub claimed: u32,
    pub delivered: u32,
    pub retrying: u32,
    pub failed: u32,
    pub abandoned: u32,
    pub waiting: u32,
    pub errors: u32,
}

#[derive(Clone)]
pub struct CallbackServices {
    pub outbox: Arc<dyn CallbackOutbox>,
    pub transport: Arc<dyn CallbackTransport>,
    pub content: Arc<dyn CallbackContent>,
    pub crypto: Arc<dyn ApiCrypto>,
    pub clock: Arc<dyn Clock>,
}

/// The delivery worker's use case (D5). The host's event relay calls
/// [`Callbacks::fan_out`] in its own transaction; `Workers` runs `dispatch_pass`.
pub struct Callbacks {
    s: CallbackServices,
}

/// Lease for one claimed delivery: longer than the transport timeout.
pub const DELIVERY_LEASE_SECONDS: i64 = 60;

enum Step {
    Delivered,
    Retrying,
    Failed,
    Abandoned,
    Waiting,
    Superseded,
}

impl Callbacks {
    pub fn new(services: CallbackServices) -> Callbacks {
        Callbacks { s: services }
    }

    /// D5/D6: the delivery of a completed job's event, idempotent on `event_id`.
    /// `true` when a delivery was created.
    pub async fn fan_out(uow: &mut dyn JobUow, event_id: i64, event: &JobEvent, now: Micros) -> Result<bool, StoreError> {
        match callbacks::delivery_for(DeliveryId(Uuid::new_v4()), event_id, event, now) {
            Some(delivery) => uow.api().insert_delivery(&delivery).await,
            None => Ok(false),
        }
    }

    /// One pass over due deliveries: wait for the summary (D7), record the
    /// attempt, sign and send (D5), then settle by the schedule.
    pub async fn dispatch_pass(&self, limit: u32) -> CallbackPass {
        let mut pass = CallbackPass::default();
        let claims = match self.s.outbox.claim_deliveries(limit, DELIVERY_LEASE_SECONDS, self.s.clock.now()).await {
            Ok(claims) => claims,
            Err(_) => {
                pass.errors += 1;
                return pass;
            }
        };
        for claim in claims {
            pass.claimed += 1;
            match self.deliver(&claim).await {
                Ok(Step::Delivered) => pass.delivered += 1,
                Ok(Step::Retrying) => pass.retrying += 1,
                Ok(Step::Failed) => pass.failed += 1,
                Ok(Step::Abandoned) => pass.abandoned += 1,
                Ok(Step::Waiting) => pass.waiting += 1,
                Ok(Step::Superseded) => {}
                Err(_) => {
                    pass.errors += 1;
                    // Unsent: due again after the recheck interval; the lease also expires.
                    let _ = self.s.outbox.postpone(&claim, Micros(self.s.clock.now().0 + callbacks::SUMMARY_RECHECK)).await;
                }
            }
        }
        pass
    }

    async fn deliver(&self, claim: &ClaimedDelivery) -> Result<Step, String> {
        let now = self.s.clock.now();
        let d = &claim.delivery;
        let outcome = self.s.content.summary_outcome(d.job).await?;
        if let SummaryGate::Wait { recheck_at } = callbacks::summary_gate(outcome.as_ref(), d.completed_at, now) {
            return Ok(if self.s.outbox.postpone(claim, recheck_at).await.map_err(|e| e.to_string())? { Step::Waiting } else { Step::Superseded });
        }
        let Some(facts) = self.s.content.content(d.account, d.job).await? else {
            let ended = self.s.outbox.abandon(claim, "the job no longer exists", now).await.map_err(|e| e.to_string())?;
            return Ok(if ended { Step::Abandoned } else { Step::Superseded });
        };
        let stored = callbacks::payload(&facts.job, &facts.lines, facts.summary.as_deref());
        let secret = self.s.outbox.signing_secret(d.account, &self.s.crypto.random_token()).await.map_err(|e| e.to_string())?;
        let mut attempt = CallbackAttempt { delivery: d.id, n: d.attempts + 1, started_at: now, outcome: None, finished_at: None };
        // D5, API-D07: the attempt is durable before the POST.
        if !self.s.outbox.begin_attempt(claim, &attempt, &stored).await.map_err(|e| e.to_string())? {
            return Ok(Step::Superseded);
        }
        let audio = self.s.content.audio_url(&facts.job).await;
        let body = serde_json::to_vec(&callbacks::with_audio_url(&stored, audio.as_deref())).map_err(|e| e.to_string())?;
        let t = now.0.div_euclid(MICROS_PER_SECOND);
        let hmac = self.s.crypto.hmac_sha256_hex(secret.as_bytes(), &callbacks::signed_message(t, &body));
        let request = CallbackRequest { url: d.url.clone(), body, delivery: d.id, signature: callbacks::signature(t, &hmac) };
        let sent: SendOutcome = self.s.transport.post(&request).await;
        let finished = self.s.clock.now();
        let settlement = callbacks::settle(attempt.n, &sent, finished);
        attempt.outcome = Some(sent);
        attempt.finished_at = Some(finished);
        if !self.s.outbox.finish_attempt(claim, &attempt, &settlement).await.map_err(|e| e.to_string())? {
            return Ok(Step::Superseded);
        }
        Ok(match settlement.state {
            DeliveryState::Delivered => Step::Delivered,
            DeliveryState::Pending => Step::Retrying,
            DeliveryState::Failed => Step::Failed,
            DeliveryState::Abandoned => Step::Abandoned,
        })
    }
}

/// [`CallbackContent`] over the job store, `Jobs` (audio signing) and `Ai`
/// (summary outcome and the consent-visible summary).
pub struct JobsCallbackContent {
    pub store: Arc<dyn JobStore>,
    pub jobs: Arc<Jobs>,
    pub ai: Arc<Ai>,
}

impl CallbackContent for JobsCallbackContent {
    fn summary_outcome(&self, job: JobId) -> BoxFuture<'_, Result<Option<SummaryOutcome>, String>> {
        Box::pin(async move { self.ai.summary_outcome(job).await.map_err(|e| e.to_string()) })
    }

    fn content(&self, account: AccountId, job: JobId) -> BoxFuture<'_, Result<Option<CallbackContentFacts>, String>> {
        Box::pin(async move {
            let Some(row) = self.store.owned_job(account, job).await.map_err(|e| e.to_string())? else { return Ok(None) };
            let lines = self.store.lines(job).await.map_err(|e| e.to_string())?;
            // Without valid consent the summary is not readable and is omitted.
            let summary = self.ai.completion_summary(account, job).await.ok().flatten().map(|s| s.text);
            Ok(Some(CallbackContentFacts { job: row, lines, summary }))
        })
    }

    fn audio_url<'a>(&'a self, job: &'a Job) -> BoxFuture<'a, Option<String>> {
        Box::pin(self.jobs.callback_audio_url(job))
    }
}

#[cfg(test)]
mod tests;
