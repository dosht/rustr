//! Catalog, coupons, currency and the quote of an order, plus the display
//! policies that read the same data.
//!
//! Decision D2: every stage is evaluated on exact decimals in the legacy
//! operation order. The locale discount floors to cents, the coupon rounds half
//! up to cents, the TRY conversion multiplies by the configured rate and the
//! provider-facing result is quantized to two decimals once. The legacy
//! binary-float result differs for a measured 3824 of 319840 input
//! combinations; those are price changes recorded in the decisions register,
//! not equivalence defects.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::decimal::{Decimal, DecimalError, Rounding};
use crate::grants::PackageSpec;
use crate::types::Micros;

pub const MIN_HOURS: i64 = 5;
pub const MAX_HOURS: i64 = 9999;
pub const DEFAULT_HOURS: i64 = 10;
pub const HOUR_PRESETS: [i64; 3] = [5, 10, 25];
pub const DEFAULT_RATE: i64 = 44;
/// The fixed yearly-plan badge (DEF-022 is a recorded display defect, kept).
pub const SAVE_BADGE: &str = "Save %10";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Currency {
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "TRY")]
    Try,
}

impl Currency {
    /// The request currency is upper-cased; anything but USD and TRY is unknown.
    pub fn parse(text: &str) -> Option<Currency> {
        match text.to_uppercase().as_str() {
            "USD" => Some(Currency::Usd),
            "TRY" => Some(Currency::Try),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Try => "TRY",
        }
    }
}

/// TRY exactly for the `tr` locale, USD for everything else (CPN-001).
pub fn currency_for_locale(locale: &str) -> Currency {
    if locale == "tr" { Currency::Try } else { Currency::Usd }
}

/// `USD_TRY_RATE` when it is a positive non-zero number, otherwise 44.
pub fn parse_rate(text: Option<&str>) -> Decimal {
    text.and_then(|t| Decimal::parse(t.trim()).ok())
        .filter(|r| !r.is_zero() && !r.is_negative())
        .unwrap_or(Decimal::new(DEFAULT_RATE as i128, 0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum PlanInterval {
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub name: String,
    pub interval: PlanInterval,
    pub spec: PackageSpec,
    pub usd_price: Decimal,
    /// Struck-through reference price in the same unit the card shows.
    pub struck_price: Decimal,
    pub plan_code_try: Option<String>,
    pub plan_code_usd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaygEntry {
    /// The name written into packages and matched by coupon filters.
    pub stored_name: String,
    /// The catalog constant's label, kept distinct (DEF-026).
    pub catalog_label: String,
    pub usd_per_hour: Decimal,
    pub struck_per_hour: Decimal,
    pub min_hours: i64,
    pub max_hours: i64,
    pub minutes_per_hour: i64,
}

/// Host data. No price, plan code or name is a constant of this crate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// The array index is the `packageId` the checkout form sends.
    pub plans: Vec<PlanEntry>,
    pub payg: Option<PaygEntry>,
    pub locale_discount_locales: BTreeSet<String>,
    pub special_offer_try_per_hour: Option<Decimal>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coupon {
    pub id: String,
    /// A fraction such as 0.25. Read through float4 shortest text, never float8.
    pub amount: Option<Decimal>,
    pub expires_at: Option<Micros>,
    pub filter_product: Option<String>,
    /// Stored, never read: every coupon is a percentage.
    pub discount_type: Option<String>,
}

impl Coupon {
    /// Strictly after `expires_at`; a null expiry never expires (CPN-011).
    pub fn is_expired(&self, now: Micros) -> bool {
        self.expires_at.is_some_and(|at| now > at)
    }

    pub fn applies_to(&self, product: &str) -> bool {
        self.filter_product.as_deref() == Some(product)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    Plan { index: usize },
    PayAsYouGo { hours: i64 },
}

pub struct OrderInput<'a> {
    pub product: Product,
    pub currency: Currency,
    pub ui_locale: &'a str,
    /// The coupon as read again on the server, never a client claim.
    pub coupon: Option<&'a Coupon>,
    pub special_offer: bool,
    pub rate: &'a Decimal,
    pub now: Micros,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub product_name: String,
    /// Two decimals, in `currency`. What the provider is asked to charge.
    pub amount: Decimal,
    pub currency: Currency,
    /// Set whenever an existing unexpired coupon was read, applied or not (CPN-013).
    pub coupon_id: Option<String>,
    pub discount_applied: bool,
    pub pricing_plan_id: Option<String>,
    pub spec: PackageSpec,
    pub hours: Option<i64>,
    pub usd_amount: Decimal,
    pub rate: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderError {
    UnknownPlan(usize),
    NoPlanCode,
    PayAsYouGoNotSold,
    HoursOutOfRange { hours: i64, min: i64, max: i64 },
    CouponAmountInvalid,
    Arithmetic(String),
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderError::UnknownPlan(i) => write!(f, "no package at index {i}"),
            OrderError::NoPlanCode => write!(f, "the plan has no code for that currency"),
            OrderError::PayAsYouGoNotSold => write!(f, "pay-as-you-go is not sold here"),
            OrderError::HoursOutOfRange { hours, min, max } => {
                write!(f, "{hours} hours is outside the whole hours {min} to {max}")
            }
            OrderError::CouponAmountInvalid => write!(f, "coupon amount must be a fraction between 0 and 1"),
            OrderError::Arithmetic(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OrderError {}

impl From<DecimalError> for OrderError {
    fn from(e: DecimalError) -> Self {
        OrderError::Arithmetic(e.to_string())
    }
}

fn convert(usd: &Decimal, currency: Currency, rate: &Decimal) -> Result<Decimal, OrderError> {
    match currency {
        Currency::Usd => Ok(usd.quantize(2, Rounding::HalfUp)?),
        Currency::Try => Ok(usd.mul(rate)?.quantize(2, Rounding::HalfUp)?),
    }
}

fn payg_entry(catalog: &Catalog, hours: i64) -> Result<&PaygEntry, OrderError> {
    let payg = catalog.payg.as_ref().ok_or(OrderError::PayAsYouGoNotSold)?;
    if hours < payg.min_hours || hours > payg.max_hours {
        return Err(OrderError::HoursOutOfRange { hours, min: payg.min_hours, max: payg.max_hours });
    }
    Ok(payg)
}

pub fn price_order(catalog: &Catalog, input: &OrderInput<'_>) -> Result<Quote, OrderError> {
    match input.product {
        Product::Plan { index } => {
            let plan = catalog.plans.get(index).ok_or(OrderError::UnknownPlan(index))?;
            let code = match input.currency {
                Currency::Usd => plan.plan_code_usd.clone(),
                Currency::Try => plan.plan_code_try.clone(),
            }
            .ok_or(OrderError::NoPlanCode)?;
            let usd = plan.usd_price.quantize(2, Rounding::HalfUp)?;
            Ok(Quote {
                product_name: plan.name.clone(),
                amount: convert(&usd, input.currency, input.rate)?,
                currency: input.currency,
                // Plans pass no product name, so no coupon ever reaches them.
                coupon_id: None,
                discount_applied: false,
                pricing_plan_id: Some(code),
                spec: plan.spec.clone(),
                hours: None,
                usd_amount: usd,
                rate: *input.rate,
            })
        }
        Product::PayAsYouGo { hours } => {
            let payg = payg_entry(catalog, hours)?;
            let spec = PackageSpec {
                name: payg.stored_name.clone(),
                minutes: hours * payg.minutes_per_hour,
                count: 1,
                interval: "year".into(),
                frequency: 1,
                expiry: 1,
            };
            let base = payg.usd_per_hour.mul_int(i128::from(hours))?.quantize(2, Rounding::HalfUp)?;
            if input.special_offer && let Some(per_hour) = &catalog.special_offer_try_per_hour {
                return Ok(Quote {
                    product_name: payg.stored_name.clone(),
                    amount: per_hour.mul_int(i128::from(hours))?.quantize(2, Rounding::HalfUp)?,
                    currency: Currency::Try,
                    // The source stores the coupon id even though the offer price ignores it (CPN-013).
                    coupon_id: input.coupon.filter(|c| !c.is_expired(input.now)).map(|c| c.id.clone()),
                    discount_applied: false,
                    pricing_plan_id: None,
                    spec,
                    hours: Some(hours),
                    usd_amount: base,
                    rate: *input.rate,
                });
            }
            let mut total = base;
            if catalog.locale_discount_locales.contains(input.ui_locale) {
                let half = total.mul(&Decimal::new(5, 1))?;
                total = total.sub(&half)?.quantize(2, Rounding::Floor)?;
            }
            let mut coupon_id = None;
            let mut discount_applied = false;
            if let Some(coupon) = input.coupon.filter(|c| !c.is_expired(input.now)) {
                coupon_id = Some(coupon.id.clone());
                if coupon.applies_to(&payg.stored_name) {
                    let fraction = coupon.amount.unwrap_or(Decimal::zero());
                    if fraction.is_negative() || fraction > Decimal::new(1, 0) {
                        return Err(OrderError::CouponAmountInvalid);
                    }
                    let factor = Decimal::new(1, 0).sub(&fraction)?;
                    total = total.mul(&factor)?.quantize(2, Rounding::HalfUp)?;
                    discount_applied = true;
                }
            }
            Ok(Quote {
                product_name: payg.stored_name.clone(),
                amount: convert(&total, input.currency, input.rate)?,
                currency: input.currency,
                coupon_id,
                discount_applied,
                pricing_plan_id: None,
                spec,
                hours: Some(hours),
                usd_amount: total,
                rate: *input.rate,
            })
        }
    }
}

/// What the page shows for `id` and `pt` (DEF-010): half the hourly price,
/// times hours. The charged total halves the whole total instead; the two
/// differ by design until mu decides which is right.
pub fn payg_display_total(catalog: &Catalog, hours: i64, ui_locale: &str) -> Result<Decimal, OrderError> {
    let payg = payg_entry(catalog, hours)?;
    let mut hourly = payg.usd_per_hour;
    if catalog.locale_discount_locales.contains(ui_locale) {
        let half = hourly.mul(&Decimal::new(5, 1))?;
        hourly = hourly.sub(&half)?.quantize(2, Rounding::Floor)?;
    }
    Ok(hourly.mul_int(i128::from(hours))?.quantize(2, Rounding::HalfUp)?)
}

/// D8: the total the purchase summary shows, kept apart from the charged quote.
/// Pay as you go shows `payg_display_total`, then a coupon for the displayed
/// product applied to that shown total (source `calculateCoupon`). A plan shows
/// its USD price; as in its quote, no coupon reaches a plan. The special TRY
/// offer changes only what is charged.
pub fn display_amount(catalog: &Catalog, product: Product, ui_locale: &str, coupon: Option<&Coupon>, now: Micros) -> Result<Decimal, OrderError> {
    match product {
        Product::Plan { index } => {
            let plan = catalog.plans.get(index).ok_or(OrderError::UnknownPlan(index))?;
            Ok(plan.usd_price.quantize(2, Rounding::HalfUp)?)
        }
        Product::PayAsYouGo { hours } => {
            let shown = payg_display_total(catalog, hours, ui_locale)?;
            let payg = payg_entry(catalog, hours)?;
            match coupon.filter(|c| !c.is_expired(now) && c.applies_to(&payg.stored_name)) {
                Some(c) => {
                    let fraction = c.amount.unwrap_or(Decimal::zero());
                    if fraction.is_negative() || fraction > Decimal::new(1, 0) {
                        return Err(OrderError::CouponAmountInvalid);
                    }
                    Ok(shown.mul(&Decimal::new(1, 0).sub(&fraction)?)?.quantize(2, Rounding::HalfUp)?)
                }
                None => Ok(shown),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoursInput {
    pub hours: i64,
    pub minimum_warning: bool,
}

/// The hours control: under 5 or not a number resets to 5 with a warning,
/// above 9999 becomes 9999, fractions drop to whole hours.
pub fn normalize_hours(raw: &str) -> HoursInput {
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && v >= MIN_HOURS as f64 => {
            HoursInput { hours: (v.floor() as i64).min(MAX_HOURS), minimum_warning: false }
        }
        _ => HoursInput { hours: MIN_HOURS, minimum_warning: true },
    }
}

pub fn step_hours(current: i64, delta: i64) -> i64 {
    (current + delta).clamp(MIN_HOURS, MAX_HOURS)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlanCard {
    pub name: String,
    pub interval: PlanInterval,
    pub usd_per_month: Decimal,
    pub struck_per_month: Decimal,
    pub badge: &'static str,
}

pub fn plan_card(plan: &PlanEntry) -> PlanCard {
    let usd_per_month = match plan.interval {
        PlanInterval::Monthly => plan.usd_price,
        PlanInterval::Yearly => plan
            .usd_price
            .div(&Decimal::new(12, 0), 2, Rounding::HalfUp)
            .unwrap_or(plan.usd_price),
    };
    PlanCard {
        name: plan.name.clone(),
        interval: plan.interval,
        usd_per_month,
        struck_per_month: plan.struck_price,
        badge: SAVE_BADGE,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaygCard {
    pub price_text: String,
    pub struck_per_hour: Decimal,
}

pub fn payg_card(catalog: &Catalog) -> Option<PaygCard> {
    catalog.payg.as_ref().map(|p| PaygCard {
        price_text: format!("${} per 1 hour", p.usd_per_hour),
        struck_per_hour: p.struck_per_hour,
    })
}

/// `= TL <usd total x rate, rounded to 2 decimals>` for the Turkish locale.
pub fn try_total_text(usd_total: &Decimal, rate: &Decimal) -> String {
    match usd_total.mul(rate).and_then(|v| v.quantize(2, Rounding::HalfUp)) {
        Ok(v) => format!("= TL {v}"),
        Err(_) => String::new(),
    }
}

/// The analytics purchase event reports TRY in USD with the same rate.
pub fn analytics_usd(amount: &Decimal, currency: Currency, rate: &Decimal) -> Decimal {
    match currency {
        Currency::Usd => *amount,
        Currency::Try => amount.div(rate, 2, Rounding::HalfUp).unwrap_or(*amount),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InvoiceTax {
    pub subtotal: Decimal,
    pub vat: Decimal,
    pub label: Option<&'static str>,
}

/// TRY invoices treat the stored amount as VAT inclusive at 20 percent; every
/// other currency shows no tax (CPN-018).
pub fn invoice_tax(amount: &Decimal, currency: Currency) -> InvoiceTax {
    match currency {
        Currency::Try => {
            let subtotal = amount
                .div(&Decimal::new(12, 1), 2, Rounding::HalfUp)
                .unwrap_or(*amount);
            let vat = amount.sub(&subtotal).unwrap_or(Decimal::zero());
            InvoiceTax { subtotal, vat, label: Some("VAT (20%)") }
        }
        Currency::Usd => InvoiceTax { subtotal: *amount, vat: Decimal::new(0, 2), label: None },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CouponLookup {
    Found(Coupon),
    NotFound,
    /// The server found the coupon but it has expired (a client sees no expiry instant).
    Expired,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CouponEffect {
    Fetch(String),
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum CouponMessage {
    NotValid,
    Expired,
    Error,
}

/// The state machine behind the coupon field (CPN-012).
#[derive(Clone, Debug, Default)]
pub struct CouponField {
    applied: Option<Coupon>,
    message: Option<CouponMessage>,
    last_fetched: Option<String>,
}

impl CouponField {
    pub fn submit(&mut self, code: &str) -> CouponEffect {
        if code.is_empty() {
            self.clear();
            return CouponEffect::None;
        }
        if self.is_locked() || self.last_fetched.as_deref() == Some(code) {
            return CouponEffect::None;
        }
        self.last_fetched = Some(code.to_string());
        CouponEffect::Fetch(code.to_string())
    }

    pub fn lookup_result(&mut self, result: CouponLookup, now: Micros) {
        match result {
            CouponLookup::Found(coupon) if coupon.is_expired(now) => {
                self.applied = None;
                self.message = Some(CouponMessage::Expired);
            }
            CouponLookup::Found(coupon) => {
                self.applied = Some(coupon);
                self.message = None;
            }
            CouponLookup::NotFound => {
                self.applied = None;
                self.message = Some(CouponMessage::NotValid);
            }
            CouponLookup::Expired => {
                self.applied = None;
                self.message = Some(CouponMessage::Expired);
            }
            CouponLookup::Failed => {
                self.applied = None;
                self.message = Some(CouponMessage::Error);
            }
        }
    }

    pub fn message(&self) -> Option<&'static str> {
        self.message.map(|m| match m {
            CouponMessage::NotValid => "Coupon is not valid",
            CouponMessage::Expired => "Coupon has expired",
            CouponMessage::Error => "Error",
        })
    }

    /// The same outcome as `message`, as a code a localized shell maps to its own copy.
    pub fn notice(&self) -> Option<CouponMessage> {
        self.message
    }

    pub fn applied(&self) -> Option<&Coupon> {
        self.applied.as_ref()
    }

    pub fn is_locked(&self) -> bool {
        self.applied.is_some()
    }

    pub fn clear(&mut self) {
        *self = CouponField::default();
    }
}

/// Host data for the loyalty alert; message and percentage cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialClientOffer {
    pub code: String,
    /// Data only. The text below is shown verbatim: the captured 25 versus 33
    /// percent disagreement (DEF-023) stays a host-data question for mu.
    pub percent: u32,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpecialClientAlert {
    pub code: String,
    pub percent: u32,
    pub message: String,
}

pub fn special_client_alert(is_special_client: bool, offer: &SpecialClientOffer) -> Option<SpecialClientAlert> {
    is_special_client.then(|| SpecialClientAlert {
        code: offer.code.clone(),
        percent: offer.percent,
        message: offer.message.clone(),
    })
}

/// Why a provider order's money cannot be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderAmountProblem {
    /// A negative, zero-subtotal or overflowing amount.
    Invalid,
    /// This sale never discounts at the provider, so any nonzero discount is unsupported.
    DiscountPresent,
    /// The total does not follow from subtotal and tax in the order's own tax mode.
    TotalMismatch,
}

/// The pre-tax amount the buyer was charged for the item, in cents, checked
/// for internal consistency. The quote is compared to this value, never to the
/// total: inclusive `total == subtotal - discount`, exclusive
/// `total == subtotal - discount + tax`.
pub fn order_net(subtotal: i64, discount: i64, tax: i64, total: i64, tax_inclusive: bool) -> Result<i64, OrderAmountProblem> {
    if subtotal <= 0 || discount < 0 || tax < 0 || total < 0 {
        return Err(OrderAmountProblem::Invalid);
    }
    if discount != 0 {
        return Err(OrderAmountProblem::DiscountPresent);
    }
    let net = subtotal;
    let expected = if tax_inclusive { net } else { net.checked_add(tax).ok_or(OrderAmountProblem::Invalid)? };
    if total != expected {
        return Err(OrderAmountProblem::TotalMismatch);
    }
    Ok(net)
}

#[cfg(test)]
mod tests;
