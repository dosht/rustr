//! Billing use cases over the ports.
//!
//! Every write follows one shape: no provider call while a transaction is
//! open; account row lock first; payment and subscription rows after;
//! all-or-nothing commit by whoever opened the transaction.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::calendar::add_months;
use crate::charge::{JobStart, StartCharge, StartRefusal, plan_batch, decide_deferred};
use crate::decimal::Decimal;
use crate::erasure::{CheckoutClosure, TerminationTarget};
use crate::freemium::{Facets, FreemiumLists, GuardRefusal, facets, is_freemium_app1};
use crate::grants::{Observation, PaymentStatus, Transition, expand_grant, transition};
use crate::ledger::{Allocation, PackageBalance, balance, balance_or_zero, open_at};
use crate::model::{
    BillingEvent, BillingInfo, NewPackage, NewPayment, NewSubscription, NewUnresolved, NewUsage,
    Payment, PaymentInsert, PaymentUpdate, UnresolvedNotification, reasons,
};
use crate::notify::{NotifyConfig, plan_analytics, plan_deliveries};
use crate::ports::{
    AnalyticsSink, BillingStore, BillingTx, Buyer, CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart,
    Customer, CustomerDirectory, DirectoryError, GatewayError, Headers, Mailer, OrderNotification,
    OrderWebhook, StoreError, SubscriptionGateway, SubscriptionNotification, SubscriptionOutcome,
    SubscriptionWebhook, UnitOfWork, WebhookError,
};
use crate::presentation::{
    AnalyticsIntent, CONFIRM_COUNTDOWN_SECONDS, CurrentPlan, HistoryRow, balance_text, confirm_analytics,
    current_plan, history_row, start_answer_meta, StartAnswerMeta,
};
use crate::pricing::{
    Catalog, Coupon, Currency, OrderAmountProblem, OrderError, OrderInput, Product, Quote, SpecialClientOffer,
    currency_for_locale, order_net, price_order,
};
use crate::types::{AccountId, Clock, Micros, SubjectRef};

/// The free package every account receives once, at signup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreemiumPolicy {
    pub name: String,
    pub minutes: i64,
    pub months: u32,
}

/// Eligibility data owned by the host, never by this crate and never by a
/// request. Emails are lower case.
#[derive(Clone, Debug, Default)]
pub struct Audience {
    pub special_offer_emails: BTreeSet<String>,
    pub special_clients: BTreeSet<AccountId>,
}

#[derive(Clone, Debug)]
pub struct BillingConfig {
    pub freemium: FreemiumPolicy,
    pub catalog: Catalog,
    pub rate: Decimal,
    pub lists: FreemiumLists,
    pub audience: Audience,
    pub special_client_offer: Option<SpecialClientOffer>,
    pub notify: NotifyConfig,
    /// Where providers send the browser back. Configured, never read from the
    /// request's Host header.
    pub public_base_url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BillingError {
    NotProvisioned,
    /// The account has an erasure barrier: refused without any change (D9, D10).
    AccountErased(AccountId),
    NotFound,
    InvalidRequest(String),
    Order(OrderError),
    Refused(StartRefusal),
    Guard(GuardRefusal),
    NotConfigured(String),
    /// The provider answered with an explicit failure.
    Gateway(GatewayError),
    /// No usable provider answer: state unchanged, retry or reconcile later.
    Uncertain(String),
    Conflict(String),
    Store(StoreError),
    Directory(DirectoryError),
    Webhook(WebhookError),
    Internal(String),
}

impl BillingError {
    pub fn code(&self) -> &'static str {
        match self {
            BillingError::NotProvisioned => "account_not_provisioned",
            BillingError::AccountErased(_) => "account_erased",
            BillingError::NotFound => "not_found",
            BillingError::InvalidRequest(_) => "invalid_request",
            BillingError::Order(_) => "invalid_order",
            BillingError::Refused(r) => r.code(),
            BillingError::Guard(_) => "insufficient_balance",
            BillingError::NotConfigured(_) => "not_configured",
            BillingError::Gateway(_) => "checkout_failed",
            BillingError::Uncertain(_) => "provider_uncertain",
            BillingError::Conflict(_) => "conflict",
            BillingError::Store(_) | BillingError::Directory(_) | BillingError::Internal(_) => "storage",
            BillingError::Webhook(_) => "unauthenticated",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            BillingError::NotProvisioned | BillingError::NotFound => 404,
            BillingError::AccountErased(_) => 410,
            BillingError::InvalidRequest(_) | BillingError::Order(_) => 400,
            BillingError::Refused(_) | BillingError::Guard(_) => 402,
            BillingError::NotConfigured(_) => 503,
            BillingError::Gateway(_) => 502,
            BillingError::Uncertain(_) => 504,
            BillingError::Conflict(_) => 409,
            BillingError::Store(_) | BillingError::Directory(_) | BillingError::Internal(_) => 500,
            BillingError::Webhook(_) => 401,
        }
    }
}

impl fmt::Display for BillingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BillingError::NotProvisioned => write!(f, "this account has no billing record yet"),
            BillingError::AccountErased(a) => write!(f, "billing account {a} is unavailable due to account erasure"),
            BillingError::NotFound => write!(f, "not found"),
            BillingError::InvalidRequest(e) | BillingError::Conflict(e) | BillingError::NotConfigured(e) | BillingError::Internal(e) | BillingError::Uncertain(e) => write!(f, "{e}"),
            BillingError::Order(e) => write!(f, "{e}"),
            BillingError::Refused(r) => write!(f, "{}", r.message(false)),
            BillingError::Guard(g) => write!(f, "{}", g.message()),
            BillingError::Gateway(e) => write!(f, "{e}"),
            BillingError::Store(e) => write!(f, "{e}"),
            BillingError::Directory(e) => write!(f, "{e}"),
            BillingError::Webhook(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BillingError {}

impl From<StoreError> for BillingError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Conflict(m) => BillingError::Conflict(m),
            StoreError::AccountErased(a) => BillingError::AccountErased(a),
            other => BillingError::Store(other),
        }
    }
}

impl From<DirectoryError> for BillingError {
    fn from(e: DirectoryError) -> Self {
        BillingError::Directory(e)
    }
}

impl From<OrderError> for BillingError {
    fn from(e: OrderError) -> Self {
        BillingError::Order(e)
    }
}

impl From<WebhookError> for BillingError {
    fn from(e: WebhookError) -> Self {
        match e {
            // A provider lookup that could not be answered is retryable, never a 401.
            WebhookError::Unavailable(why) => BillingError::Uncertain(why),
            other => BillingError::Webhook(other),
        }
    }
}

impl From<GatewayError> for BillingError {
    fn from(e: GatewayError) -> Self {
        match e {
            GatewayError::Uncertain(m) => BillingError::Uncertain(m),
            rejected => BillingError::Gateway(rejected),
        }
    }
}

/// A request to price or start an order. Nothing in it names an account,
/// an amount or a completion count.
#[derive(Clone, Debug, Default)]
pub struct OrderRequest {
    pub package_id: Option<usize>,
    pub pay_as_you_go_hours: Option<i64>,
    pub currency: Option<String>,
    pub ui_locale: String,
    pub coupon_id: Option<String>,
    pub gateway: String,
    pub analytics: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageView {
    pub id: i64,
    pub name: Option<String>,
    pub minutes_included: i64,
    pub consumed: i64,
    pub remaining: i64,
    pub start_at: Micros,
    pub end_at: Micros,
}

impl From<&PackageBalance> for PackageView {
    fn from(p: &PackageBalance) -> Self {
        PackageView {
            id: p.package.id,
            name: p.package.name.clone(),
            minutes_included: p.package.minutes_included,
            consumed: p.consumed,
            remaining: p.remaining(),
            start_at: p.package.start_at,
            end_at: p.package.end_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub balance: Option<i64>,
    pub balance_text: String,
    pub open_packages: Vec<PackageView>,
    pub packages: Vec<PackageView>,
    pub facets: Facets,
    pub is_freemium: bool,
    pub current_plan: CurrentPlan,
    pub special_client_alert: Option<crate::pricing::SpecialClientAlert>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Provisioned {
    pub account: AccountId,
    pub granted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckoutView {
    pub payment_id: i64,
    pub gateway: String,
    pub token: String,
    pub form_content: Option<String>,
    pub url: Option<String>,
    pub amount: Decimal,
    pub currency: String,
    pub coupon_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfirmView {
    pub payment_id: i64,
    pub status: PaymentStatus,
    pub success: bool,
    /// False for a repeat: nothing changed and no events are emitted.
    pub changed: bool,
    pub analytics: Vec<AnalyticsIntent>,
    pub redirect_after_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum NotificationOutcome {
    Applied { payment_id: i64, status: PaymentStatus, changed: bool },
    /// Verified, understood and deliberately a no-op.
    Ignored { reason: String },
    /// Verified but not applicable yet. Stored durably for an admin.
    Held { reason: String, held_id: i64 },
}

#[derive(Clone, Debug, Serialize)]
pub struct AdminLookup {
    pub customer: Customer,
    pub payments: Vec<Payment>,
    pub history: Vec<HistoryRow>,
    pub billing_info: Option<BillingInfo>,
    /// The same invoice facts the customer's own context carries (P5 D7).
    pub invoice_context: crate::presentation::invoice::InvoiceContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelView {
    pub payment_id: i64,
    pub canceled_at: Micros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ResolveView {
    pub already_resolved: bool,
    pub replayed: usize,
}

/// The erasure after a verified checkout closure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErasureCheckoutView {
    pub payment_id: i64,
    /// Plan checkouts that still keep the erasure pending.
    pub remaining_unbound: Vec<i64>,
    /// Subscription targets not yet confirmed stopped.
    pub pending_subscriptions: usize,
    /// Nothing on the billing side blocks the local cascade any more.
    pub billing_blockers_cleared: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RelayReport {
    pub claimed: usize,
    pub acknowledged: usize,
    pub retried: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaymentStatusView {
    pub payment_id: i64,
    pub gateway: String,
    pub status: PaymentStatus,
}

pub struct Billing {
    store: Arc<dyn BillingStore>,
    clock: Arc<dyn Clock>,
    directory: Arc<dyn CustomerDirectory>,
    config: BillingConfig,
    checkout: BTreeMap<String, Arc<dyn CheckoutGateway>>,
    subscription_webhook: Option<Arc<dyn SubscriptionWebhook>>,
    subscription_gateway: Option<Arc<dyn SubscriptionGateway>>,
    order_webhook: Option<Arc<dyn OrderWebhook>>,
    mailer: Option<Arc<dyn Mailer>>,
    analytics: Option<Arc<dyn AnalyticsSink>>,
    mail_required: bool,
}

async fn finish<T>(uow: Box<dyn UnitOfWork>, result: Result<T, BillingError>) -> Result<T, BillingError> {
    match result {
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

/// A checkout result lookup. An error envelope on the lookup call (a wrong key, an
/// unknown token) says nothing about whether the customer paid: the payment stays as
/// it was, and an admin can reconcile. Only a payment-status answer is definite.
async fn checkout_result(gateway: &dyn CheckoutGateway, token: &str, kind: CheckoutKind) -> Result<CheckoutResult, BillingError> {
    match gateway.result(token, kind).await {
        Ok(result) => Ok(result),
        Err(GatewayError::Rejected { code, message }) => {
            Err(BillingError::Uncertain(format!("the provider could not confirm the payment: {code} {message}")))
        }
        Err(GatewayError::Uncertain(why)) => Err(BillingError::Uncertain(why)),
    }
}

fn internal(e: impl fmt::Display) -> BillingError {
    BillingError::Internal(e.to_string())
}

impl Billing {
    pub fn new(
        store: Arc<dyn BillingStore>,
        clock: Arc<dyn Clock>,
        directory: Arc<dyn CustomerDirectory>,
        config: BillingConfig,
    ) -> Billing {
        Billing {
            store,
            clock,
            directory,
            config,
            checkout: BTreeMap::new(),
            subscription_webhook: None,
            subscription_gateway: None,
            order_webhook: None,
            mailer: None,
            analytics: None,
            mail_required: false,
        }
    }

    pub fn with_checkout(mut self, gateway: Arc<dyn CheckoutGateway>) -> Billing {
        self.checkout.insert(gateway.gateway().to_string(), gateway);
        self
    }

    pub fn with_subscription_webhook(mut self, hook: Arc<dyn SubscriptionWebhook>) -> Billing {
        self.subscription_webhook = Some(hook);
        self
    }

    pub fn with_subscription_gateway(mut self, gateway: Arc<dyn SubscriptionGateway>) -> Billing {
        self.subscription_gateway = Some(gateway);
        self
    }

    pub fn with_order_webhook(mut self, hook: Arc<dyn OrderWebhook>) -> Billing {
        self.order_webhook = Some(hook);
        self
    }

    pub fn with_mailer(mut self, mailer: Arc<dyn Mailer>) -> Billing {
        self.mailer = Some(mailer);
        self
    }

    /// Server-side analytics: each committed purchase event is sent once to this
    /// destination from the outbox, with a stable dedup identity.
    pub fn with_analytics(mut self, sink: Arc<dyn AnalyticsSink>) -> Billing {
        self.analytics = Some(sink);
        self
    }

    /// A deployment whose product promises email (buyer, sales, support) is not
    /// ready without a mailer: readiness fails instead of silently queueing.
    pub fn requires_mail(mut self) -> Billing {
        self.mail_required = true;
        self
    }

    pub fn config(&self) -> &BillingConfig {
        &self.config
    }

    pub fn store(&self) -> &Arc<dyn BillingStore> {
        &self.store
    }

    pub fn directory(&self) -> &Arc<dyn CustomerDirectory> {
        &self.directory
    }

    pub fn now(&self) -> Micros {
        self.clock.now()
    }

    /// Names of the gateways and hooks this deployment actually has.
    pub fn configured(&self) -> Vec<String> {
        let mut names: Vec<String> = self.checkout.keys().map(|k| format!("checkout:{k}")).collect();
        if self.subscription_webhook.is_some() {
            names.push("webhook:iyzico".into());
        }
        if self.order_webhook.is_some() {
            names.push("webhook:lemonsqueezy".into());
        }
        if self.mailer.is_some() {
            names.push("mail".into());
        }
        if self.analytics.is_some() {
            names.push("analytics".into());
        }
        names
    }

    pub async fn ready(&self) -> Result<(), BillingError> {
        if self.mail_required && self.mailer.is_none() {
            return Err(BillingError::NotConfigured("mail delivery is required and not configured".into()));
        }
        self.store.ping().await.map_err(BillingError::from)
    }

    fn gateway(&self, name: &str) -> Result<&Arc<dyn CheckoutGateway>, BillingError> {
        self.checkout
            .get(name)
            .ok_or_else(|| BillingError::NotConfigured(format!("the {name} gateway is not configured")))
    }

    // ---- accounts ---------------------------------------------------------

    /// Creates the account and its one free package, anchored to the trusted
    /// signup instant carried by the customer record. Repeats and late first
    /// access never grant again.
    pub async fn provision(&self, customer: Customer) -> Result<Provisioned, BillingError> {
        let stored = self.directory.register(&customer).await?;
        let mut uow = self.store.begin().await?;
        let result = self.provision_in(uow.tx(), &stored).await;
        finish(uow, result).await
    }

    async fn provision_in(&self, tx: &mut dyn BillingTx, customer: &Customer) -> Result<Provisioned, BillingError> {
        let account = customer.account;
        if !tx.create_account(account, customer.created_at).await? {
            return Ok(Provisioned { account, granted: false });
        }
        let policy = &self.config.freemium;
        let end = add_months(customer.created_at, i64::from(policy.months)).map_err(internal)?;
        let package_id = tx
            .insert_package(&NewPackage {
                account,
                payment_id: None,
                payment_seq: None,
                name: policy.name.clone(),
                minutes: policy.minutes,
                start_at: customer.created_at,
                end_at: end,
                created: customer.created_at,
                signup_grant: true,
            })
            .await?;
        tx.emit(&BillingEvent::FreemiumGranted { account, package_id }, self.now()).await?;
        Ok(Provisioned { account, granted: true })
    }

    /// Normal authenticated access: the account must exist, or the host's
    /// directory must hold a trusted creation instant to provision from.
    /// Otherwise refuse; never invent a signup time from `now` or a token. An
    /// account with an erasure barrier is refused before anything else (D9).
    pub async fn ensure_provisioned(&self, account: AccountId) -> Result<(), BillingError> {
        let mut uow = self.store.begin().await?;
        let read = async {
            let erasing = uow.tx().erasure_status(account).await?.is_some();
            let created = uow.tx().account_created_at(account).await?;
            Ok::<_, StoreError>((erasing, created))
        }
        .await;
        let _ = uow.rollback().await;
        let (erasing, created) = read?;
        if erasing {
            return Err(BillingError::AccountErased(account));
        }
        if created.is_some() {
            return Ok(());
        }
        match self.directory.by_account(account).await? {
            Some(customer) => self.provision(customer).await.map(|_| ()),
            None => Err(BillingError::NotProvisioned),
        }
    }

    pub async fn overview(&self, account: AccountId) -> Result<Overview, BillingError> {
        self.ensure_provisioned(account).await?;
        let mut uow = self.store.begin().await?;
        let all = uow.tx().packages(account).await;
        let _ = uow.rollback().await;
        let all = all?;
        let now = self.now();
        let open = open_at(&all, now);
        let packages: Vec<_> = all.iter().map(|p| p.package.clone()).collect();
        let open_packages: Vec<_> = open.iter().map(|p| p.package.clone()).collect();
        let bal = balance(&open);
        Ok(Overview {
            balance: bal,
            // No open package is displayed as `0 min` (BIL-008); the field itself stays absent.
            balance_text: balance_text(Some(balance_or_zero(&open))),
            open_packages: open.iter().map(PackageView::from).collect(),
            packages: all.iter().map(PackageView::from).collect(),
            facets: facets(&packages, now),
            is_freemium: is_freemium_app1(&packages),
            current_plan: current_plan(&open_packages),
            special_client_alert: self.config.special_client_offer.as_ref().and_then(|offer| {
                crate::pricing::special_client_alert(self.config.audience.special_clients.contains(&account), offer)
            }),
        })
    }

    // ---- pricing and checkout ---------------------------------------------

    fn resolve_product(&self, request: &OrderRequest) -> Result<Product, BillingError> {
        match (request.package_id, request.pay_as_you_go_hours) {
            (Some(index), _) => Ok(Product::Plan { index }),
            (None, Some(hours)) => Ok(Product::PayAsYouGo { hours }),
            (None, None) => Err(BillingError::InvalidRequest("choose a package or pay-as-you-go hours".into())),
        }
    }

    async fn price(
        &self,
        tx: &mut dyn BillingTx,
        request: &OrderRequest,
        email: Option<&str>,
    ) -> Result<Quote, BillingError> {
        let product = self.resolve_product(request)?;
        let currency = match &request.currency {
            Some(text) => Currency::parse(text)
                .ok_or_else(|| BillingError::InvalidRequest(format!("unknown currency {text:?}")))?,
            None => currency_for_locale(&request.ui_locale),
        };
        let coupon: Option<Coupon> = match request.coupon_id.as_deref() {
            Some(id) if !id.is_empty() => tx.coupon(id).await?,
            _ => None,
        };
        let special_offer = email
            .map(|e| self.config.audience.special_offer_emails.contains(&e.to_lowercase()))
            .unwrap_or(false);
        Ok(price_order(
            &self.config.catalog,
            &OrderInput {
                product,
                currency,
                ui_locale: &request.ui_locale,
                coupon: coupon.as_ref(),
                special_offer,
                rate: &self.config.rate,
                now: self.now(),
            },
        )?)
    }

    async fn customer(&self, account: AccountId) -> Result<Customer, BillingError> {
        self.directory.by_account(account).await?.ok_or(BillingError::NotProvisioned)
    }

    /// The quote plus the total the purchase summary shows (P5 D8). The two are
    /// computed separately and are never forced equal.
    pub async fn quote_view(&self, account: AccountId, request: &OrderRequest) -> Result<crate::presentation::purchase::QuoteView, BillingError> {
        self.ensure_provisioned(account).await?;
        let customer = self.customer(account).await?;
        let product = self.resolve_product(request)?;
        let mut uow = self.store.begin().await?;
        let read = async {
            let quote = self.price(uow.tx(), request, Some(&customer.email)).await?;
            let coupon = match request.coupon_id.as_deref() {
                Some(id) if !id.is_empty() => uow.tx().coupon(id).await?,
                _ => None,
            };
            let display_amount =
                crate::pricing::display_amount(&self.config.catalog, product, &request.ui_locale, coupon.as_ref(), self.now())?;
            Ok::<_, BillingError>(crate::presentation::purchase::QuoteView { quote, display_amount })
        }
        .await;
        let _ = uow.rollback().await;
        read
    }

    /// Prices an order for a provisioned account without persisting anything.
    pub async fn quote(&self, account: AccountId, request: &OrderRequest) -> Result<Quote, BillingError> {
        self.ensure_provisioned(account).await?;
        let customer = self.customer(account).await?;
        let mut uow = self.store.begin().await?;
        let quote = self.price(uow.tx(), request, Some(&customer.email)).await;
        let _ = uow.rollback().await;
        quote
    }

    pub async fn start_checkout(&self, account: AccountId, request: &OrderRequest) -> Result<CheckoutView, BillingError> {
        self.ensure_provisioned(account).await?;
        let gateway = self.gateway(&request.gateway)?.clone();
        let customer = self.customer(account).await?;
        let mut uow = self.store.begin().await?;
        let quote = self.price(uow.tx(), request, Some(&customer.email)).await;
        let _ = uow.rollback().await;
        let quote = quote?;

        let lemon = request.gateway == "lemonsqueezy";
        if lemon && (quote.hours.is_none() || quote.currency != Currency::Usd) {
            return Err(BillingError::InvalidRequest(
                "this gateway sells pay-as-you-go hours in USD only".into(),
            ));
        }
        let kind = if quote.pricing_plan_id.is_some() { CheckoutKind::Subscription } else { CheckoutKind::PayAsYouGo };
        let buyer = Buyer { account, email: customer.email.clone(), full_name: customer.full_name.clone() };
        let base_url = self.config.public_base_url.trim_end_matches('/').to_string();
        let mut metadata = json!({ "package": serde_json::to_value(&quote.spec).map_err(internal)? });
        let mut analytics = match &request.analytics {
            Value::Object(map) => Value::Object(map.clone()),
            _ => json!({}),
        };
        analytics["payment_origin"] = json!("initial");
        metadata["analytics"] = analytics;

        let new_payment = |token: Option<String>, conversation: &str, metadata: &Value| {
            let mut metadata = metadata.clone();
            metadata["conversation_id"] = json!(conversation);
            NewPayment {
                account,
                amount: quote.amount,
                currency: quote.currency.as_str().to_string(),
                gateway: request.gateway.clone(),
                environment: gateway.environment().to_string(),
                token,
                created: self.now(),
                status: PaymentStatus::Pending,
                metadata,
                coupon_id: quote.coupon_id.clone(),
                pricing_plan_id: quote.pricing_plan_id.clone(),
                provider_order_ref: None,
                subscription_ref: None,
                customer_id: None,
            }
        };
        // Iyzico posts the browser back to the confirm route with its token. LemonSqueezy
        // redirects with a plain GET and no token, so it returns to the authenticated status
        // route for our own payment id, mounted by hosts under `/api`: it reads the stored
        // payment and never grants or calls a provider.
        let start = |reference: String| {
            let callback_url = if lemon {
                format!("{base_url}/api/billing/payments/{reference}/status")
            } else {
                format!("{base_url}/checkout/confirm")
            };
            CheckoutStart {
                reference,
                kind,
                amount: quote.amount,
                currency: quote.currency.as_str().to_string(),
                product_name: quote.product_name.clone(),
                pricing_plan_id: quote.pricing_plan_id.clone(),
                locale: request.ui_locale.clone(),
                callback_url,
                buyer: buyer.clone(),
            }
        };

        if !lemon {
            // Initialize first, with no transaction open. Persist before the
            // form is returned; a failed insert returns no payable form (BIL-067).
            let reference = Uuid::new_v4().to_string();
            let started = gateway.start(&start(reference.clone())).await.map_err(BillingError::from)?;
            let mut uow = self.store.begin().await?;
            let inserted = uow.tx().insert_payment(&new_payment(Some(started.token.clone()), &reference, &metadata)).await;
            let payment = match inserted {
                Ok(PaymentInsert::Inserted(p)) => p,
                Ok(PaymentInsert::Duplicate(_)) => {
                    let _ = uow.rollback().await;
                    return Err(BillingError::Conflict("the provider token is already recorded".into()));
                }
                Err(e) => {
                    let _ = uow.rollback().await;
                    return Err(e.into());
                }
            };
            uow.commit().await?;
            return Ok(CheckoutView {
                payment_id: payment.id,
                gateway: request.gateway.clone(),
                token: started.token,
                form_content: started.form_content,
                url: started.url,
                amount: quote.amount,
                currency: payment.currency,
                coupon_id: payment.coupon_id,
            });
        }

        // LemonSqueezy: our payment id is the correlation key, so the pending
        // payment and its frozen quote exist before the provider call.
        let mut uow = self.store.begin().await?;
        let inserted = uow.tx().insert_payment(&new_payment(None, "", &metadata)).await;
        let payment = match inserted {
            Ok(PaymentInsert::Inserted(p)) => p,
            Ok(PaymentInsert::Duplicate(p)) => p,
            Err(e) => {
                let _ = uow.rollback().await;
                return Err(e.into());
            }
        };
        uow.commit().await?;
        match gateway.start(&start(payment.id.to_string())).await {
            Ok(started) => {
                // A fresh barrier observation under the account lock before the URL is
                // saved or shown: an erasure opened during the provider call refuses it.
                let mut uow = self.store.begin().await?;
                let update = PaymentUpdate { token: Some(started.token.clone()), ..PaymentUpdate::default() };
                let result = async {
                    uow.tx().lock_account(payment.account).await?;
                    uow.tx().update_payment(payment.id, &update).await
                }
                .await
                .map_err(BillingError::from);
                finish(uow, result).await?;
                Ok(CheckoutView {
                    payment_id: payment.id,
                    gateway: request.gateway.clone(),
                    token: started.token,
                    form_content: None,
                    url: started.url,
                    amount: quote.amount,
                    currency: payment.currency,
                    coupon_id: payment.coupon_id,
                })
            }
            Err(GatewayError::Uncertain(why)) => {
                // Not a rejection: the payment stays pending and nothing is claimed.
                Err(BillingError::Uncertain(why))
            }
            Err(rejected) => {
                let mut uow = self.store.begin().await?;
                let result = self
                    .fail_in(uow.tx(), payment.account, payment.id)
                    .await;
                finish(uow, result).await?;
                Err(BillingError::from(rejected))
            }
        }
    }

    async fn fail_in(&self, tx: &mut dyn BillingTx, account: AccountId, payment_id: i64) -> Result<(), BillingError> {
        tx.lock_account(account).await?;
        let Some(current) = tx.payment(payment_id, true).await? else { return Ok(()) };
        if let Transition::Apply { to, .. } = transition(current.status, Observation::Failed) && tx.set_payment_status(current.id, current.status, to).await? {
            let mut failed = current.clone();
            failed.status = PaymentStatus::Failed;
            self.emit_failed(tx, &failed).await?;
        }
        Ok(())
    }

    // ---- confirmation -----------------------------------------------------

    fn confirm_view(&self, payment: &Payment, changed_to: Option<PaymentStatus>) -> ConfirmView {
        ConfirmView {
            payment_id: payment.id,
            status: payment.status,
            success: payment.status == PaymentStatus::Completed,
            changed: changed_to.is_some(),
            analytics: confirm_analytics(changed_to, payment, &self.config.rate),
            redirect_after_seconds: CONFIRM_COUNTDOWN_SECONDS,
        }
    }

    /// The browser return of a hosted checkout. A transport error leaves the
    /// payment as it was and says so; only a definite provider answer moves it.
    /// Under erasure (D10) nothing is applied: an authenticated subscription for
    /// a plan checkout no target covers is kept as a cancellation target.
    pub async fn confirm_checkout(&self, gateway_name: &str, token: &str) -> Result<ConfirmView, BillingError> {
        let gateway = self.gateway(gateway_name)?.clone();
        let mut uow = self.store.begin().await?;
        let read = async {
            let Some(payment) = uow.tx().payment_by_token(gateway_name, gateway.environment(), token, false).await? else {
                return Ok(None);
            };
            let erasure = uow.tx().erasure_status(payment.account).await?;
            Ok::<_, StoreError>(Some((payment, erasure)))
        }
        .await;
        let _ = uow.rollback().await;
        let (payment, erasure) = read?.ok_or(BillingError::NotFound)?;
        if let Some(status) = erasure {
            // Whatever the local status, a completed imported row included: only a plan
            // checkout no target covers can still hide a renewing subscription.
            if payment.pricing_plan_id.is_none() || !status.unbound_plan_payments.contains(&payment.id) {
                return Err(BillingError::AccountErased(payment.account));
            }
            let result = checkout_result(gateway.as_ref(), token, CheckoutKind::Subscription).await?;
            return self.keep_erasure_target(&payment, &result).await;
        }
        if payment.status == PaymentStatus::Completed {
            return Ok(self.confirm_view(&payment, None));
        }
        let kind = if payment.pricing_plan_id.is_some() { CheckoutKind::Subscription } else { CheckoutKind::PayAsYouGo };
        let result = checkout_result(gateway.as_ref(), token, kind).await?;
        let observation = match &result {
            CheckoutResult::Success { .. } => Observation::Completed,
            CheckoutResult::Failure { .. } => Observation::Failed,
        };
        let success = matches!(result, CheckoutResult::Success { .. }).then_some(&result);
        let mut uow = self.store.begin().await?;
        let applied = self.observe_in(uow.tx(), payment.account, payment.id, observation, success).await;
        match finish(uow, applied).await {
            // The erasure opened between the read and the account lock: the authenticated
            // answer is kept as a target instead of being applied.
            Err(BillingError::AccountErased(_)) if payment.pricing_plan_id.is_some() => self.keep_erasure_target(&payment, &result).await,
            other => other,
        }
    }

    /// D10, erasure only: an authenticated subscription for a plan checkout becomes a
    /// pending termination target and the payment keeps the provider's references,
    /// under the account's erasure lock, then payment row lock. No status change, grant,
    /// binding or event. The evidence is committed before the account is refused.
    async fn keep_erasure_target(&self, payment: &Payment, result: &CheckoutResult) -> Result<ConfirmView, BillingError> {
        let account = payment.account;
        let CheckoutResult::Success { customer_ref, subscription_ref: Some(subscription), observed_amount, observed_currency, provider_payment_id } =
            result
        else {
            // A payment failure, or success without a subscription, proves nothing about the form.
            return Err(BillingError::AccountErased(account));
        };
        let now = self.now();
        let mut uow = self.store.begin().await?;
        let kept = async {
            let tx = uow.tx();
            tx.lock_account_for_erasure(account).await?;
            let current = tx.payment(payment.id, true).await?.ok_or(BillingError::NotFound)?;
            if current.subscription_ref.as_ref().is_some_and(|stored| stored != subscription) {
                return Err(BillingError::Conflict("the provider reports another subscription than the payment's stored reference".into()));
            }
            if let (Some(stored), Some(reported)) = (&current.customer_id, customer_ref)
                && stored != reported
            {
                return Err(BillingError::Conflict("the provider reports another customer than the payment's stored reference".into()));
            }
            let target = TerminationTarget {
                gateway: current.gateway.clone(),
                environment: current.environment.clone(),
                subscription_ref: subscription.clone(),
                customer_ref: customer_ref.clone().or_else(|| current.customer_id.clone()),
            };
            tx.adopt_erasure_target(account, &target, now).await?;
            let update = PaymentUpdate {
                customer_id: target.customer_ref.clone(),
                subscription_ref: Some(subscription.clone()),
                observed: Some(json!({
                    "amount": observed_amount.map(|a| a.to_string()),
                    "currency": observed_currency,
                    "provider_payment_id": provider_payment_id,
                })),
                ..PaymentUpdate::default()
            };
            tx.update_payment(current.id, &update).await?;
            Ok::<_, BillingError>(())
        }
        .await;
        finish(uow, kept).await?;
        Err(BillingError::AccountErased(account))
    }

    async fn observe_in(
        &self,
        tx: &mut dyn BillingTx,
        account: AccountId,
        payment_id: i64,
        observation: Observation,
        result: Option<&CheckoutResult>,
    ) -> Result<ConfirmView, BillingError> {
        tx.lock_account(account).await?;
        let current = tx.payment(payment_id, true).await?.ok_or(BillingError::NotFound)?;
        let Transition::Apply { to, grants } = transition(current.status, observation) else {
            return Ok(self.confirm_view(&current, None));
        };
        if !tx.set_payment_status(current.id, current.status, to).await? {
            return Err(BillingError::Conflict("the payment changed concurrently".into()));
        }
        let mut updated = current.clone();
        updated.status = to;
        let mut update = PaymentUpdate::default();
        if let Some(CheckoutResult::Success {
            customer_ref,
            subscription_ref,
            observed_amount,
            observed_currency,
            provider_payment_id,
        }) = result
        {
            update.customer_id = customer_ref.clone();
            update.subscription_ref = subscription_ref.clone();
            update.observed = Some(json!({
                "amount": observed_amount.map(|a| a.to_string()),
                "currency": observed_currency,
                "provider_payment_id": provider_payment_id,
            }));
            updated.customer_id = customer_ref.clone().or(updated.customer_id);
            updated.subscription_ref = subscription_ref.clone().or(updated.subscription_ref);
        }
        if update != PaymentUpdate::default() {
            tx.update_payment(current.id, &update).await?;
        }
        if grants {
            self.grant_packages(tx, &updated).await?;
            if let (Some(_), Some(sub_ref)) = (&updated.pricing_plan_id, &updated.subscription_ref) {
                let spec = updated.spec().ok_or_else(|| BillingError::InvalidRequest("payment has no package metadata".into()))?;
                tx.insert_subscription(&NewSubscription {
                    account,
                    gateway: updated.gateway.clone(),
                    environment: updated.environment.clone(),
                    subscription_ref: sub_ref.clone(),
                    customer_ref: updated.customer_id.clone(),
                    pricing_plan_id: updated.pricing_plan_id.clone(),
                    currency: updated.currency.clone(),
                    amount: updated.amount,
                    spec,
                    initial_payment_id: updated.id,
                    created_at: self.now(),
                })
                .await?;
            }
            self.emit_completed(tx, &updated).await?;
        } else {
            self.emit_failed(tx, &updated).await?;
        }
        Ok(self.confirm_view(&updated, Some(to)))
    }

    async fn grant_packages(&self, tx: &mut dyn BillingTx, payment: &Payment) -> Result<(), BillingError> {
        let spec = payment
            .spec()
            .ok_or_else(|| BillingError::InvalidRequest("payment has no package metadata".into()))?;
        let granted = expand_grant(payment.created, &spec).map_err(|e| BillingError::InvalidRequest(e.to_string()))?;
        let now = self.now();
        for package in granted {
            tx.insert_package(&NewPackage {
                account: payment.account,
                payment_id: Some(payment.id),
                payment_seq: Some(package.sequence),
                name: package.name,
                minutes: package.minutes,
                start_at: package.start_at,
                end_at: package.end_at,
                created: now,
                signup_grant: false,
            })
            .await?;
        }
        Ok(())
    }

    async fn emit_completed(&self, tx: &mut dyn BillingTx, payment: &Payment) -> Result<(), BillingError> {
        let event = BillingEvent::PaymentCompleted {
            payment_id: payment.id,
            account: payment.account,
            amount: payment.amount,
            currency: payment.currency.clone(),
            gateway: payment.gateway.clone(),
            package_name: payment.package_name(),
            renewal: payment.is_renewal(),
            payment: serde_json::to_value(payment).map_err(internal)?,
        };
        tx.emit(&event, self.now()).await?;
        Ok(())
    }

    async fn emit_failed(&self, tx: &mut dyn BillingTx, payment: &Payment) -> Result<(), BillingError> {
        let event = BillingEvent::PaymentFailed {
            payment_id: payment.id,
            account: payment.account,
            amount: payment.amount,
            currency: payment.currency.clone(),
            gateway: payment.gateway.clone(),
            payment: serde_json::to_value(payment).map_err(internal)?,
        };
        tx.emit(&event, self.now()).await?;
        Ok(())
    }

    /// Admin recovery for a payment left pending by an uncertain confirmation.
    /// Same flow as the browser return; no schedule, no blind retry. While the
    /// account is under erasure a completed Iyzico plan payment may be asked about
    /// too, so an imported row no target covers can still yield its subscription.
    pub async fn reconcile_payment(&self, payment_id: i64) -> Result<ConfirmView, BillingError> {
        let mut uow = self.store.begin().await?;
        let read = async {
            let Some(payment) = uow.tx().payment(payment_id, false).await? else { return Ok(None) };
            let erasing = uow.tx().erasure_status(payment.account).await?.is_some();
            Ok::<_, StoreError>(Some((payment, erasing)))
        }
        .await;
        let _ = uow.rollback().await;
        let (payment, erasing) = read?.ok_or(BillingError::NotFound)?;
        let reconcilable = payment.status != PaymentStatus::Completed || (erasing && payment.pricing_plan_id.is_some());
        let token = payment
            .token
            .clone()
            .filter(|_| payment.gateway == "iyzico" && reconcilable)
            .ok_or_else(|| BillingError::InvalidRequest("only an unfinished Iyzico checkout can be reconciled".into()))?;
        self.confirm_checkout(&payment.gateway, &token).await
    }

    // ---- Iyzico subscription notifications --------------------------------

    pub async fn process_subscription_webhook(&self, headers: &Headers, body: &[u8]) -> Result<NotificationOutcome, BillingError> {
        let hook = self
            .subscription_webhook
            .as_ref()
            .ok_or_else(|| BillingError::NotConfigured("the Iyzico webhook verifier is not configured".into()))?;
        let notification = match hook.verify(headers, body) {
            Ok(n) => n,
            Err(WebhookError::Unsupported(reason)) => return Ok(NotificationOutcome::Ignored { reason }),
            Err(e) => return Err(e.into()),
        };
        let environment = hook.environment().to_string();
        self.process_notification(&notification, &environment, None, None).await
    }

    async fn process_notification(
        &self,
        notification: &SubscriptionNotification,
        environment: &str,
        replay_of: Option<i64>,
        received_at: Option<Micros>,
    ) -> Result<NotificationOutcome, BillingError> {
        let mut uow = self.store.begin().await?;
        let result = self.notification_in(uow.tx(), notification, environment, replay_of, received_at).await;
        finish(uow, result).await
    }

    async fn hold(
        &self,
        tx: &mut dyn BillingTx,
        notification: &SubscriptionNotification,
        environment: &str,
        reason: &str,
        payment_id: Option<i64>,
    ) -> Result<NotificationOutcome, BillingError> {
        let event_type = match notification.outcome {
            SubscriptionOutcome::Success => "success",
            SubscriptionOutcome::Failure => "failure",
        };
        let held = NewUnresolved {
            gateway: "iyzico".into(),
            environment: environment.to_string(),
            event_type: event_type.into(),
            subscription_ref: Some(notification.subscription_ref.clone()),
            order_ref: Some(notification.order_ref.clone()),
            customer_ref: Some(notification.customer_ref.clone()),
            payment_id,
            reason: reason.to_string(),
            payload: json!({
                "raw_event_type": notification.raw_event_type,
                "order_ref": notification.order_ref,
                "subscription_ref": notification.subscription_ref,
                "customer_ref": notification.customer_ref,
            }),
            received_at: self.now(),
        };
        let (row, inserted) = tx.hold_notification(&held).await?;
        if inserted {
            tx.emit(
                &BillingEvent::NotificationHeld {
                    gateway: "iyzico".into(),
                    reason: reason.to_string(),
                    order_ref: Some(notification.order_ref.clone()),
                    subscription_ref: Some(notification.subscription_ref.clone()),
                },
                self.now(),
            )
            .await?;
        }
        Ok(NotificationOutcome::Held { reason: reason.to_string(), held_id: row.id })
    }

    async fn notification_in(
        &self,
        tx: &mut dyn BillingTx,
        n: &SubscriptionNotification,
        environment: &str,
        replay_of: Option<i64>,
        received_at: Option<Micros>,
    ) -> Result<NotificationOutcome, BillingError> {
        let Some(found) = tx.subscription("iyzico", environment, &n.subscription_ref, false).await? else {
            return self.hold(tx, n, environment, reasons::SUBSCRIPTION_UNBOUND, None).await;
        };
        tx.lock_account(found.account).await?;
        let sub = tx
            .subscription("iyzico", environment, &n.subscription_ref, true)
            .await?
            .ok_or_else(|| internal("subscription binding vanished"))?;
        if sub.customer_ref.as_deref() != Some(n.customer_ref.as_str()) {
            return self.hold(tx, n, environment, reasons::CUSTOMER_MISMATCH, None).await;
        }
        // D4: no documented automatic proof of which order is the initial one.
        // Until an admin identifies it, every verified order is held.
        let Some(first) = sub.first_order_ref.clone() else {
            return self.hold(tx, n, environment, reasons::INITIAL_ORDER_UNPROVEN, None).await;
        };
        let now = self.now();
        // A replayed renewal starts its packages when its notification arrived,
        // not when an admin resolved the held row (BIL-045, BIL-055).
        let created = received_at.unwrap_or(now);
        if first == n.order_ref {
            if let Some(id) = replay_of {
                tx.mark_unresolved(id, "replayed", None, now).await?;
            }
            return Ok(NotificationOutcome::Ignored { reason: "initial_order_alias".into() });
        }
        // Read without a lock first: another account's row is looked at, never locked.
        let existing = tx.payment_by_order_ref("iyzico", environment, &n.order_ref, false).await?;
        let payment = match existing {
            Some(p) => p,
            None => {
                let spec = serde_json::to_value(&sub.spec).map_err(internal)?;
                let insert = NewPayment {
                    account: sub.account,
                    amount: sub.amount,
                    currency: sub.currency.clone(),
                    gateway: "iyzico".into(),
                    environment: environment.to_string(),
                    token: Some(n.order_ref.clone()),
                    created,
                    status: PaymentStatus::Pending,
                    metadata: json!({"package": spec, "analytics": {"payment_origin": "renewal"}}),
                    coupon_id: None,
                    pricing_plan_id: sub.pricing_plan_id.clone(),
                    provider_order_ref: Some(n.order_ref.clone()),
                    subscription_ref: Some(sub.subscription_ref.clone()),
                    customer_id: Some(n.customer_ref.clone()),
                };
                match tx.insert_payment(&insert).await? {
                    PaymentInsert::Inserted(p) | PaymentInsert::Duplicate(p) => p,
                }
            }
        };
        // An order reference that already belongs to another account, subscription
        // or customer is conflicting evidence, never a payment to transition.
        if payment.account != sub.account
            || payment.subscription_ref.as_deref() != Some(sub.subscription_ref.as_str())
            || payment.customer_id.as_deref() != Some(n.customer_ref.as_str())
        {
            return self.hold(tx, n, environment, reasons::CONFLICTING_ORDER, Some(payment.id)).await;
        }
        let payment = tx.payment(payment.id, true).await?.ok_or(BillingError::NotFound)?;
        let observation = match n.outcome {
            SubscriptionOutcome::Success => Observation::Completed,
            SubscriptionOutcome::Failure => Observation::Failed,
        };
        let (status, changed) = match transition(payment.status, observation) {
            Transition::Ignore => (payment.status, false),
            Transition::Apply { to, grants } => {
                if !tx.set_payment_status(payment.id, payment.status, to).await? {
                    return Err(BillingError::Conflict("the payment changed concurrently".into()));
                }
                let mut updated = payment.clone();
                updated.status = to;
                if grants {
                    self.grant_packages(tx, &updated).await?;
                    self.emit_completed(tx, &updated).await?;
                } else {
                    self.emit_failed(tx, &updated).await?;
                }
                (to, true)
            }
        };
        if let Some(id) = replay_of {
            tx.mark_unresolved(id, "replayed", Some(payment.id), now).await?;
        }
        Ok(NotificationOutcome::Applied { payment_id: payment.id, status, changed })
    }

    pub async fn list_unresolved(&self, status: Option<&str>) -> Result<Vec<UnresolvedNotification>, BillingError> {
        let mut uow = self.store.begin().await?;
        let rows = uow.tx().list_unresolved(status).await;
        let _ = uow.rollback().await;
        Ok(rows?)
    }

    /// Re-runs a held Iyzico notification through the normal path. It stays
    /// held if it is still not applicable.
    pub async fn replay_unresolved(&self, id: i64) -> Result<NotificationOutcome, BillingError> {
        let mut uow = self.store.begin().await?;
        let row = uow.tx().unresolved(id, false).await;
        let _ = uow.rollback().await;
        let row = row?.ok_or(BillingError::NotFound)?;
        if row.gateway != "iyzico" || row.status != "held" {
            return Err(BillingError::InvalidRequest("only a held Iyzico notification can be replayed".into()));
        }
        let missing = || BillingError::Internal("held notification lacks identifiers".into());
        let notification = SubscriptionNotification {
            outcome: if row.event_type == "failure" { SubscriptionOutcome::Failure } else { SubscriptionOutcome::Success },
            raw_event_type: row.payload.get("raw_event_type").and_then(Value::as_str).unwrap_or_default().to_string(),
            order_ref: row.order_ref.clone().ok_or_else(missing)?,
            subscription_ref: row.subscription_ref.clone().ok_or_else(missing)?,
            customer_ref: row.customer_ref.clone().ok_or_else(missing)?,
        };
        self.process_notification(&notification, &row.environment, Some(id), Some(row.received_at)).await
    }

    /// An admin identifies the initial order of a subscription. The claim is
    /// checked against the provider's own subscription detail before it is
    /// stored, and it can never be changed afterwards. Held notifications for
    /// the subscription are then replayed: the initial order is a no-op, later
    /// distinct orders grant once.
    pub async fn resolve_first_order(
        &self,
        admin: &str,
        subscription_ref: &str,
        order_ref: &str,
        rationale: &str,
    ) -> Result<ResolveView, BillingError> {
        if admin.trim().is_empty() || rationale.trim().is_empty() || order_ref.trim().is_empty() {
            return Err(BillingError::InvalidRequest("an order reference and a rationale are required".into()));
        }
        let gateway = self
            .subscription_gateway
            .as_ref()
            .ok_or_else(|| BillingError::NotConfigured("the Iyzico subscription API is not configured".into()))?;
        let detail = gateway.detail(subscription_ref).await.map_err(BillingError::from)?;
        if detail.subscription_ref != subscription_ref {
            return Err(BillingError::Uncertain("the provider answered for another subscription".into()));
        }
        if !detail.order_refs.iter().any(|o| o == order_ref) {
            return Err(BillingError::InvalidRequest(
                "the provider does not list that order for this subscription".into(),
            ));
        }
        let environment = gateway.environment().to_string();
        let mut uow = self.store.begin().await?;
        let already = self
            .resolve_in(uow.tx(), &environment, subscription_ref, order_ref, admin, rationale, detail.customer_ref.as_deref())
            .await;
        let already = finish(uow, already).await?;

        let held = self.list_unresolved(Some("held")).await?;
        let mut replayed = 0;
        for row in held {
            if row.gateway == "iyzico" && row.environment == environment && row.subscription_ref.as_deref() == Some(subscription_ref) && let Ok(NotificationOutcome::Applied { .. } | NotificationOutcome::Ignored { .. }) =
                    self.replay_unresolved(row.id).await {
                replayed += 1;
            }
        }
        Ok(ResolveView { already_resolved: already, replayed })
    }

    #[allow(clippy::too_many_arguments)]
    async fn resolve_in(
        &self,
        tx: &mut dyn BillingTx,
        environment: &str,
        subscription_ref: &str,
        order_ref: &str,
        admin: &str,
        rationale: &str,
        provider_customer: Option<&str>,
    ) -> Result<bool, BillingError> {
        let found = tx
            .subscription("iyzico", environment, subscription_ref, false)
            .await?
            .ok_or(BillingError::NotFound)?;
        tx.lock_account(found.account).await?;
        let sub = tx
            .subscription("iyzico", environment, subscription_ref, true)
            .await?
            .ok_or(BillingError::NotFound)?;
        // Fail closed: the provider must name the customer and it must be the bound one.
        match (provider_customer, sub.customer_ref.as_deref()) {
            (Some(a), Some(b)) if a == b => {}
            (Some(_), Some(_)) => {
                return Err(BillingError::Conflict("the provider's customer differs from the stored binding".into()));
            }
            _ => {
                return Err(BillingError::Uncertain(
                    "the provider's subscription detail does not name the bound customer, so the order cannot be verified".into(),
                ));
            }
        }
        match sub.first_order_ref.as_deref() {
            Some(existing) if existing == order_ref => Ok(true),
            Some(_) => Err(BillingError::Conflict("the initial order was already identified as another reference".into())),
            None => {
                if !tx.set_first_order_ref(sub.id, order_ref, admin, rationale).await? {
                    return Err(BillingError::Conflict("the initial order was identified concurrently".into()));
                }
                tx.emit(
                    &BillingEvent::FirstOrderResolved {
                        subscription_ref: subscription_ref.to_string(),
                        order_ref: order_ref.to_string(),
                        admin: admin.to_string(),
                    },
                    self.now(),
                )
                .await?;
                Ok(false)
            }
        }
    }

    // ---- erasure: verified checkout closure (D10) ---------------------------

    /// An operator's verified closure of a plan checkout that keeps an account's
    /// erasure pending: the form may still create a subscription and no local
    /// status proves it never will. Not a risk waiver: the closure must attest,
    /// from retained provider evidence, that the checkout cannot complete and
    /// that no subscription from it renews. Only for an account under erasure
    /// and a plan checkout no target of that account covers. Nothing is granted,
    /// completed or bound.
    pub async fn resolve_erasure_checkout(&self, payment_id: i64, closure: &CheckoutClosure) -> Result<ErasureCheckoutView, BillingError> {
        if let Some(why) = closure.refusal() {
            return Err(BillingError::InvalidRequest(why.into()));
        }
        let closure = CheckoutClosure {
            resolved_by: closure.resolved_by.trim().to_string(),
            evidence_ref: closure.evidence_ref.trim().to_string(),
            rationale: closure.rationale.trim().to_string(),
            ..closure.clone()
        };
        let mut uow = self.store.begin().await?;
        let found = uow.tx().payment(payment_id, false).await;
        let _ = uow.rollback().await;
        let payment = found?.ok_or(BillingError::NotFound)?;
        let account = payment.account;
        let now = self.now();
        let mut uow = self.store.begin().await?;
        let result = async {
            let tx = uow.tx();
            tx.lock_account_for_erasure(account).await?;
            let open = tx.erasure_status(account).await?.filter(|s| s.billing_erased_at.is_none());
            if open.is_none_or(|s| !s.unbound_plan_payments.contains(&payment_id)) {
                return Err(BillingError::Conflict(
                    "only an unresolved plan checkout of an account under erasure can be closed".into(),
                ));
            }
            if !tx.resolve_erasure_checkout(account, payment_id, &closure, now).await? {
                return Err(BillingError::Conflict("the checkout was closed or covered concurrently".into()));
            }
            let status = tx.erasure_status(account).await?.ok_or_else(|| internal("the erasure barrier disappeared"))?;
            Ok(ErasureCheckoutView {
                payment_id,
                billing_blockers_cleared: status.subscriptions_stopped(),
                pending_subscriptions: status.pending.len(),
                remaining_unbound: status.unbound_plan_payments,
            })
        }
        .await;
        finish(uow, result).await
    }

    // ---- LemonSqueezy orders ----------------------------------------------

    pub async fn process_order_webhook(&self, headers: &Headers, body: &[u8]) -> Result<NotificationOutcome, BillingError> {
        let hook = self
            .order_webhook
            .as_ref()
            .ok_or_else(|| BillingError::NotConfigured("the LemonSqueezy webhook verifier is not configured".into()))?
            .clone();
        let order = match hook.verify(headers, body).await {
            Ok(o) => o,
            Err(WebhookError::Unsupported(reason)) => return Ok(NotificationOutcome::Ignored { reason }),
            Err(e) => return Err(e.into()),
        };
        if order.event_name != "order_created" || !order.status.eq_ignore_ascii_case("paid") {
            return Ok(NotificationOutcome::Ignored { reason: format!("not a paid order event: {}", order.event_name) });
        }
        let mut uow = self.store.begin().await?;
        let first = self.order_in(uow.tx(), &*hook, &order).await;
        match first {
            Ok(outcome) => {
                finish(uow, Ok(outcome)).await
            }
            Err(BillingError::Conflict(_)) => {
                // A unique identity refused the grant (the order id belongs to
                // another payment): keep nothing from this attempt, hold it.
                let _ = uow.rollback().await;
                let mut uow = self.store.begin().await?;
                let held = self.hold_order(uow.tx(), &order, reasons::CONFLICTING_ORDER).await;
                finish(uow, held).await
            }
            Err(e) => {
                let _ = uow.rollback().await;
                Err(e)
            }
        }
    }

    async fn hold_order(&self, tx: &mut dyn BillingTx, order: &OrderNotification, reason: &str) -> Result<NotificationOutcome, BillingError> {
        let held = NewUnresolved {
            gateway: "lemonsqueezy".into(),
            environment: if order.test_mode { "test".into() } else { "live".into() },
            event_type: "success".into(),
            subscription_ref: None,
            order_ref: Some(order.order_id.clone()),
            customer_ref: None,
            payment_id: order.payment_id,
            reason: reason.to_string(),
            payload: json!({
                "order_id": order.order_id,
                "store_id": order.store_id,
                "variant_id": order.variant_id,
                "quantity": order.quantity,
                "currency": order.currency,
                "subtotal_cents": order.subtotal_cents,
                "discount_cents": order.discount_cents,
                "tax_cents": order.tax_cents,
                "total_cents": order.total_cents,
                "tax_inclusive": order.tax_inclusive,
                "test_mode": order.test_mode,
                "payment_id": order.payment_id,
                "account_id": order.account_id,
            }),
            received_at: self.now(),
        };
        let (row, inserted) = tx.hold_notification(&held).await?;
        if inserted {
            tx.emit(
                &BillingEvent::NotificationHeld {
                    gateway: "lemonsqueezy".into(),
                    reason: reason.to_string(),
                    order_ref: Some(order.order_id.clone()),
                    subscription_ref: None,
                },
                self.now(),
            )
            .await?;
        }
        Ok(NotificationOutcome::Held { reason: reason.to_string(), held_id: row.id })
    }

    async fn order_in(
        &self,
        tx: &mut dyn BillingTx,
        hook: &dyn OrderWebhook,
        order: &OrderNotification,
    ) -> Result<NotificationOutcome, BillingError> {
        let Some(payment_id) = order.payment_id else {
            return self.hold_order(tx, order, reasons::PAYMENT_UNKNOWN).await;
        };
        let Some(found) = tx.payment(payment_id, false).await? else {
            return self.hold_order(tx, order, reasons::PAYMENT_UNKNOWN).await;
        };
        tx.lock_account(found.account).await?;
        let payment = tx.payment(payment_id, true).await?.ok_or(BillingError::NotFound)?;
        if payment.gateway != "lemonsqueezy" {
            return self.hold_order(tx, order, reasons::PAYMENT_UNKNOWN).await;
        }
        if order.account_id.as_deref() != Some(payment.account.to_string().as_str()) {
            return self.hold_order(tx, order, reasons::ACCOUNT_MISMATCH).await;
        }
        let environment = if order.test_mode { "test" } else { "live" };
        if payment.environment != environment || hook.environment() != environment {
            return self.hold_order(tx, order, reasons::ENVIRONMENT_MISMATCH).await;
        }
        if order.store_id != hook.expected_store() {
            return self.hold_order(tx, order, reasons::STORE_MISMATCH).await;
        }
        if order.variant_id != hook.expected_variant() {
            return self.hold_order(tx, order, reasons::VARIANT_MISMATCH).await;
        }
        if order.quantity != 1 {
            return self.hold_order(tx, order, reasons::QUANTITY_MISMATCH).await;
        }
        if !order.currency.eq_ignore_ascii_case(&payment.currency) {
            return self.hold_order(tx, order, reasons::CURRENCY_MISMATCH).await;
        }
        // The item price LemonSqueezy charges is the custom price we set, in both tax
        // modes. Money must be internally consistent for the order's tax mode, and the
        // quote is compared to the pre-tax net, never to the total.
        let net = match order_net(order.subtotal_cents, order.discount_cents, order.tax_cents, order.total_cents, order.tax_inclusive) {
            Ok(net) => net,
            Err(OrderAmountProblem::Invalid) => return self.hold_order(tx, order, reasons::AMOUNT_INVALID).await,
            Err(OrderAmountProblem::DiscountPresent) => return self.hold_order(tx, order, reasons::DISCOUNT_PRESENT).await,
            Err(OrderAmountProblem::TotalMismatch) => return self.hold_order(tx, order, reasons::AMOUNT_MISMATCH).await,
        };
        if payment.amount.to_minor_units() != Some(net) {
            return self.hold_order(tx, order, reasons::AMOUNT_MISMATCH).await;
        }
        if let Some(existing) = &payment.provider_order_ref {
            if existing == &order.order_id {
                return Ok(NotificationOutcome::Ignored { reason: "duplicate_order".into() });
            }
            return self.hold_order(tx, order, reasons::CONFLICTING_ORDER).await;
        }
        let Transition::Apply { to, grants } = transition(payment.status, Observation::Completed) else {
            return self.hold_order(tx, order, reasons::CONFLICTING_ORDER).await;
        };
        if !tx.set_payment_status(payment.id, payment.status, to).await? {
            return Err(BillingError::Conflict("the payment changed concurrently".into()));
        }
        let observed = json!({
            "subtotal_cents": order.subtotal_cents,
            "discount_cents": order.discount_cents,
            "tax_cents": order.tax_cents,
            "total_cents": order.total_cents,
            "currency": order.currency,
            "tax_inclusive": order.tax_inclusive,
            "store_id": order.store_id,
            "variant_id": order.variant_id,
            "test_mode": order.test_mode,
        });
        let update = PaymentUpdate {
            provider_order_ref: Some(order.order_id.clone()),
            observed: Some(observed.clone()),
            receipt_url: order.receipt_url.clone(),
            ..PaymentUpdate::default()
        };
        tx.update_payment(payment.id, &update).await?;
        let mut updated = payment.clone();
        updated.status = to;
        updated.provider_order_ref = Some(order.order_id.clone());
        updated.observed = Some(observed);
        updated.receipt_url = order.receipt_url.clone();
        if grants {
            self.grant_packages(tx, &updated).await?;
            self.emit_completed(tx, &updated).await?;
        }
        Ok(NotificationOutcome::Applied { payment_id: payment.id, status: to, changed: true })
    }

    // ---- history, cancellation, admin --------------------------------------

    pub async fn history(&self, account: AccountId) -> Result<Vec<HistoryRow>, BillingError> {
        self.ensure_provisioned(account).await?;
        let mut uow = self.store.begin().await?;
        let payments = uow.tx().completed_payments(account).await;
        let _ = uow.rollback().await;
        Ok(payments?.iter().map(history_row).collect())
    }

    /// The stored status of the caller's own payment: what a provider redirect that
    /// carries no token asks for. Pending until a verified notification completes it;
    /// reading it never calls a provider and never grants. Another account's payment
    /// looks exactly like a missing one.
    pub async fn payment_status(&self, account: AccountId, payment_id: i64) -> Result<PaymentStatusView, BillingError> {
        self.ensure_provisioned(account).await?;
        let mut uow = self.store.begin().await?;
        let payment = uow.tx().payment(payment_id, false).await;
        let _ = uow.rollback().await;
        let payment = payment?.filter(|p| p.account == account).ok_or(BillingError::NotFound)?;
        Ok(PaymentStatusView { payment_id: payment.id, gateway: payment.gateway, status: payment.status })
    }

    /// Records a cancellation request for the caller's own payment and queues
    /// the support email. The provider is not called: stopping the charges
    /// remains a manual step (DEF-006, awaiting mu's decision).
    pub async fn cancel_subscription(&self, account: AccountId, payment_id: i64) -> Result<CancelView, BillingError> {
        self.ensure_provisioned(account).await?;
        let mut uow = self.store.begin().await?;
        let result = self.cancel_in(uow.tx(), account, payment_id).await;
        finish(uow, result).await
    }

    async fn cancel_in(&self, tx: &mut dyn BillingTx, account: AccountId, payment_id: i64) -> Result<CancelView, BillingError> {
        tx.lock_account(account).await?;
        let payment = tx.payment(payment_id, true).await?;
        // Another account's payment looks exactly like a missing one.
        let payment = payment.filter(|p| p.account == account).ok_or(BillingError::NotFound)?;
        let now = self.now();
        tx.stamp_cancellation(payment.id, now).await?;
        if let Some(sub_ref) = &payment.subscription_ref {
            tx.stamp_subscription_cancel(&payment.gateway, &payment.environment, sub_ref, now).await?;
        }
        tx.emit(&BillingEvent::CancelRequested { payment_id: payment.id, account }, now).await?;
        Ok(CancelView { payment_id: payment.id, canceled_at: now })
    }

    /// Admin lookup of another customer by email: the real customer's
    /// completed payments and latest billing info.
    pub async fn admin_customer_invoices(&self, email: &str) -> Result<AdminLookup, BillingError> {
        let customer = self.directory.by_email(email).await?.ok_or(BillingError::NotFound)?;
        let mut uow = self.store.begin().await?;
        let payments = uow.tx().completed_payments(customer.account).await;
        let info = uow.tx().latest_billing_info(customer.account).await;
        let _ = uow.rollback().await;
        let payments = payments?;
        let billing_info = info?;
        let history = payments.iter().map(history_row).collect();
        let invoice_context = crate::presentation::invoice::invoice_context(&customer, billing_info.as_ref(), &payments);
        Ok(AdminLookup { customer, payments, history, billing_info, invoice_context })
    }

    /// The settings Subscription tab's facts (WEB-BIL-022/023): the open Premium or
    /// Business package and, when it is the caller's, the payment that bought it.
    pub async fn subscription(&self, account: AccountId) -> Result<Option<crate::presentation::subscription::SubscriptionFacts>, BillingError> {
        self.ensure_provisioned(account).await?;
        let now = self.now();
        let mut uow = self.store.begin().await?;
        let read = async {
            let all = uow.tx().packages(account).await?;
            let open: Vec<crate::ledger::Package> = open_at(&all, now).iter().map(|p| p.package.clone()).collect();
            let plan_payment = {
                let mut ordered: Vec<&crate::ledger::Package> = open.iter().collect();
                ordered.sort_by_key(|p| p.id);
                ordered.into_iter().find(|p| matches!(p.name.as_deref(), Some("PREMIUM") | Some("BUSINESS"))).and_then(|p| p.payment_id)
            };
            let payment = match plan_payment {
                Some(id) => uow.tx().payment(id, false).await?,
                None => None,
            };
            Ok::<_, BillingError>(crate::presentation::subscription::subscription_facts(&open, |_| payment))
        }
        .await;
        let _ = uow.rollback().await;
        read
    }

    /// The caller's own invoice context (P5 D7): Bill To identity and the facts of
    /// every completed payment, all derived here, never by a client.
    pub async fn invoice_context(&self, account: AccountId) -> Result<crate::presentation::invoice::InvoiceContext, BillingError> {
        self.ensure_provisioned(account).await?;
        let customer = self.customer(account).await?;
        let mut uow = self.store.begin().await?;
        let payments = uow.tx().completed_payments(account).await;
        let info = uow.tx().latest_billing_info(account).await;
        let _ = uow.rollback().await;
        Ok(crate::presentation::invoice::invoice_context(&customer, info?.as_ref(), &payments?))
    }

    pub async fn coupon_status(&self, id: &str) -> Result<Option<Coupon>, BillingError> {
        let mut uow = self.store.begin().await?;
        let coupon = uow.tx().coupon(id).await;
        let _ = uow.rollback().await;
        Ok(coupon?)
    }

    // ---- outbox relay ------------------------------------------------------

    /// Delivers committed events: claim in a short transaction, deliver with
    /// no lock held, acknowledge. At least once, never exactly once.
    pub async fn relay_outbox(&self, limit: i64, lease_seconds: i64) -> Result<RelayReport, BillingError> {
        if self.mailer.is_none() && self.analytics.is_none() {
            return Err(BillingError::NotConfigured("neither mail nor analytics delivery is configured".into()));
        }
        let events = self.store.claim_outbox(limit, lease_seconds, self.now()).await?;
        let mut report = RelayReport { claimed: events.len(), ..RelayReport::default() };
        for event in &events {
            match self.deliver(event).await {
                Ok(()) => {
                    if self.store.ack_outbox(event, self.now()).await? {
                        report.acknowledged += 1;
                    }
                }
                Err(why) => {
                    let backoff = 30i64 * (1i64 << event.attempts.clamp(0, 8));
                    self.store.release_outbox(event, &why, self.now().plus_secs(backoff)).await?;
                    report.retried += 1;
                }
            }
        }
        Ok(report)
    }

    /// Every delivery of one event has its own durable mark, so a retry after a partial
    /// failure sends only what is still missing, with the same stable key.
    async fn deliver(&self, event: &crate::model::OutboxEvent) -> Result<(), String> {
        // Each recipient and each analytics event is its own delivery with its own durable mark.
        // One failing destination never withholds the others; the event is retried until every
        // delivery has succeeded, and a retry sends only what is still missing.
        let done = self.store.delivered_targets(event.event_id).await.map_err(|e| e.to_string())?;
        let mut failure: Option<String> = None;
        let mut note = |why: String| {
            failure.get_or_insert(why);
        };

        match plan_deliveries(&event.event, &*self.directory, &self.config.notify).await {
            Err(e) => note(e.to_string()),
            Ok(deliveries) => {
                for delivery in deliveries {
                    if done.iter().any(|t| t == delivery.target) {
                        continue;
                    }
                    let Some(mailer) = self.mailer.as_ref() else {
                        note("mail delivery is not configured".to_string());
                        continue;
                    };
                    let key = format!("{}:{}", event.event_id, delivery.target);
                    match mailer.send(&delivery.message, &key).await {
                        Ok(()) => {
                            if let Err(e) = self.store.record_delivery(event.event_id, delivery.target, self.now()).await {
                                note(e.to_string());
                            }
                        }
                        Err(e) => note(e.to_string()),
                    }
                }
            }
        }

        if let Some(sink) = &self.analytics {
            match plan_analytics(&event.event, event.occurred_at) {
                Err(e) => note(e),
                Ok(planned) => {
                    for analytics in planned {
                        let target = analytics.target();
                        if done.contains(&target) {
                            continue;
                        }
                        let key = format!("{}:{}", event.event_id, target);
                        match sink.send(&analytics, &key).await {
                            Ok(()) => {
                                if let Err(e) = self.store.record_delivery(event.event_id, &target, self.now()).await {
                                    note(e.to_string());
                                }
                            }
                            Err(e) => note(e.to_string()),
                        }
                    }
                }
            }
        }
        match failure {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }
}

// ---- host transaction seam: job charging ------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChargeError {
    Refused(StartRefusal),
    Store(StoreError),
}

impl From<StoreError> for ChargeError {
    fn from(e: StoreError) -> Self {
        ChargeError::Store(e)
    }
}

impl From<StartRefusal> for ChargeError {
    fn from(e: StartRefusal) -> Self {
        ChargeError::Refused(e)
    }
}

impl fmt::Display for ChargeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChargeError::Refused(r) => write!(f, "{}", r.message(false)),
            ChargeError::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ChargeError {}

fn usage_rows(account: AccountId, subject: SubjectRef, allocations: &[Allocation], at: Micros) -> Vec<NewUsage> {
    allocations
        .iter()
        .map(|a| NewUsage { account, package_id: a.package_id, subject: Some(subject), minutes: a.minutes, used_at: at })
        .collect()
}

/// Charges a batch of job starts inside a transaction the host opened.
///
/// The host must have called `lock_account` before it read its own rows, and
/// must have computed each job's `other_completed_jobs` after that lock. Jobs
/// are decided and written in ascending subject id. Any refusal returns an
/// error and writes nothing; the host then rolls back its own status changes
/// in the same transaction.
pub async fn charge_job_starts(
    tx: &mut dyn BillingTx,
    account: AccountId,
    jobs: &[JobStart],
    at: Micros,
) -> Result<Vec<(SubjectRef, StartCharge)>, ChargeError> {
    tx.lock_account(account).await?;
    let all = tx.packages(account).await?;
    let open = open_at(&all, at);
    let plan = plan_batch(jobs, &open)?;
    let mut rows = Vec::new();
    for (subject, charge) in &plan {
        match charge {
            StartCharge::Strict(allocations) => rows.extend(usage_rows(account, *subject, allocations, at)),
            StartCharge::Bypass(allocation) => rows.extend(usage_rows(account, *subject, std::slice::from_ref(allocation), at)),
            StartCharge::Deferred => {}
        }
    }
    if !rows.is_empty() {
        tx.insert_usage(&rows).await?;
    }
    Ok(plan)
}

/// A strict debit of known minutes: the deferred provider charge and any
/// host operation that must never overdraw. Never uses the first-job bypass.
pub async fn charge_strict(
    tx: &mut dyn BillingTx,
    account: AccountId,
    subject: SubjectRef,
    minutes: i64,
    at: Micros,
) -> Result<Vec<Allocation>, ChargeError> {
    tx.lock_account(account).await?;
    let all = tx.packages(account).await?;
    let open = open_at(&all, at);
    let allocations = decide_deferred(minutes, &open)?;
    tx.insert_usage(&usage_rows(account, subject, &allocations, at)).await?;
    Ok(allocations)
}

/// BIL-074: the answer to a started job, read inside the debiting transaction after the
/// debit so the balance and freemium flag are those the account really has now. Hosts call
/// it only when they build that answer; charging itself reads nothing extra.
pub async fn start_answer(tx: &mut dyn BillingTx, account: AccountId, at: Micros) -> Result<StartAnswerMeta, StoreError> {
    let all = tx.packages(account).await?;
    Ok(start_answer_meta(&all, at))
}

/// Hard delete of an authorized subject returns its minutes (BIL-042), including
/// imported consumption that drew from another account's package. Failed,
/// canceled and soft-deleted work keeps its charge; there is no refund path.
///
/// This must be the first billing call of the host's deletion transaction, before
/// any other account lock: every account whose package the subject's imported usage
/// drew from is locked together with `account`, in ascending account id, so two
/// concurrent deletes and a concurrent debit by an affected owner serialize without
/// deadlock. If the set of affected owners changed between discovery and locking,
/// the delete refuses with a conflict instead of changing an unlocked balance.
pub async fn release_subject_charges(
    tx: &mut dyn BillingTx,
    account: AccountId,
    subject: SubjectRef,
) -> Result<u64, ChargeError> {
    let owners = tx.imported_release_owners(account, subject).await?;
    for owner in &owners {
        tx.lock_account(*owner).await?;
    }
    let locked = tx.imported_release_owners(account, subject).await?;
    if locked != owners {
        return Err(ChargeError::Store(StoreError::Conflict(
            "the imported usage of this subject changed while its owners were being locked; retry".into(),
        )));
    }
    Ok(tx.release_subject(account, subject).await?)
}
