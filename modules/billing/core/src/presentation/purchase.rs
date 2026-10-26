//! The purchase modal and sheet as a pure state machine (WEB-BIL-001..014,
//! CPN-005..013, P2 D2, P5 D8). Shells send events, run the effects and render
//! the view; no price, coupon or hours rule lives in a shell. The displayed total
//! and the charged quote stay two separate facts from the server.

use serde::{Deserialize, Serialize};

use crate::decimal::{Decimal, Rounding};
use crate::pricing::{
    CouponField, CouponLookup, CouponMessage, Currency, DEFAULT_HOURS, HOUR_PRESETS, PlanInterval, Quote,
    currency_for_locale, normalize_hours, step_hours,
};
use crate::presentation::{RenewalNotice, renewal_notice};
use crate::types::Micros;

/// D17: the checkout document POST is one urlencoded form of `order`
/// (base64url without padding of the [`OrderBody`] JSON) and `access_token`.
/// The client refuses to build, and the server refuses to read, anything larger.
pub const CHECKOUT_FORM_MAX_BYTES: usize = 32 * 1024;
pub const CHECKOUT_TOKEN_MAX_BYTES: usize = 8 * 1024;
/// The decoded `order` JSON.
pub const CHECKOUT_ORDER_MAX_BYTES: usize = 16 * 1024;

/// One plan card as `GET /api/billing/catalog` lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlanCardView {
    pub name: String,
    pub interval: PlanInterval,
    pub usd_per_month: Decimal,
    pub struck_per_month: Decimal,
    pub badge: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogPlan {
    pub package_id: usize,
    pub card: PlanCardView,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PaygCardView {
    pub price_text: String,
    pub struck_per_hour: Decimal,
}

/// The `GET /api/billing/catalog` body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogView {
    pub plans: Vec<CatalogPlan>,
    pub pay_as_you_go: Option<PaygCardView>,
}

/// The `POST /api/billing/quote` body: the charged quote plus the shown total.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuoteView {
    #[serde(flatten)]
    pub quote: Quote,
    pub display_amount: Decimal,
}

/// The coupon found by `GET /api/billing/coupon/{id}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CouponReplyCoupon {
    pub id: String,
    pub amount: Option<Decimal>,
    pub product: Option<String>,
}

/// The `GET /api/billing/coupon/{id}` body (its English `message` is ignored).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CouponReply {
    pub valid: bool,
    pub notice: Option<CouponMessage>,
    pub coupon: Option<CouponReplyCoupon>,
}

impl CouponReply {
    /// The server already decided validity and expiry; this only carries its answer.
    pub fn lookup(self) -> CouponLookup {
        match (self.valid, self.coupon, self.notice) {
            (true, Some(c), _) => CouponLookup::Found(crate::pricing::Coupon {
                id: c.id,
                amount: c.amount,
                expires_at: None,
                filter_product: c.product,
                discount_type: None,
            }),
            (_, _, Some(CouponMessage::Expired)) => CouponLookup::Expired,
            (_, _, Some(CouponMessage::Error)) => CouponLookup::Failed,
            _ => CouponLookup::NotFound,
        }
    }
}

/// The exact JSON `/api/billing/quote` and `/api/billing/checkout` accept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderBody {
    pub package_id: Option<usize>,
    pub pay_as_you_go_hours: Option<i64>,
    pub currency: String,
    pub locale: String,
    pub coupon_id: Option<String>,
    pub gateway: String,
    /// The shell's attribution object (`utm_*`, `referrer`, `device_type`, `browser`),
    /// set only on a checkout. The server keeps an object and drops anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics: Option<serde_json::Value>,
}

/// A refused or failed request, as the shell maps the HTTP outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum PurchaseFailure {
    /// 401: sign in again.
    Unauthenticated,
    /// 410: the account is under erasure.
    Erased,
    /// The request could not be completed (network, 5xx, provider start failure).
    Unavailable,
    /// A 4xx the server explains with its error code (for example `hours_out_of_range`).
    Refused { code: String },
}

/// The checkout-started analytics intent (source `paymentEvents.checkoutStarted`).
/// As in the source, the amount is the shown total and the currency follows the locale.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckoutStarted {
    pub plan: String,
    pub amount: Decimal,
    pub currency: Currency,
    pub billing_cycle: Option<PlanInterval>,
}

impl CheckoutStarted {
    /// The source event name (`paymentEvents.checkoutStarted`).
    pub const EVENT: &'static str = "checkout_started";

    /// The source properties this intent decides. The shell adds its attribution
    /// (`referrer`, `utm_*`, `device_type`, `browser`) and sends it only with consent.
    pub fn properties(&self) -> serde_json::Value {
        serde_json::json!({
            "plan": self.plan,
            "amount": self.amount.to_string().parse::<f64>().ok(),
            "currency": self.currency.as_str(),
            "billing_cycle": self.billing_cycle.map(|c| match c {
                PlanInterval::Monthly => "monthly",
                PlanInterval::Yearly => "yearly",
            }),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum PurchaseEffect {
    /// `request` orders the answers: only the latest one is applied.
    FetchQuote { request: u64, order: OrderBody },
    FetchCoupon(String),
    /// D17: the shell posts one hidden same-origin form to `POST /checkout` (fields
    /// `order` = base64url of this JSON, `access_token`) and the page leaves for the
    /// checkout document. No checkout API call precedes it.
    SubmitCheckout(OrderBody),
    Analytics(CheckoutStarted),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PurchaseEvent {
    SelectPayAsYouGo,
    /// A plan card was clicked.
    SelectPlan { package_id: usize },
    /// The plan section header was clicked: keeps a selected plan, else restores
    /// the last plan kind (Premium by default) for the current interval.
    PlanSectionClicked,
    /// The Monthly or Yearly tab: selects the same plan kind in that interval.
    IntervalChosen { yearly: bool },
    PresetHours(i64),
    HoursTyped(String),
    StepHours(i64),
    CouponTyped(String),
    CouponApplied,
    CouponAnswered(CouponLookup),
    TermsAccepted(bool),
    QuoteAnswered { request: u64, result: Result<QuoteView, PurchaseFailure> },
    PayRequested,
    /// The shell could not post the form (for example no session); nothing was sent.
    CheckoutNotSubmitted(PurchaseFailure),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlanOption {
    pub package_id: usize,
    pub card: PlanCardView,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChargedQuote {
    pub amount: Decimal,
    pub currency: Currency,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PurchaseVm {
    /// The Premium and Business cards of the chosen interval.
    pub plans: Vec<PlanOption>,
    pub pay_as_you_go: Option<PaygCardView>,
    pub pay_as_you_go_selected: bool,
    pub yearly: bool,
    pub hours: i64,
    pub hour_presets: [i64; 3],
    pub minimum_warning: bool,
    pub coupon_code: String,
    pub coupon_locked: bool,
    pub coupon_notice: Option<CouponMessage>,
    pub terms_accepted: bool,
    /// What the summary shows (`display_amount`), in USD.
    pub display_total: Option<Decimal>,
    /// What the provider will be asked to charge. Never derived from the shown total.
    pub charged_quote: Option<ChargedQuote>,
    /// The Turkish locale's `= TL` line: the shown total times the quote's rate.
    pub try_display: Option<Decimal>,
    pub renewal: RenewalNotice,
    pub quote_pending: bool,
    pub processing: bool,
    pub can_pay: bool,
    pub failure: Option<PurchaseFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    PayAsYouGo,
    Plan(usize),
}

pub struct PurchaseState {
    catalog: CatalogView,
    ui_locale: String,
    selection: Selection,
    business_last: bool,
    yearly: bool,
    hours: i64,
    minimum_warning: bool,
    coupon_code: String,
    coupon: CouponField,
    terms_accepted: bool,
    quote_request: u64,
    quote: Option<QuoteView>,
    quote_pending: bool,
    processing: bool,
    failure: Option<PurchaseFailure>,
}

impl PurchaseState {
    /// Opening the modal: pay as you go, 10 hours, yearly tab, terms unaccepted.
    /// Returns the state and the first quote request.
    pub fn new(catalog: CatalogView, ui_locale: &str) -> (PurchaseState, Vec<PurchaseEffect>) {
        let mut state = PurchaseState {
            catalog,
            ui_locale: ui_locale.to_string(),
            selection: Selection::PayAsYouGo,
            business_last: false,
            yearly: true,
            hours: DEFAULT_HOURS,
            minimum_warning: false,
            coupon_code: String::new(),
            coupon: CouponField::default(),
            terms_accepted: false,
            quote_request: 0,
            quote: None,
            quote_pending: false,
            processing: false,
            failure: None,
        };
        let effects = state.requote();
        (state, effects)
    }

    fn plan(&self, package_id: usize) -> Option<&CatalogPlan> {
        self.catalog.plans.iter().find(|p| p.package_id == package_id)
    }

    fn is_business(&self, package_id: usize) -> bool {
        self.plan(package_id).is_some_and(|p| p.card.name.eq_ignore_ascii_case("business"))
    }

    /// The Premium or Business plan of an interval, by catalog card.
    fn plan_of(&self, business: bool, yearly: bool) -> Option<usize> {
        let interval = if yearly { PlanInterval::Yearly } else { PlanInterval::Monthly };
        self.catalog
            .plans
            .iter()
            .find(|p| p.card.interval == interval && p.card.name.eq_ignore_ascii_case(if business { "business" } else { "premium" }))
            .map(|p| p.package_id)
    }

    fn select_plan(&mut self, package_id: usize) {
        if self.plan(package_id).is_some() {
            self.business_last = self.is_business(package_id);
            self.selection = Selection::Plan(package_id);
        }
    }

    fn order(&self) -> OrderBody {
        let (package_id, hours) = match self.selection {
            Selection::PayAsYouGo => (None, Some(self.hours)),
            Selection::Plan(id) => (Some(id), None),
        };
        OrderBody {
            package_id,
            pay_as_you_go_hours: hours,
            currency: currency_for_locale(&self.ui_locale).as_str().to_string(),
            locale: self.ui_locale.clone(),
            coupon_id: self.coupon.applied().map(|c| c.id.clone()),
            gateway: "iyzico".into(),
            analytics: None,
        }
    }

    /// Any change to what would be bought asks the server again; the shown
    /// quote stays until the newer answer arrives.
    fn requote(&mut self) -> Vec<PurchaseEffect> {
        self.quote_request += 1;
        self.quote_pending = true;
        vec![PurchaseEffect::FetchQuote { request: self.quote_request, order: self.order() }]
    }

    pub fn update(&mut self, event: PurchaseEvent, now: Micros) -> Vec<PurchaseEffect> {
        if self.processing && !matches!(event, PurchaseEvent::CheckoutNotSubmitted(_)) {
            return vec![];
        }
        match event {
            PurchaseEvent::SelectPayAsYouGo => {
                self.selection = Selection::PayAsYouGo;
                self.requote()
            }
            PurchaseEvent::SelectPlan { package_id } => {
                self.select_plan(package_id);
                self.requote()
            }
            PurchaseEvent::PlanSectionClicked => {
                if self.selection != Selection::PayAsYouGo {
                    return vec![];
                }
                if let Some(id) = self.plan_of(self.business_last, self.yearly) {
                    self.select_plan(id);
                }
                self.requote()
            }
            PurchaseEvent::IntervalChosen { yearly } => {
                let business = matches!(self.selection, Selection::Plan(id) if self.is_business(id));
                if let Some(id) = self.plan_of(business, yearly) {
                    self.select_plan(id);
                }
                self.yearly = yearly;
                self.requote()
            }
            PurchaseEvent::PresetHours(hours) => {
                self.hours = step_hours(hours, 0);
                self.minimum_warning = false;
                self.requote()
            }
            PurchaseEvent::HoursTyped(text) => {
                let input = normalize_hours(&text);
                self.hours = input.hours;
                self.minimum_warning = input.minimum_warning;
                self.requote()
            }
            PurchaseEvent::StepHours(delta) => {
                self.hours = step_hours(self.hours, delta);
                self.minimum_warning = false;
                self.requote()
            }
            PurchaseEvent::CouponTyped(text) => {
                if !self.coupon.is_locked() {
                    self.coupon_code = text;
                }
                vec![]
            }
            PurchaseEvent::CouponApplied => {
                let was_applied = self.coupon.applied().is_some();
                match self.coupon.submit(self.coupon_code.trim()) {
                    crate::pricing::CouponEffect::Fetch(code) => vec![PurchaseEffect::FetchCoupon(code)],
                    crate::pricing::CouponEffect::None if was_applied != self.coupon.applied().is_some() => self.requote(),
                    crate::pricing::CouponEffect::None => vec![],
                }
            }
            PurchaseEvent::CouponAnswered(result) => {
                self.coupon.lookup_result(result, now);
                self.requote()
            }
            PurchaseEvent::TermsAccepted(accepted) => {
                self.terms_accepted = accepted;
                vec![]
            }
            PurchaseEvent::QuoteAnswered { request, result } => {
                if request != self.quote_request {
                    return vec![];
                }
                self.quote_pending = false;
                match result {
                    Ok(quote) => {
                        self.quote = Some(quote);
                        self.failure = None;
                    }
                    Err(failure) => {
                        self.quote = None;
                        self.failure = Some(failure);
                    }
                }
                vec![]
            }
            PurchaseEvent::PayRequested => {
                let Some(display) = self.payable() else { return vec![] };
                self.processing = true;
                self.failure = None;
                let (plan, billing_cycle) = match self.selection {
                    Selection::PayAsYouGo => ("pay-as-you-go".to_string(), None),
                    Selection::Plan(id) => {
                        let card = &self.plan(id).expect("selected plans exist").card;
                        (card.name.to_lowercase(), Some(card.interval))
                    }
                };
                vec![
                    PurchaseEffect::Analytics(CheckoutStarted {
                        plan,
                        amount: display,
                        currency: currency_for_locale(&self.ui_locale),
                        billing_cycle,
                    }),
                    PurchaseEffect::SubmitCheckout(self.order()),
                ]
            }
            PurchaseEvent::CheckoutNotSubmitted(failure) => {
                if self.processing {
                    self.processing = false;
                    self.failure = Some(failure);
                }
                vec![]
            }
        }
    }

    /// The shown total when everything needed to pay is in place.
    fn payable(&self) -> Option<Decimal> {
        if !self.terms_accepted || self.quote_pending || self.processing {
            return None;
        }
        self.quote.as_ref().map(|q| q.display_amount)
    }

    pub fn view(&self) -> PurchaseVm {
        let interval = if self.yearly { PlanInterval::Yearly } else { PlanInterval::Monthly };
        let selected_interval = match self.selection {
            Selection::Plan(id) => self.plan(id).map(|p| p.card.interval),
            Selection::PayAsYouGo => None,
        };
        PurchaseVm {
            plans: self
                .catalog
                .plans
                .iter()
                .filter(|p| p.card.interval == interval)
                .map(|p| PlanOption {
                    package_id: p.package_id,
                    card: p.card.clone(),
                    selected: self.selection == Selection::Plan(p.package_id),
                })
                .collect(),
            pay_as_you_go: self.catalog.pay_as_you_go.clone(),
            pay_as_you_go_selected: self.selection == Selection::PayAsYouGo,
            yearly: self.yearly,
            hours: self.hours,
            hour_presets: HOUR_PRESETS,
            minimum_warning: self.minimum_warning,
            coupon_code: self.coupon_code.clone(),
            coupon_locked: self.coupon.is_locked(),
            coupon_notice: self.coupon.notice(),
            terms_accepted: self.terms_accepted,
            display_total: self.quote.as_ref().map(|q| q.display_amount),
            charged_quote: self.quote.as_ref().map(|q| ChargedQuote { amount: q.quote.amount, currency: q.quote.currency }),
            try_display: self
                .quote
                .as_ref()
                .filter(|_| currency_for_locale(&self.ui_locale) == Currency::Try)
                .and_then(|q| q.display_amount.mul(&q.quote.rate).and_then(|v| v.quantize(2, Rounding::HalfUp)).ok()),
            renewal: renewal_notice(selected_interval),
            quote_pending: self.quote_pending,
            processing: self.processing,
            can_pay: self.payable().is_some(),
            failure: self.failure.clone(),
        }
    }
}

#[cfg(test)]
#[path = "purchase_tests.rs"]
mod tests;
