//! The shared effect driver: each core `AppEffect` becomes client work whose answer
//! returns to core as an `AppEvent`. Core fences every event by its epoch. Every
//! shell (the browser, the native FFI, the TUI, the CLI) wraps this one driver and
//! supplies only a [`Platform`]; no product rule lives here.
//!
//! The driver holds the fences only a shell can hold:
//! - a request runs only with the token of its epoch's own user, so a late
//!   effect never borrows a newer account's session;
//! - nothing reaches the clipboard, a file, the share sheet or the checkout form
//!   unless `AppDriver::is_current(epoch)` holds immediately before it;
//! - dispose aborts every watch, poll and timer, and late answers are dropped.
//!
//! Any request answered 401 also reports `Unauthorized`, and any 410
//! `account_deletion_in_progress` reports `AccountErased`, before its own result.
//!
//! State is borrowed only inside `update`/`view`; listeners and platform calls
//! always run after the borrow ends, so a listener may dispatch again. The driver
//! is single threaded (`Rc`): a native shell runs it on one thread.
mod platform;
mod tasks;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use billing_core::presentation::confirm::{ConfirmAnalytic, ConfirmEffect};
use billing_core::presentation::invoices::{DocumentPurpose, InvoicesEffect, InvoicesEvent};
use billing_core::presentation::purchase::{CheckoutStarted, CouponReply, PurchaseEffect, PurchaseEvent, PurchaseFailure};
use billing_core::presentation::subscription::{SubscriptionCancelled, SubscriptionEffect, SubscriptionEvent};
use billing_core::pricing::CouponLookup;
use serde_json::Value;
use transgate_core::ai::GenerationId;
use transgate_core::documents::InvoiceDocument;
use transgate_core::jobs::app::{DraftOptions, NewDraft, ResolveAnswer};
use transgate_core::presentation::ai::{RequestKey, WatchFailure, WatchKind, analysis, chat, consent};
use transgate_core::presentation::app::{
    AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEffect, AuthEvent, AuthFailure, BalanceView, Epoch, SessionFacts, Timer,
};
use transgate_core::presentation::failure::ApiFailure;
use transgate_core::presentation::layout::YouTubeLayout;
use transgate_core::presentation::{api_keys, data_management, feedback, job_list, support, transcript, upload_queue};
use transgate_core::{JobId, Micros};
use uuid::Uuid;

use crate::ai::{StartOutcome, WatchItem};
use crate::api::{ApiError, HttpApi};
use crate::attribution;
use crate::auth::{AuthError, AuthErrorKind, AuthOutcome, AuthSession, GoTrue};
use crate::checkout::checkout_form;
use crate::export::{Delivery, DocumentFonts, export_analysis, export_invoice};

pub use platform::{LocalFuture, Platform, ShareOutcome};
use tasks::{TaskKey, Tasks};

/// The chat download type (`useChatExport.ts`).
const CHAT_TEXT_TYPE: &str = "text/plain";

/// What a shell registers to be told of every new view.
pub type ViewListener = Rc<dyn Fn(&AppVm)>;

/// Core's `AppState` plus the effect driver for one shell.
pub struct AppDriver {
    state: RefCell<AppState>,
    listener: RefCell<Option<ViewListener>>,
    disposed: Cell<bool>,
    platform: Box<dyn Platform>,
    api: Arc<HttpApi>,
    auth: Arc<GoTrue>,
    tasks: Tasks,
    /// One parsed PDF writer per driver; public licensed assets, safe to keep across sessions.
    documents: DocumentFonts,
    /// The GoTrue user of the current session epoch. Requests of another epoch
    /// never borrow it, so a late request cannot run with a newer account's token.
    user: Cell<Option<(u64, Uuid)>>,
    /// Picked files the shell still holds for this driver.
    handles: RefCell<BTreeSet<u64>>,
    /// When the current UPL-008 draft poll began (ms since epoch).
    drafts_poll_started: Cell<f64>,
}

impl AppDriver {
    pub fn new(config: AppConfig, api: Arc<HttpApi>, auth: Arc<GoTrue>, documents: DocumentFonts, platform: Box<dyn Platform>) -> Rc<AppDriver> {
        Rc::new(AppDriver {
            state: RefCell::new(AppState::new(config)),
            listener: RefCell::new(None),
            disposed: Cell::new(false),
            platform,
            api,
            auth,
            tasks: Tasks::default(),
            documents,
            user: Cell::new(None),
            handles: RefCell::new(BTreeSet::new()),
            drafts_poll_started: Cell::new(0.0),
        })
    }

    /// The single view listener a shell fans out to its subscribers.
    pub fn set_listener(&self, listener: Option<ViewListener>) {
        *self.listener.borrow_mut() = listener;
    }

    pub fn disposed(&self) -> bool {
        self.disposed.get()
    }

    pub fn now(&self) -> Micros {
        Micros((self.platform.now_ms() * 1000.0) as i64)
    }

    pub fn epoch(&self) -> Epoch {
        self.state.borrow().epoch()
    }

    /// The delivery fence: the core epoch (session, consent, erasure) and disposal.
    pub fn is_current(&self, epoch: Epoch) -> bool {
        !self.disposed.get() && self.state.borrow().is_current(epoch)
    }

    /// Core owns invoice-context and preview-request invalidation; this adds the
    /// same session/disposal fence used by every private document delivery.
    pub fn is_current_invoice_preview(&self, epoch: Epoch, ticket: u64, payment_id: i64) -> bool {
        if self.disposed.get() {
            return false;
        }
        let state = self.state.borrow();
        state.is_current(epoch) && state.is_current_invoice_preview(ticket, payment_id)
    }

    /// Records that the current session epoch belongs to `user` (after core began it).
    fn bind_user(&self, user: Uuid) {
        self.user.set(Some((self.epoch().session, user)));
    }

    /// The user whose token may serve a request of `epoch`, if that epoch is still current.
    pub fn user(&self, epoch: Epoch) -> Option<Uuid> {
        match self.user.get() {
            Some((session, user)) if session == epoch.session && self.is_current(epoch) => Some(user),
            _ => None,
        }
    }

    /// Touch device (share sheet instead of a download, `useChatExport.ts`).
    fn touch(&self) -> bool {
        self.state.borrow().layouts().youtube == YouTubeLayout::Touch
    }

    fn locale(&self) -> String {
        match self.state.borrow().view(self.now()) {
            AppVm::Restoring { locale } | AppVm::Erased { locale, .. } | AppVm::CheckoutConfirm { locale, .. } => locale,
            AppVm::SignedOut(vm) => vm.locale,
            AppVm::SignedIn(vm) => vm.locale,
        }
    }

    pub fn current(&self) -> AppVm {
        self.state.borrow().view(self.now())
    }

    fn step(&self, event: AppEvent) -> (Vec<AppEffect>, AppVm) {
        let at = self.now();
        let effects = self.state.borrow_mut().update(event, at);
        let view = self.state.borrow().view(at);
        (effects, view)
    }

    fn notify(&self, view: &AppVm) {
        let listener = self.listener.borrow().clone();
        if let Some(listener) = listener {
            listener(view);
        }
    }

    fn run_all(self: &Rc<Self>, effects: Vec<AppEffect>) {
        for effect in effects {
            run(self, effect);
        }
    }

    /// Applies a user intent and returns the resulting view. Its effects start
    /// after this returns; their views reach the listener, not this call.
    pub fn dispatch(self: &Rc<Self>, intent: AppIntent) -> AppVm {
        if self.disposed.get() {
            return self.current();
        }
        let (effects, view) = self.step(AppEvent::User(intent));
        self.run_all(effects);
        view
    }

    /// Feeds an event (a shell's own, or an asynchronous result) into core and publishes the new view.
    pub fn apply(self: &Rc<Self>, event: AppEvent) {
        if self.disposed.get() {
            return;
        }
        // The account an accepted auth event names owns the session core holds
        // after it; bound before its effects run, so they carry its token.
        let signed_in = match &event {
            AppEvent::Auth {
                epoch,
                event:
                    AuthEvent::Restored { facts: Some(facts) }
                    | AuthEvent::SignedIn { facts }
                    | AuthEvent::PasswordRecovery { facts }
                    | AuthEvent::SessionAdopted { facts },
            } if epoch.session == self.epoch().session => Some(facts.user_id),
            _ => None,
        };
        let (effects, view) = self.step(event);
        if let Some(user) = signed_in {
            self.bind_user(user);
        }
        self.notify(&view);
        self.run_all(effects);
    }

    /// Runs `work` on the driver's thread, never to be aborted by key (only
    /// dispose stops it, by fencing its result).
    fn detached(&self, work: impl Future<Output = ()> + 'static) {
        self.platform.spawn(Box::pin(work));
    }

    /// Runs `work` under `key`, replacing a task already running under it.
    fn keyed(&self, key: TaskKey, work: impl Future<Output = ()> + 'static) {
        self.tasks.spawn(&*self.platform, key, work);
    }

    /// Another tab (or process) wrote or cleared the stored session.
    pub fn session_changed_elsewhere(self: &Rc<Self>) {
        let epoch = self.epoch();
        let event = match self.auth.current() {
            Some(session) => AuthEvent::SessionAdopted { facts: facts(&session) },
            // Only a driver that holds a session has one to lose.
            None if self.user(epoch).is_some() => AuthEvent::SignedOut,
            None => return,
        };
        self.apply(AppEvent::Auth { epoch, event });
    }

    /// Stops every watch, poll and timer, forgets file handles and drops the view
    /// listener. Late results are discarded. `false` when already disposed.
    pub fn dispose(&self) -> bool {
        if self.disposed.replace(true) {
            return false;
        }
        self.listener.borrow_mut().take();
        self.tasks.abort_all();
        let handles = std::mem::take(&mut *self.handles.borrow_mut());
        for handle in handles {
            self.platform.release(handle);
        }
        true
    }
}

fn run(inner: &Rc<AppDriver>, effect: AppEffect) {
    match effect {
        AppEffect::Auth { epoch, effect } => auth(inner, epoch, effect),
        AppEffect::LoadSession { epoch, locale } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.session(user, &locale).await },
            move |_, result| Some(AppEvent::SessionLoaded { epoch, result: result.map_err(|e| failure(&e)) }),
        ),
        AppEffect::Navigate { path, replace } => inner.platform.navigate(&path, replace),
        AppEffect::Leave { url } => inner.platform.leave(&url),
        AppEffect::LoadBalance { epoch } => fetch(
            inner,
            epoch,
            |api, user| async move { api.balance(user).await },
            move |_, result| {
                let result = result
                    .map(|b| BalanceView {
                        balance: b.minutes,
                        is_freemium: b.is_freemium,
                        special_client_alert: b.special_client_alert,
                    })
                    .map_err(|e| failure(&e));
                Some(AppEvent::Balance { epoch, result })
            },
        ),
        AppEffect::Timer { epoch, timer, after_ms } => {
            let key = match timer {
                Timer::Balance => "balance",
                Timer::PasswordUpdated => "password-updated",
                Timer::Confirmation { .. } => "confirmation",
                Timer::JobsPoll { .. } => "jobs.poll",
            };
            later(inner, key, after_ms, AppEvent::Timer { epoch, timer });
        }
        AppEffect::LoadJob { epoch, job } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.job_detail(user, job).await },
            move |_, result| Some(AppEvent::JobLoaded { epoch, job, result: result.map(|v| v.detail).map_err(|e| failure(&e)) }),
        ),
        AppEffect::LoadQueue { epoch } => fetch(
            inner,
            epoch,
            |api, user| async move {
                let (drafts, usage) = futures_util::join!(api.drafts(user), api.language_usage(user));
                // The usage only orders the language menus; drafts restore without it.
                Ok::<_, ApiError>((drafts?, usage.unwrap_or_default()))
            },
            move |_, result| {
                let (drafts, usage) = result.ok()?;
                let event = upload_queue::Event::Restored {
                    drafts,
                    preferred: usage.languages.into_iter().map(|(l, _)| l).collect(),
                    preferred_targets: usage.targets.into_iter().map(|(l, _)| l).collect(),
                };
                Some(AppEvent::Upload { epoch, event })
            },
        ),
        AppEffect::LoadSettings { epoch } => fetch(
            inner,
            epoch,
            |api, user| async move { api.settings(user).await },
            move |_, result| Some(AppEvent::ManageData { epoch, event: data_management::Event::Loaded { settings: result.ok()? } }),
        ),
        AppEffect::LoadCatalog { epoch } => fetch(
            inner,
            epoch,
            |api, user| async move { api.catalog(user).await },
            move |_, result| Some(AppEvent::Catalog { epoch, result: result.map_err(|e| failure(&e)) }),
        ),
        AppEffect::Upload { epoch, effect } => upload(inner, epoch, effect),
        AppEffect::Transcript { epoch, job, effect } => transcript(inner, epoch, job, effect),
        AppEffect::Jobs { epoch, effect } => jobs(inner, epoch, effect),
        AppEffect::ManageData { epoch, effect } => manage_data(inner, epoch, effect),
        AppEffect::Feedback { epoch, effect: feedback::Effect::Submit { job, rating, comment } } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.submit_feedback(user, job, rating, &comment).await },
            move |_, result| {
                let event = feedback::Event::Answered { result: result.map(drop).map_err(|e| failure(&e)) };
                Some(AppEvent::Feedback { epoch, event })
            },
        ),
        AppEffect::Support { epoch, effect: support::Effect::Send { message, job } } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.request_support(user, &message, job).await },
            move |_, result| {
                let event = support::Event::Answered { result: result.map(drop).map_err(|e| failure(&e)) };
                Some(AppEvent::Support { epoch, event })
            },
        ),
        AppEffect::Consent { epoch, effect } => consent(inner, epoch, effect),
        AppEffect::Analysis { epoch, job, effect } => analysis(inner, epoch, job, effect),
        AppEffect::Chat { epoch, job, effect } => chat(inner, epoch, job, effect),
        AppEffect::Purchase { epoch, effect } => purchase(inner, epoch, effect),
        AppEffect::Subscription { epoch, effect } => subscription(inner, epoch, effect),
        AppEffect::Invoices { epoch, ticket, effect } => invoices(inner, epoch, ticket, *effect),
        AppEffect::ApiKeys { epoch, effect } => api_keys(inner, epoch, effect),
        AppEffect::ReadConfirmation { epoch, sequence } => {
            if inner.is_current(epoch) {
                inner.apply(AppEvent::ConfirmationLoaded { epoch, sequence, payload: inner.platform.confirmation() });
            }
        }
        AppEffect::Confirmation { epoch, effect } => confirmation(inner, epoch, effect),
        AppEffect::Telemetry { event } => inner.platform.analytics(&event),
        AppEffect::TelemetryReset => inner.platform.telemetry_reset(),
    }
}

// ---- plumbing ----------------------------------------------------------------------------

/// Runs one request with the token of `epoch`'s user and applies what `deliver`
/// builds from the answer. An effect whose epoch already ended sends nothing.
fn fetch<T: 'static, Fut>(
    inner: &Rc<AppDriver>,
    epoch: Epoch,
    call: impl FnOnce(Arc<HttpApi>, Uuid) -> Fut + 'static,
    deliver: impl FnOnce(&Rc<AppDriver>, Result<T, ApiError>) -> Option<AppEvent> + 'static,
) where
    Fut: Future<Output = Result<T, ApiError>> + 'static,
{
    let Some(user) = inner.user(epoch) else { return };
    let this = inner.clone();
    inner.detached(async move {
        let result = call(this.api.clone(), user).await;
        if let Err(error) = &result {
            escalate(&this, epoch, error);
        }
        if let Some(event) = deliver(&this, result) {
            this.apply(event);
        }
    });
}

/// Applies `event` after `after_ms`, replacing an earlier timer under `key`.
fn later(inner: &Rc<AppDriver>, key: &'static str, after_ms: u64, event: AppEvent) {
    let this = inner.clone();
    inner.keyed(TaskKey::Timer(key), async move {
        this.platform.sleep(after_ms).await;
        this.apply(event);
    });
}

fn escalate(inner: &Rc<AppDriver>, epoch: Epoch, error: &ApiError) {
    match error {
        // The stored session is gone or belongs to someone else now.
        ApiError::SignedOut => inner.session_changed_elsewhere(),
        ApiError::Http { status, code, .. } => escalate_status(inner, epoch, *status, code.as_deref()),
        ApiError::Network(_) | ApiError::Decode(_) => {}
    }
}

fn escalate_status(inner: &Rc<AppDriver>, epoch: Epoch, status: u16, code: Option<&str>) {
    let failure = ApiFailure::http(status, code);
    if failure.is_unauthorized() {
        inner.apply(AppEvent::Unauthorized { epoch });
    } else if failure.is_account_erased() {
        inner.apply(AppEvent::AccountErased { epoch });
    }
}

/// `SignedOut` sent nothing; to a leaf it is the 401 it would have received.
fn failure(error: &ApiError) -> ApiFailure {
    error.failure().unwrap_or_else(|| ApiFailure::http(401, None))
}

/// A refused write is answered by rereading the stored truth (agreed with
/// p1X), unless the session itself failed: `escalate` already reported that,
/// and a reread would be refused the same way.
fn rereads(error: &ApiError) -> bool {
    error.failure().is_some_and(|f| !f.is_unauthorized() && !f.is_account_erased())
}

/// Writes the clipboard only while `epoch` is current; `answer` sees whether it worked.
fn copy(inner: &Rc<AppDriver>, epoch: Epoch, text: String, html: Option<String>, answer: impl FnOnce(bool) -> Option<AppEvent> + 'static) {
    if !inner.is_current(epoch) {
        return;
    }
    let this = inner.clone();
    inner.detached(async move {
        let ok = this.platform.copy(&text, html.as_deref()).await;
        if let Some(event) = answer(ok) {
            this.apply(event);
        }
    });
}

/// `POST /api/jobs/{id}/export`; the server's ZIP is saved only while `epoch` is current.
fn export_zip(inner: &Rc<AppDriver>, epoch: Epoch, job: JobId, request: transgate_core::jobs::app::ExportRequest, finished: Option<AppEvent>) {
    fetch(
        inner,
        epoch,
        move |api, user| async move { api.export(user, job, &request).await },
        move |this, result| {
            if let Ok(download) = result
                && this.is_current(epoch)
            {
                this.platform.download(&download.bytes, &download.filename, &download.content_type);
            }
            finished
        },
    );
}

// ---- auth ---------------------------------------------------------------------------------

fn facts(session: &AuthSession) -> SessionFacts {
    SessionFacts { user_id: session.user_id, email: session.email.clone(), expires_at: Micros(session.expires_at * 1_000_000) }
}

fn auth_failure(kind: AuthErrorKind) -> AuthFailure {
    match kind {
        AuthErrorKind::InvalidCredentials => AuthFailure::InvalidCredentials,
        AuthErrorKind::EmailNotConfirmed => AuthFailure::EmailNotConfirmed,
        AuthErrorKind::RateLimited => AuthFailure::RateLimited,
        AuthErrorKind::WeakPassword => AuthFailure::WeakPassword,
        AuthErrorKind::UserExists => AuthFailure::UserExists,
        AuthErrorKind::InvalidOrExpiredCode => AuthFailure::InvalidOrExpiredCode,
        AuthErrorKind::CaptchaFailed => AuthFailure::CaptchaFailed,
        // The callback or recovery session is gone: the link must be requested again.
        AuthErrorKind::SessionMissing => AuthFailure::InvalidOrExpiredCode,
        AuthErrorKind::Network => AuthFailure::Network,
        AuthErrorKind::Unavailable | AuthErrorKind::Rejected => AuthFailure::Unavailable,
    }
}

fn failed(error: &AuthError) -> AuthEvent {
    AuthEvent::Failed { kind: auth_failure(error.kind) }
}

fn outcome(result: Result<AuthOutcome, AuthError>) -> AuthEvent {
    match result {
        Ok(AuthOutcome::SignedIn(session)) => AuthEvent::SignedIn { facts: facts(&session) },
        Ok(AuthOutcome::PasswordRecovery(session)) => AuthEvent::PasswordRecovery { facts: facts(&session) },
        Ok(AuthOutcome::PasswordUpdated(_)) => AuthEvent::PasswordUpdated,
        Ok(AuthOutcome::EmailSent) => AuthEvent::EmailSent,
        Ok(AuthOutcome::SignedOut) => AuthEvent::SignedOut,
        Err(error) => failed(&error),
    }
}

/// The stored session, refreshed when due; a rejected refresh restores nobody.
async fn restore(gotrue: &GoTrue) -> AuthEvent {
    let Some(stored) = gotrue.current() else { return AuthEvent::Restored { facts: None } };
    match gotrue.access_token(stored.user_id).await {
        Ok(Some(_)) => AuthEvent::Restored { facts: gotrue.current().filter(|s| s.user_id == stored.user_id).map(|s| facts(&s)) },
        Ok(None) => AuthEvent::Restored { facts: None },
        Err(error) => failed(&error),
    }
}

fn auth(inner: &Rc<AppDriver>, epoch: Epoch, effect: AuthEffect) {
    let gotrue = inner.auth.clone();
    if let AuthEffect::GoogleRedirect { redirect } = &effect {
        inner.platform.leave(&gotrue.google_authorize_url(redirect));
        return;
    }
    // A refresh renews the session of this epoch's user, never whoever signed in since.
    let user = inner.user(epoch);
    let this = inner.clone();
    inner.detached(async move {
        let event = match effect {
            AuthEffect::RestoreSession => restore(&gotrue).await,
            AuthEffect::PasswordSignIn { email, password, captcha } => {
                outcome(gotrue.sign_in_password(&email, &password, captcha.as_deref()).await)
            }
            AuthEffect::SignUp { email, password, redirect, captcha } => {
                outcome(gotrue.sign_up(&email, &password, &redirect, captcha.as_deref()).await)
            }
            AuthEffect::MagicLink { email, redirect, captcha } => outcome(gotrue.magic_link(&email, &redirect, captcha.as_deref()).await),
            AuthEffect::Recover { email, redirect, captcha } => outcome(gotrue.recover(&email, &redirect, captcha.as_deref()).await),
            // Answered synchronously above: the page leaves.
            AuthEffect::GoogleRedirect { .. } => return,
            AuthEffect::ExchangeCallback { code } => outcome(gotrue.exchange_code(&code).await),
            AuthEffect::UpdatePassword { password } => outcome(gotrue.update_password(&password).await),
            AuthEffect::Refresh => match user {
                Some(user) => match gotrue.renew(user).await {
                    Ok(Some(session)) => AuthEvent::SessionAdopted { facts: facts(&session) },
                    Ok(None) => AuthEvent::SignedOut,
                    Err(error) => failed(&error),
                },
                None => AuthEvent::SignedOut,
            },
            AuthEffect::SignOut => outcome(gotrue.sign_out().await),
        };
        this.apply(AppEvent::Auth { epoch, event });
    });
}

// ---- upload -------------------------------------------------------------------------------

fn upload(inner: &Rc<AppDriver>, epoch: Epoch, effect: upload_queue::Effect) {
    use upload_queue::{Effect, Event};
    let up = move |event: Event| AppEvent::Upload { epoch, event };
    match effect {
        Effect::MeasureDuration { file, handle } => {
            inner.handles.borrow_mut().insert(handle);
            let this = inner.clone();
            inner.detached(async move {
                let seconds = this.platform.measure(handle).await;
                this.apply(up(Event::Measured { file, seconds }));
            });
        }
        Effect::CreateDraft { file, name, size, language, speakers, duration } => fetch(
            inner,
            epoch,
            move |api, user| async move {
                let draft = NewDraft { name, size_bytes: Some(size), language, speakers, duration_minutes: duration };
                api.create_draft(user, &draft).await
            },
            move |_, result| {
                Some(up(match result {
                    Ok(job) => Event::DraftCreated { file, job },
                    Err(_) => Event::DraftFailed { file },
                }))
            },
        ),
        Effect::SetOptions { jobs, language, translate_to, speakers } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.set_draft_options(user, &jobs, &DraftOptions { language, translate_to, speakers }).await },
            move |this, result| {
                if let Err(error) = &result
                    && rereads(error)
                {
                    fetch(
                        this,
                        epoch,
                        |api, user| async move { api.drafts(user).await },
                        move |_, drafts| Some(up(Event::DraftsReloaded { drafts: drafts.ok()?, elapsed_ms: 0 })),
                    );
                }
                None
            },
        ),
        Effect::Upload { file, handle, job, name } => {
            inner.handles.borrow_mut().insert(handle);
            let Some(user) = inner.user(epoch) else { return };
            let this = inner.clone();
            inner.detached(async move {
                let ok = match this.api.init_upload(user, job, &name).await {
                    Ok(url) => {
                        let tick = this.clone();
                        this.platform.upload(handle, &url, Box::new(move |progress| tick.apply(up(Event::UploadProgress { file, progress })))).await
                    }
                    Err(error) => {
                        escalate(&this, epoch, &error);
                        false
                    }
                };
                this.apply(up(Event::UploadFinished { file, ok }));
            });
        }
        Effect::DeleteDraft { job } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.remove_draft(user, job).await },
            move |_, result| result.err().filter(rereads).map(|_| AppEvent::Jobs { epoch, event: job_list::Event::Refresh }),
        ),
        Effect::Start { jobs, language, translate_to } => {
            let locale = inner.locale();
            fetch(
                inner,
                epoch,
                move |api, user| async move {
                    // JOB-005: the options are rewritten once more right before the start.
                    let options = DraftOptions { language: Some(language), translate_to: Some(translate_to), speakers: None };
                    api.set_draft_options(user, &jobs, &options).await?;
                    api.start_jobs(user, &jobs, Some(locale.as_str())).await?;
                    Ok(jobs.first().copied())
                },
                move |_, result| {
                    Some(up(match result {
                        Ok(Some(first_job)) => Event::StartSucceeded { first_job },
                        Ok(None) => Event::StartFailed { balance: false },
                        Err(error) => Event::StartFailed { balance: insufficient_balance(&error) },
                    }))
                },
            )
        }
        Effect::ResolveYouTube { job } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.youtube_resolve(user, job).await },
            move |_, result| {
                Some(up(match result {
                    Ok(ResolveAnswer::Completed { job_id }) => Event::YouTubeResolved { job: job_id, completed: true, code: None },
                    Ok(ResolveAnswer::Failed { job_id, code, .. }) => Event::YouTubeResolved { job: job_id, completed: false, code: Some(code) },
                    Err(_) => Event::YouTubeResolveCrashed,
                }))
            },
        ),
        Effect::AddYouTube { url, language } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.youtube_add(user, &url, language).await },
            move |_, result| {
                Some(up(match result {
                    // Core reads a non-positive duration as unknown.
                    Ok((job, metadata)) => Event::YouTubeAdded {
                        job,
                        title: metadata.title,
                        duration_seconds: metadata.duration_seconds.unwrap_or(0.0),
                        detected_language: metadata.language,
                    },
                    Err(error) => Event::YouTubeAddFailed { failure: failure(&error) },
                }))
            },
        ),
        // Core issues this once, when a UPL-008 poll begins.
        Effect::ReloadDrafts => {
            inner.drafts_poll_started.set(inner.platform.now_ms());
            reload_drafts(inner, epoch);
        }
        Effect::ScheduleReload { after_ms } => {
            let this = inner.clone();
            inner.keyed(TaskKey::Timer("drafts"), async move {
                this.platform.sleep(after_ms).await;
                reload_drafts(&this, epoch);
            });
        }
        Effect::Telemetry { event } => inner.platform.analytics(&event),
        // Core lifts these into `AppEffect::Navigate`; the leaf effect never reaches a shell.
        Effect::Navigate { .. } => {}
    }
}

fn insufficient_balance(error: &ApiError) -> bool {
    matches!(error, ApiError::Http { status: 402, .. }) || matches!(error, ApiError::Http { code: Some(code), .. } if code == "insufficient_balance")
}

/// One UPL-008 tick. A failed reread is a tick that learned nothing, so the
/// poll and its timeout keep running as the source's interval did.
fn reload_drafts(inner: &Rc<AppDriver>, epoch: Epoch) {
    let started = inner.drafts_poll_started.get();
    fetch(
        inner,
        epoch,
        |api, user| async move { api.drafts(user).await },
        move |this, result| {
            let elapsed_ms = (this.platform.now_ms() - started).max(0.0) as u64;
            let event = upload_queue::Event::DraftsReloaded { drafts: result.unwrap_or_default(), elapsed_ms };
            Some(AppEvent::Upload { epoch, event })
        },
    );
}

// ---- transcript and job list ----------------------------------------------------------------

fn transcript(inner: &Rc<AppDriver>, epoch: Epoch, job: JobId, effect: transcript::Effect) {
    use transcript::{Effect, Event};
    let tr = move |event: Event| AppEvent::Transcript { epoch, job, event };
    match effect {
        Effect::SaveEdit { edit } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.edit_line(user, &edit).await },
            move |this, result| {
                // Lines merge by id, so the stored text replaces the refused edit.
                if let Err(error) = &result
                    && rereads(error)
                {
                    fetch(
                        this,
                        epoch,
                        move |api, user| async move { api.job_lines(user, job).await },
                        move |_, lines| Some(tr(Event::LinesArrived { lines: lines.ok()? })),
                    );
                }
                None
            },
        ),
        Effect::PollJob { after_ms } => {
            let this = inner.clone();
            inner.keyed(TaskKey::Timer("transcript.poll"), async move {
                this.platform.sleep(after_ms).await;
                fetch(
                    &this,
                    epoch,
                    move |api, user| async move { api.job_detail(user, job).await },
                    move |this, result| match result {
                        Ok(view) => Some(tr(Event::JobPolled { job: view.detail.job })),
                        // The source's interval kept polling through a failed read.
                        Err(_) => {
                            run(this, AppEffect::Transcript { epoch, job, effect: Effect::PollJob { after_ms } });
                            None
                        }
                    },
                );
            });
        }
        Effect::LoadLines { after_ms } => {
            let this = inner.clone();
            inner.keyed(TaskKey::Timer("transcript.lines"), async move {
                this.platform.sleep(after_ms).await;
                fetch(
                    &this,
                    epoch,
                    move |api, user| async move { api.job_lines(user, job).await },
                    move |_, result| {
                        Some(tr(match result {
                            Ok(lines) => Event::LinesArrived { lines },
                            Err(error) => Event::LinesFailed { detail: error.to_string() },
                        }))
                    },
                );
            });
        }
        Effect::PausePlayer => {
            if inner.is_current(epoch) {
                let playing = inner.platform.pause_player();
                inner.apply(tr(Event::PlayerState { playing }));
            }
        }
        Effect::ResumePlayer => {
            if inner.is_current(epoch) {
                let this = inner.clone();
                inner.detached(async move { this.platform.resume_player().await });
            }
        }
        Effect::Seek { seconds } => {
            if inner.is_current(epoch) {
                inner.platform.seek_player(seconds);
            }
        }
        Effect::Export { job, request } => export_zip(inner, epoch, job, request, None),
        Effect::CopyText { text } => copy(inner, epoch, text, None, |_| None),
        Effect::LoadMediaUrl { job: media, file_name } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.streaming_url(user, media, &file_name).await },
            move |_, result| Some(tr(Event::MediaUrlLoaded { job: media, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::ScheduleMediaUrl { job: media, after_ms } => later(inner, "transcript.media", after_ms, tr(Event::MediaUrlDue { job: media })),
        Effect::ScheduleNoAudioAlert { after_ms } => later(inner, "transcript.no-audio", after_ms, tr(Event::NoAudioAlertExpired)),
    }
}

fn jobs(inner: &Rc<AppDriver>, epoch: Epoch, effect: job_list::Effect) {
    use job_list::{Effect, Event};
    let list = move |event: Event| AppEvent::Jobs { epoch, event };
    match effect {
        Effect::LoadPage { before } => {
            let calendar = inner.platform.calendar();
            fetch(
                inner,
                epoch,
                move |api, user| async move { api.list_jobs(user, before, calendar).await },
                move |_, result| {
                    Some(list(match result {
                        Ok(page) => Event::PageLoaded { jobs: page.jobs, next_cursor: page.next_cursor, replace: before.is_none() },
                        Err(error) => Event::LoadFailed { detail: Some(error.to_string()) },
                    }))
                },
            )
        }
        // D11: the owned list newest first down to the oldest loaded row. A failed
        // poll keeps every row; the next tick tries again.
        Effect::PollPage { through } => {
            let calendar = inner.platform.calendar();
            fetch(
                inner,
                epoch,
                move |api, user| async move { api.poll_jobs(user, through, calendar).await },
                move |_, result| {
                    Some(list(match result {
                        Ok(jobs) => Event::Polled { jobs },
                        Err(_) => Event::PollFailed,
                    }))
                },
            )
        }
        // The row already left the list; a refused delete reloads the truth.
        Effect::DeleteJob { job } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.delete_job(user, job).await },
            move |_, result| result.err().filter(rereads).map(|_| list(Event::Refresh)),
        ),
        Effect::Export { job, request } => export_zip(inner, epoch, job, request, Some(list(Event::DownloadFinished { job }))),
    }
}

fn manage_data(inner: &Rc<AppDriver>, epoch: Epoch, effect: data_management::Effect) {
    use data_management::{Effect, Event};
    let manage = move |event: Event| AppEvent::ManageData { epoch, event };
    // A refused write answers `SaveFailed`, and core returns the switch to its stored
    // value. A session failure (401/410) was already reported by `fetch`'s escalation.
    let saved = move |_: &Rc<AppDriver>, result: Result<_, ApiError>| match result {
        Ok(settings) => Some(manage(Event::Saved { settings })),
        Err(error) if rereads(&error) => Some(manage(Event::SaveFailed { failure: failure(&error) })),
        Err(_) => None,
    };
    match effect {
        Effect::SetAutoDelete { value } => {
            fetch(inner, epoch, move |api, user| async move { api.update_settings(user, Some(value), None).await }, saved)
        }
        Effect::SetDeletedMonthly { value } => {
            fetch(inner, epoch, move |api, user| async move { api.update_settings(user, None, Some(value)).await }, saved)
        }
        // The list shows what is left after the soft delete.
        Effect::DeleteAllJobs => fetch(
            inner,
            epoch,
            |api, user| async move { api.delete_all(user).await },
            move |_, result| {
                let refresh = match &result {
                    Ok(_) => true,
                    Err(error) => rereads(error),
                };
                refresh.then_some(AppEvent::Jobs { epoch, event: job_list::Event::Refresh })
            },
        ),
        Effect::RequestErasure => fetch(
            inner,
            epoch,
            |api, user| async move { api.request_erasure(user).await },
            move |_, result| {
                Some(manage(match result {
                    Ok(view) => Event::ErasureState { view },
                    Err(error) => Event::ErasureFailed { failure: failure(&error) },
                }))
            },
        ),
        // Core lifts this into its sign-out sequence; the leaf effect never reaches a shell.
        Effect::SignOut => {}
    }
}

// ---- AI -----------------------------------------------------------------------------------

/// How one start answer reaches its panel.
struct Started<F: Fn(StartAnswer) -> AppEvent> {
    event: F,
}

enum StartAnswer {
    Accepted { key: RequestKey, generation: GenerationId },
    Rejected { key: RequestKey, failure: ApiFailure, body: Option<transgate_core::presentation::ai::StartRejection> },
    Unknown { key: RequestKey },
}

impl<F: Fn(StartAnswer) -> AppEvent> Started<F> {
    fn answer(&self, inner: &Rc<AppDriver>, epoch: Epoch, key: RequestKey, outcome: StartOutcome) -> AppEvent {
        (self.event)(match outcome {
            StartOutcome::Accepted(generation) => StartAnswer::Accepted { key, generation },
            StartOutcome::Rejected { status, rejection } => {
                let code = (!rejection.code.is_empty()).then_some(rejection.code.as_str());
                escalate_status(inner, epoch, status, code);
                StartAnswer::Rejected { key, failure: ApiFailure::http(status, code), body: Some(rejection) }
            }
            // Nothing was sent, so nothing runs for this key.
            StartOutcome::SignedOut => {
                inner.session_changed_elsewhere();
                StartAnswer::Rejected { key, failure: ApiFailure::http(401, None), body: None }
            }
            StartOutcome::Unknown => StartAnswer::Unknown { key },
        })
    }
}

/// Starts (or replays) one start POST for the epoch's user.
fn start<F: Fn(StartAnswer) -> AppEvent + 'static, Fut: Future<Output = StartOutcome> + 'static>(
    inner: &Rc<AppDriver>,
    epoch: Epoch,
    key: RequestKey,
    call: impl FnOnce(Arc<HttpApi>, Uuid) -> Fut + 'static,
    started: Started<F>,
) {
    let Some(user) = inner.user(epoch) else { return };
    let this = inner.clone();
    inner.detached(async move {
        let outcome = call(this.api.clone(), user).await;
        let event = started.answer(&this, epoch, key, outcome);
        this.apply(event);
    });
}

/// Attaches one generation's watch; a newer attachment of the same generation replaces it.
fn watch(
    inner: &Rc<AppDriver>,
    epoch: Epoch,
    generation: GenerationId,
    kind: WatchKind,
    after: Option<u64>,
    item: impl Fn(WatchItem) -> Option<AppEvent> + 'static,
    ended: impl FnOnce(Result<(), WatchFailure>) -> AppEvent + 'static,
) {
    let Some(user) = inner.user(epoch) else { return };
    let this = inner.clone();
    inner.keyed(TaskKey::Watch(generation.0), async move {
        let result = this
            .api
            .watch(user, generation, kind, after, |watched| {
                if let Some(event) = item(watched) {
                    this.apply(event);
                }
            })
            .await;
        if let Err(WatchFailure::Http { failure }) = &result {
            escalate_status(&this, epoch, failure.status.unwrap_or_default(), failure.code.as_deref());
        }
        this.apply(ended(result));
    });
}

fn consent(inner: &Rc<AppDriver>, epoch: Epoch, effect: consent::Effect) {
    use consent::{Effect, Event};
    let answer = move |event: Event| AppEvent::Consent { epoch, event };
    match effect {
        Effect::LoadConsent => fetch(
            inner,
            epoch,
            |api, user| async move { api.consent(user).await },
            move |_, result| Some(answer(Event::Loaded { result: result.map_err(|e| failure(&e)) })),
        ),
        // POST {hasConsented}: `false` is the nondestructive learn-more refusal (D22).
        Effect::RecordConsent { granted } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.record_consent(user, granted).await },
            move |_, result| Some(answer(Event::DecisionSaved { result: result.map_err(|e| failure(&e)) })),
        ),
        // DELETE: revoke plus the account-side purge of saved AI data (D22, P4 D3).
        Effect::RevokeConsent => fetch(
            inner,
            epoch,
            |api, user| async move { api.revoke_consent(user).await },
            move |_, result| Some(answer(Event::DecisionSaved { result: result.map_err(|e| failure(&e)) })),
        ),
    }
}

fn analysis(inner: &Rc<AppDriver>, epoch: Epoch, job: JobId, effect: analysis::Effect) {
    use analysis::{Effect, Event};
    let panel = move |event: Event| AppEvent::Analysis { epoch, job, event };
    match effect {
        Effect::LoadAnalysis { job: of } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.analysis(user, of).await },
            move |_, result| Some(panel(Event::AnalysisLoaded { job: of, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::StartAnalysis { job: of, key } => start(
            inner,
            epoch,
            key,
            move |api, user| async move { api.start_analysis(user, of, key).await },
            Started {
                event: move |answer: StartAnswer| {
                    panel(match answer {
                        StartAnswer::Accepted { key, generation } => Event::StartAccepted { key, generation },
                        StartAnswer::Rejected { key, failure, body } => Event::StartRejected { key, failure, body },
                        StartAnswer::Unknown { key } => Event::StartUnknown { key },
                    })
                },
            },
        ),
        Effect::Watch { generation, kind, after } => watch(
            inner,
            epoch,
            generation,
            kind,
            after,
            move |item| match item {
                WatchItem::Analysis(wire) => Some(panel(Event::WatchAnalysis { generation, wire })),
                WatchItem::Text(_) => None,
            },
            move |result| panel(Event::WatchEnded { generation, result }),
        ),
        Effect::Detach { generation } => inner.tasks.abort(&TaskKey::Watch(generation.0)),
        Effect::Copy { text, html } => copy(inner, epoch, text, Some(html), |_| None),
        Effect::Export { document, format } => {
            let this = inner.clone();
            inner.detached(async move {
                let delivery = export_analysis(
                    &this.documents,
                    &document,
                    format,
                    || this.is_current(epoch),
                    |bytes| this.platform.download(&bytes.bytes, &bytes.filename, &bytes.content_type),
                )
                .await;
                let result = match delivery {
                    Delivery::Delivered(result) => result,
                    Delivery::Failed(failure) => Err(failure),
                    // The capture belongs to an ended session or consent: no result applies.
                    Delivery::Stale => return,
                };
                this.apply(panel(Event::ExportFinished { format, result }));
            });
        }
    }
}

fn chat(inner: &Rc<AppDriver>, epoch: Epoch, job: JobId, effect: chat::Effect) {
    use chat::{Effect, Event};
    let panel = move |event: Event| AppEvent::Chat { epoch, job, event };
    let started = move || Started {
        event: move |answer: StartAnswer| {
            panel(match answer {
                StartAnswer::Accepted { key, generation } => Event::StartAccepted { key, generation },
                StartAnswer::Rejected { key, failure, body } => Event::StartRejected { key, failure, body },
                StartAnswer::Unknown { key } => Event::StartUnknown { key },
            })
        },
    };
    match effect {
        Effect::LoadHistory { job: of } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.chat_history(user, of).await },
            move |_, result| Some(panel(Event::HistoryLoaded { job: of, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::LoadLimits { job: of } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.chat_limits(user, of).await },
            move |_, result| Some(panel(Event::LimitsLoaded { job: of, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::StartChat { job: of, key, message, history } => start(
            inner,
            epoch,
            key,
            move |api, user| async move { api.start_chat(user, of, key, &message, &history).await },
            started(),
        ),
        Effect::RetryChat { generation, key } => start(
            inner,
            epoch,
            key,
            move |api, user| async move { api.retry_generation(user, generation, key).await },
            started(),
        ),
        Effect::Watch { generation, kind, after } => watch(
            inner,
            epoch,
            generation,
            kind,
            after,
            move |item| match item {
                WatchItem::Text(frame) => Some(panel(Event::WatchFrame { generation, frame })),
                WatchItem::Analysis(_) => None,
            },
            move |result| panel(Event::WatchEnded { generation, result }),
        ),
        Effect::Detach { generation } => inner.tasks.abort(&TaskKey::Watch(generation.0)),
        Effect::LoadGeneration { generation } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.generation(user, generation).await },
            move |_, result| Some(panel(Event::GenerationLoaded { generation, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::CopyText { text } => copy(inner, epoch, text, None, |_| None),
        Effect::CopyChat { text } => copy(inner, epoch, text, None, move |ok| Some(panel(Event::ChatCopied { ok }))),
        Effect::DownloadText { filename, text } => save_text(inner, epoch, filename, text),
        Effect::Telemetry { event } => inner.platform.analytics(&event),
    }
}

/// `useChatExport.ts`: the share sheet on touch devices (a dismissal ends it),
/// otherwise, or when files cannot be shared, a download.
fn save_text(inner: &Rc<AppDriver>, epoch: Epoch, filename: String, text: String) {
    if !inner.is_current(epoch) {
        return;
    }
    let bytes = text.into_bytes();
    if !inner.touch() {
        inner.platform.download(&bytes, &filename, CHAT_TEXT_TYPE);
        return;
    }
    let this = inner.clone();
    inner.detached(async move {
        if this.platform.share(&bytes, &filename, CHAT_TEXT_TYPE).await == ShareOutcome::Unsupported && this.is_current(epoch) {
            this.platform.download(&bytes, &filename, CHAT_TEXT_TYPE);
        }
    });
}

// ---- billing ------------------------------------------------------------------------------

/// The stored first touch plus device type and browser (`utmCapture.ts`, `deviceDetection.ts`).
fn checkout_attribution(inner: &AppDriver) -> serde_json::Map<String, Value> {
    attribution::checkout_attribution(inner.platform.stored_attribution().as_deref(), &inner.platform.user_agent())
}

fn purchase(inner: &Rc<AppDriver>, epoch: Epoch, effect: PurchaseEffect) {
    let answer = move |event: PurchaseEvent| AppEvent::Purchase { epoch, event };
    match effect {
        PurchaseEffect::FetchQuote { request, order } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.quote(user, &order).await },
            move |_, result| Some(answer(PurchaseEvent::QuoteAnswered { request, result: result.map_err(|e| e.purchase_failure()) })),
        ),
        PurchaseEffect::FetchCoupon(id) => fetch(
            inner,
            epoch,
            move |api, user| async move { api.coupon(user, &id).await },
            move |_, result| Some(answer(PurchaseEvent::CouponAnswered(result.map_or(CouponLookup::Failed, CouponReply::lookup)))),
        ),
        PurchaseEffect::SubmitCheckout(order) => submit_checkout(inner, epoch, order),
        PurchaseEffect::Analytics(started) => {
            let mut properties = checkout_attribution(inner);
            if let Value::Object(own) = started.properties() {
                properties.extend(own);
            }
            inner.platform.track_value(CheckoutStarted::EVENT, &Value::Object(properties));
        }
    }
}

/// D17: one hidden same-origin form POST to `/checkout`; the page leaves for the
/// checkout document. The token is read for this epoch's user only, and the form
/// is posted only while the epoch is still current.
fn submit_checkout(inner: &Rc<AppDriver>, epoch: Epoch, order: billing_core::presentation::purchase::OrderBody) {
    let not_submitted = move |failure: PurchaseFailure| AppEvent::Purchase { epoch, event: PurchaseEvent::CheckoutNotSubmitted(failure) };
    let Some(user) = inner.user(epoch) else { return };
    let this = inner.clone();
    inner.detached(async move {
        let token = this.auth.access_token(user).await;
        if !this.is_current(epoch) {
            return;
        }
        let token = match token {
            Ok(Some(token)) => token,
            Ok(None) => return this.apply(not_submitted(PurchaseFailure::Unauthenticated)),
            Err(_) => return this.apply(not_submitted(PurchaseFailure::Unavailable)),
        };
        match checkout_form(&order, checkout_attribution(&this), &token) {
            Ok(form) => {
                if !this.is_current(epoch) {
                    return;
                }
                if !this.platform.submit_checkout(&form) {
                    this.apply(not_submitted(PurchaseFailure::Unavailable));
                }
            }
            Err(failure) => this.apply(not_submitted(failure)),
        }
    });
}

fn subscription(inner: &Rc<AppDriver>, epoch: Epoch, effect: SubscriptionEffect) {
    let answer = move |event: SubscriptionEvent| AppEvent::Subscription { epoch, event };
    match effect {
        SubscriptionEffect::Load => fetch(
            inner,
            epoch,
            |api, user| async move { api.subscription(user).await },
            move |_, result| Some(answer(SubscriptionEvent::Loaded(result.map_err(|e| e.purchase_failure())))),
        ),
        SubscriptionEffect::Cancel { payment_id } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.cancel_subscription(user, payment_id).await },
            move |_, result| Some(answer(SubscriptionEvent::CancelAnswered(result.map_err(|e| e.purchase_failure())))),
        ),
        SubscriptionEffect::Analytics(cancelled) => inner.platform.track_value(SubscriptionCancelled::EVENT, &cancelled.properties()),
        // Core lifts this into opening the purchase modal; the leaf effect never reaches a shell.
        SubscriptionEffect::OpenPurchase => {}
    }
}

/// Core has already decided which confirmation intents apply and fenced repeat
/// payloads. D12 gates PostHog in Platform.track and omits the source GA purchase.
fn confirmation(inner: &Rc<AppDriver>, epoch: Epoch, effect: ConfirmEffect) {
    match effect {
        ConfirmEffect::Track(events) => {
            for event in events {
                if !inner.is_current(epoch) {
                    return;
                }
                let name = match event {
                    ConfirmAnalytic::PaymentCompleted => "payment_completed",
                    ConfirmAnalytic::PaymentFailed => "payment_failed",
                    ConfirmAnalytic::SubscriptionStarted => "subscription_started",
                    // GA is not carried over under D12; no invented PostHog replacement.
                    ConfirmAnalytic::Purchase { .. } => continue,
                };
                inner.platform.track_value(name, &serde_json::json!({}));
            }
        }
        // Core lifts navigation into AppEffect::Leave before it reaches the driver.
        ConfirmEffect::NavigateHome => {}
    }
}

fn invoices(inner: &Rc<AppDriver>, epoch: Epoch, ticket: u64, effect: InvoicesEffect) {
    // Echo this effect's ticket on every answer; never reread or mint a ticket.
    let answer = move |event| AppEvent::Invoices { epoch, ticket, event };
    match effect {
        InvoicesEffect::LoadOwn => fetch(
            inner,
            epoch,
            |api, user| async move { api.invoices(user).await },
            move |_, result| Some(answer(InvoicesEvent::Loaded(result.map_err(|e| e.purchase_failure())))),
        ),
        InvoicesEffect::LookUp { email } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.customer_invoices(user, &email).await },
            move |_, result| Some(answer(InvoicesEvent::LookupAnswered(result.map_err(|e| e.purchase_failure())))),
        ),
        InvoicesEffect::Document { facts, bill_to, temporary_bill_to, purpose } => {
            if !inner.is_current(epoch) {
                return;
            }
            let payment_id = facts.payment_id;
            // Capture once from the effect's owned facts before any font await.
            let captured = match InvoiceDocument::capture(&facts, &bill_to, &temporary_bill_to) {
                Ok(document) => document,
                Err(_) => {
                    inner.apply(answer(InvoicesEvent::DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable }));
                    return;
                }
            };
            let this = inner.clone();
            inner.detached(async move {
                let current = || match purpose {
                    DocumentPurpose::Preview => this.is_current_invoice_preview(epoch, ticket, payment_id),
                    DocumentPurpose::Download => this.is_current(epoch),
                };
                let delivery = export_invoice(&this.documents, &captured, &current, |bytes| match purpose {
                    DocumentPurpose::Preview => this.platform.preview_document(&bytes.bytes, &bytes.filename, &bytes.content_type),
                    DocumentPurpose::Download => this.platform.download(&bytes.bytes, &bytes.filename, &bytes.content_type),
                }).await;
                if !current() {
                    return;
                }
                match delivery {
                    Delivery::Delivered(Ok(())) | Delivery::Stale => {}
                    Delivery::Delivered(Err(_)) | Delivery::Failed(_) => {
                        // Core also rejects an old ticket: a stale failure cannot
                        // close a newer preview of the same payment.
                        this.apply(answer(InvoicesEvent::DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable }));
                    }
                }
            });
        }
    }
}

// ---- api keys ---------------------------------------------------------------------------------

/// The key screen (WEB-045/046): transport and the clipboard only. Every answer,
/// including a failed one, is core's to interpret (a stale epoch is dropped by `fetch`).
fn api_keys(inner: &Rc<AppDriver>, epoch: Epoch, effect: api_keys::Effect) {
    use api_keys::{Effect, Event};
    let keys = move |event: Event| AppEvent::ApiKeys { epoch, event };
    match effect {
        Effect::LoadKeys => fetch(
            inner,
            epoch,
            |api, user| async move { api.list_keys(user).await },
            move |_, result| Some(keys(Event::KeysLoaded { result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::LoadSigningSecret => fetch(
            inner,
            epoch,
            |api, user| async move { api.signing_secret(user).await },
            move |_, result| Some(keys(Event::SecretLoaded { result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::CreateKey { key } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.create_key(user, &key).await },
            move |_, result| Some(keys(Event::Created { result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::RevokeKey { id } => fetch(
            inner,
            epoch,
            move |api, user| async move { api.revoke_key(user, id).await },
            move |_, result| Some(keys(Event::Revoked { id, result: result.map_err(|e| failure(&e)) })),
        ),
        Effect::CopyText { text, what } => copy(inner, epoch, text, None, move |ok| Some(keys(Event::CopyAnswered { what, ok }))),
    }
}
