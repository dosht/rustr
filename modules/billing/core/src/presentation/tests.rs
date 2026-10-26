use super::*;
use crate::calendar::utc;
use crate::grants::{PackageSpec, PaymentStatus};
use crate::charge::{JobStart, StartCharge, decide_job_start};
use crate::ledger::{Package, PackageBalance, apply};
use crate::model::Payment;
use crate::types::AccountId;
use serde_json::json;
use uuid::Uuid;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn package(id: i64, name: &str, minutes: i64) -> Package {
    Package {
        id,
        account: AccountId(Uuid::from_u128(1)),
        payment_id: None,
        name: Some(name.into()),
        minutes_included: minutes,
        start_at: utc(2023, 1, 1, 0, 0, 0, 0),
        end_at: utc(2023, 2, 1, 0, 0, 0, 0),
    }
}

fn payment(id: i64, amount: &str, currency: &str, package: Option<&str>) -> Payment {
    Payment {
        id,
        account: AccountId(Uuid::from_u128(1)),
        amount: d(amount),
        currency: currency.into(),
        gateway: "iyzico".into(),
        status: PaymentStatus::Completed,
        token: Some("tok".into()),
        created: utc(2024, 3, 5, 23, 59, 59, 0),
        environment: "live".into(),
        metadata: match package {
            Some(name) => json!({"package": {"name": name, "minutes": 60, "count": 1, "interval": "year", "frequency": 1, "expiry": 1}}),
            None => json!({}),
        },
        coupon_id: None,
        pricing_plan_id: None,
        customer_id: None,
        subscription_ref: None,
        provider_order_ref: None,
        subscription_canceled_at: None,
        observed: None,
        receipt_url: None,
    }
}

#[test]
fn bil_073_balance_text_is_minutes_under_an_hour_then_hours_and_minutes() {
    assert_eq!(balance_text(None), "-");
    assert_eq!(balance_text(Some(0)), "0 min");
    assert_eq!(balance_text(Some(59)), "59 min");
    assert_eq!(balance_text(Some(60)), "1 hr , 0 min");
    assert_eq!(balance_text(Some(125)), "2 hr , 5 min");
    assert_eq!(balance_text(Some(-16)), "-16 min");
    assert_eq!(BALANCE_REFRESH_SECONDS, 30);
}

const AT: Micros = Micros(1_673_740_800_000_000); // 2023-01-15 00:00:00 UTC, inside every test package's window

fn held(id: i64, name: &str, minutes: i64) -> PackageBalance {
    PackageBalance { package: package(id, name, minutes), consumed: 0 }
}

fn job(duration: i64, other_completed_jobs: u64) -> JobStart {
    JobStart { subject: 1, duration: Some(duration), other_completed_jobs }
}

/// Applies what the charge decision wrote, as the transaction does, so the answer is derived
/// from the state after the debit rather than handed in.
fn debit(all: &mut [PackageBalance], charge: StartCharge) {
    let mut open = crate::ledger::open_at(all, AT);
    let ids: Vec<i64> = open.iter().map(|p| p.package.id).collect();
    let allocations = match charge {
        StartCharge::Bypass(allocation) => vec![allocation],
        StartCharge::Strict(allocations) => allocations,
        StartCharge::Deferred => vec![],
    };
    apply(&mut open, &allocations);
    for (id, after) in ids.into_iter().zip(open) {
        if let Some(p) = all.iter_mut().find(|p| p.package.id == id) {
            p.consumed = after.consumed;
        }
    }
}

#[test]
fn bil_074_a_started_answer_is_derived_from_the_state_after_the_debit() {
    assert_eq!(AT, utc(2023, 1, 15, 0, 0, 0, 0));
    // First job of a freemium account: 36 minutes against 20 free ones overdraws to -16, and
    // the account still has exactly one package, the free one.
    let mut free = vec![held(1, "freemium", 20)];
    let charge = decide_job_start(&job(36, 0), &crate::ledger::open_at(&free, AT)).unwrap();
    debit(&mut free, charge);
    let meta = start_answer_meta(&free, AT);
    assert_eq!((meta.new_balance, meta.is_freemium), (-16, true));

    // A paid purchase ends the freemium answer for good. The account also holds its 20 free minutes.
    let mut paid = vec![held(1, "freemium", 20), held(2, "PREMIUM", 100)];
    let charge = decide_job_start(&job(37, 5), &crate::ledger::open_at(&paid, AT)).unwrap();
    debit(&mut paid, charge);
    let meta = start_answer_meta(&paid, AT);
    assert_eq!((meta.new_balance, meta.is_freemium), (83, false));

    // The source's definition counts every package ever held, not only open ones: a paid
    // package that has already expired still makes the answer not freemium, though only the
    // free package is open and pays for the job.
    let expired_paid = PackageBalance { package: Package { end_at: utc(2023, 1, 10, 0, 0, 0, 0), ..package(3, "PREMIUM", 100) }, consumed: 0 };
    let mut history = vec![held(1, "freemium", 20), expired_paid];
    let charge = decide_job_start(&job(5, 3), &crate::ledger::open_at(&history, AT)).unwrap();
    debit(&mut history, charge);
    let meta = start_answer_meta(&history, AT);
    assert_eq!((meta.new_balance, meta.is_freemium), (15, false));

    // A deferred job debits nothing yet: the answer reports the unchanged balance.
    let mut unchanged = vec![held(1, "freemium", 20)];
    let deferred = JobStart { subject: 1, duration: None, other_completed_jobs: 0 };
    let charge = decide_job_start(&deferred, &crate::ledger::open_at(&unchanged, AT)).unwrap();
    debit(&mut unchanged, charge);
    assert_eq!(start_answer_meta(&unchanged, AT).new_balance, 20);

    // No packages at all reads as 0 minutes, never as an error.
    assert_eq!(start_answer_meta(&[], AT).new_balance, 0);
}

#[test]
fn bil_064_renewal_text_is_30_days_monthly_365_yearly_and_none_for_pay_as_you_go() {
    assert_eq!(renewal_notice(Some(PlanInterval::Monthly)), RenewalNotice::InDays(30));
    assert_eq!(renewal_notice(Some(PlanInterval::Yearly)), RenewalNotice::InDays(365));
    assert_eq!(renewal_notice(None), RenewalNotice::WillNotRenew);
    assert_eq!(RenewalNotice::WillNotRenew.text(), Some("Pay-as-you-go will not renew"));
}

#[test]
fn bil_063_next_payment_is_created_plus_count_intervals_and_empty_when_it_cannot_be_known() {
    let created = utc(2023, 1, 31, 0, 0, 0, 0);
    let monthly = PackageSpec { name: "PREMIUM".into(), minutes: 1, count: 1, interval: "month".into(), frequency: 1, expiry: 1 };
    assert_eq!(next_payment_date(created, Some(&monthly), false), Some(utc(2023, 2, 28, 0, 0, 0, 0)));
    let yearly = PackageSpec { count: 12, ..monthly.clone() };
    assert_eq!(next_payment_date(created, Some(&yearly), false), Some(utc(2024, 1, 31, 0, 0, 0, 0)));
    let years = PackageSpec { count: 1, interval: "year".into(), ..monthly.clone() };
    assert_eq!(next_payment_date(created, Some(&years), false), Some(utc(2024, 1, 31, 0, 0, 0, 0)));
    assert_eq!(next_payment_date(created, Some(&monthly), true), None, "canceled");
    assert_eq!(next_payment_date(created, None, false), None, "missing metadata");
    let weekly = PackageSpec { interval: "week".into(), ..monthly };
    assert_eq!(next_payment_date(created, Some(&weekly), false), None);
}

#[test]
fn bil_062_a_canceled_subscription_keeps_its_minutes_and_hides_the_next_payment() {
    let view = subscription_view(true);
    assert_eq!(view.message, Some("Your subscription is canceled, but you can still use your minutes."));
    assert!(!view.show_next_payment);
    let live = subscription_view(false);
    assert_eq!(live.message, None);
    assert!(live.show_next_payment);
}

#[test]
fn bil_056_065_the_plan_box_shows_only_premium_or_business() {
    let packages = vec![package(1, "freemium", 20), package(2, "Pay-as-you-Go", 600), package(3, "BUSINESS", 2400), package(4, "PREMIUM", 1200)];
    match current_plan(&packages) {
        CurrentPlan::Plan { name, minutes, ends_at } => {
            assert_eq!((name.as_str(), minutes, ends_at), ("BUSINESS", 2400, utc(2023, 2, 1, 0, 0, 0, 0)));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(current_plan(&[package(1, "freemium", 20), package(2, "Pay-as-you-Go", 60)]), CurrentPlan::NoPackage);
    assert_eq!(current_plan(&[]), CurrentPlan::NoPackage);
}

#[test]
fn bil_075_history_rows_carry_the_invoice_id_date_amount_package_and_gateway() {
    assert_eq!(invoice_id(42), "TRAN00000042");
    assert_eq!(invoice_id(123_456_789), "TRAN123456789");
    let row = history_row(&payment(42, "14.9", "USD", Some("Pay-as-you-Go")));
    assert_eq!(row.invoice_id, "TRAN00000042");
    assert_eq!(row.date, "05/03/2024");
    assert_eq!(row.amount_text, "14.90 USD");
    assert_eq!(row.package_name, "Pay-as-you-Go");
    assert_eq!(row.gateway, "iyzico");
    assert_eq!(history_row(&payment(1, "393.35999999999996", "TRY", None)).amount_text, "393.36 TRY");
    assert_eq!(history_row(&payment(1, "5", "TRY", None)).package_name, "N/A");
}

#[test]
fn bil_072_events_follow_a_real_status_change_only_and_the_countdown_is_five_seconds() {
    assert_eq!(CONFIRM_COUNTDOWN_SECONDS, 5);
    let plan_payment = Payment { pricing_plan_id: Some("plan".into()), ..payment(1, "14", "USD", Some("PREMIUM")) };
    let done = confirm_analytics(Some(PaymentStatus::Completed), &plan_payment, &d("44"));
    assert_eq!(
        done,
        vec![
            AnalyticsIntent::PaymentCompleted,
            AnalyticsIntent::SubscriptionStarted,
            AnalyticsIntent::Purchase { value_usd: d("14.00"), currency: "USD" },
        ]
    );
    // An already completed payment changed nothing: no events at all.
    assert!(confirm_analytics(None, &plan_payment, &d("44")).is_empty());
    assert_eq!(confirm_analytics(Some(PaymentStatus::Failed), &plan_payment, &d("44")), vec![AnalyticsIntent::PaymentFailed]);
    // subscription_started only for the first plan payment: renewals are not.
    let renewal = Payment {
        metadata: json!({"analytics": {"payment_origin": "renewal"}, "package": {"name": "PREMIUM", "minutes": 1, "count": 1, "interval": "month", "frequency": 1, "expiry": 1}}),
        ..plan_payment
    };
    assert!(!confirm_analytics(Some(PaymentStatus::Completed), &renewal, &d("44")).contains(&AnalyticsIntent::SubscriptionStarted));
    // TRY purchases are reported in USD (CPN-017).
    let try_payment = payment(2, "1770.12", "TRY", Some("Pay-as-you-Go"));
    assert_eq!(
        confirm_analytics(Some(PaymentStatus::Completed), &try_payment, &d("44")).last(),
        Some(&AnalyticsIntent::Purchase { value_usd: d("40.23"), currency: "USD" })
    );
}
