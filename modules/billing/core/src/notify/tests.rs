use super::*;
use crate::grants::PaymentStatus;
use crate::model::Payment;
use crate::types::{AccountId, BoxFuture, Micros};
use serde_json::json;
use uuid::Uuid;

struct Directory(Vec<Customer>);

impl CustomerDirectory for Directory {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move { Ok(self.0.iter().find(|c| c.account == account).cloned()) })
    }
    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move { Ok(self.0.iter().find(|c| c.email == email).cloned()) })
    }
    fn register<'a>(&'a self, customer: &'a Customer) -> BoxFuture<'a, Result<Customer, DirectoryError>> {
        Box::pin(async move { Ok(customer.clone()) })
    }
}

fn account() -> AccountId {
    AccountId(Uuid::from_u128(7))
}

fn customer(name: Option<&str>) -> Customer {
    Customer { account: account(), email: "ada.lovelace@example.test".into(), full_name: name.map(str::to_string), created_at: Micros(0) }
}

fn config() -> NotifyConfig {
    NotifyConfig { brand: "Transgate".into(), sales_email: "sales@example.test".into(), support_email: "support@example.test".into() }
}

fn completed(renewal: bool) -> BillingEvent {
    let payment = Payment {
        id: 42,
        account: account(),
        amount: "14.90".parse().unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        status: PaymentStatus::Completed,
        token: Some("tok".into()),
        created: Micros(0),
        environment: "live".into(),
        metadata: json!({"package": {"name": "Pay-as-you-Go", "minutes": 600, "count": 1, "interval": "year", "frequency": 1, "expiry": 1}}),
        coupon_id: None,
        pricing_plan_id: None,
        customer_id: None,
        subscription_ref: None,
        provider_order_ref: None,
        subscription_canceled_at: None,
        observed: None,
        receipt_url: None,
    };
    BillingEvent::PaymentCompleted {
        payment_id: 42,
        account: account(),
        amount: payment.amount,
        currency: "USD".into(),
        gateway: "iyzico".into(),
        package_name: Some("Pay-as-you-Go".into()),
        renewal,
        payment: serde_json::to_value(&payment).unwrap(),
    }
}

#[tokio::test]
async fn eml_bil_001_002_completion_sends_the_buyer_and_the_sales_email_with_the_documented_subjects() {
    let directory = Directory(vec![customer(Some("Ada Lovelace"))]);
    let deliveries = plan_deliveries(&completed(false), &directory, &config()).await.unwrap();
    assert_eq!(deliveries.len(), 2);
    let buyer = deliveries.iter().find(|d| d.target == "buyer").unwrap();
    assert_eq!(buyer.message.to, "ada.lovelace@example.test", "the recipient is the stored customer, never a posted field");
    assert_eq!(buyer.message.subject, "Woohoo! Your Transgate purchase is complete!");
    assert!(buyer.message.text.contains("Ada"), "it needs only the first name");
    assert!(!buyer.message.text.contains("Lovelace"));
    let sales = deliveries.iter().find(|d| d.target == "sales").unwrap();
    assert_eq!(sales.message.to, "sales@example.test");
    assert_eq!(sales.message.subject, "Payment Completed");
    for needle in ["42", "14.90", "USD", "ada.lovelace@example.test", &account().to_string(), "Pay-as-you-Go"] {
        assert!(sales.message.text.contains(needle), "sales email must carry {needle}");
    }
}

#[tokio::test]
async fn eml_bil_001_the_first_name_falls_back_to_the_email_local_part() {
    let directory = Directory(vec![customer(None)]);
    let deliveries = plan_deliveries(&completed(false), &directory, &config()).await.unwrap();
    let buyer = deliveries.iter().find(|d| d.target == "buyer").unwrap();
    assert!(buyer.message.text.contains("ada.lovelace"));
}

#[tokio::test]
async fn eml_bil_003_a_cancellation_request_goes_to_support_with_the_payment_id_as_the_body() {
    let directory = Directory(vec![customer(None)]);
    let event = BillingEvent::CancelRequested { payment_id: 42, account: account() };
    let deliveries = plan_deliveries(&event, &directory, &config()).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].target, "support");
    assert_eq!(deliveries[0].message.to, "support@example.test");
    assert_eq!(deliveries[0].message.subject, "!!!CANCEL SUBSCRIPTION!!!");
    assert_eq!(deliveries[0].message.text, "42");
}

#[tokio::test]
async fn a_missing_customer_is_an_error_to_retry_not_a_skipped_delivery() {
    let directory = Directory(vec![]);
    assert!(plan_deliveries(&completed(false), &directory, &config()).await.is_err());
}

#[tokio::test]
async fn events_without_a_recipient_plan_nothing() {
    let directory = Directory(vec![]);
    let held = BillingEvent::NotificationHeld { gateway: "iyzico".into(), reason: "x".into(), order_ref: None, subscription_ref: None };
    assert!(plan_deliveries(&held, &directory, &config()).await.unwrap().is_empty());
}

fn with_payment(event: &BillingEvent, edit: impl FnOnce(&mut serde_json::Value)) -> BillingEvent {
    let BillingEvent::PaymentCompleted { payment_id, account, amount, currency, gateway, package_name, renewal, payment } = event else {
        panic!("a completed event")
    };
    let mut payment = payment.clone();
    edit(&mut payment);
    BillingEvent::PaymentCompleted {
        payment_id: *payment_id,
        account: *account,
        amount: *amount,
        currency: currency.clone(),
        gateway: gateway.clone(),
        package_name: package_name.clone(),
        renewal: *renewal,
        payment,
    }
}

fn names(events: &[AnalyticsEvent]) -> Vec<&'static str> {
    events.iter().map(|e| e.name).collect()
}

const AT: Micros = Micros(1_768_478_400_000_000);

fn planned(event: &BillingEvent) -> Vec<AnalyticsEvent> {
    plan_analytics(event, AT).unwrap()
}

#[test]
fn bil_072_analytics_a_pay_as_you_go_completion_is_one_payment_completed_event_with_the_source_properties() {
    let events = planned(&completed(false));
    assert_eq!(names(&events), vec!["payment_completed"]);
    let event = &events[0];
    assert_eq!(event.distinct_id, account().to_string());
    assert_eq!(event.timestamp, Some(AT));
    assert_eq!(event.dedup_name, r#"["payment_completed","payments","42"]"#);
    assert_eq!(
        serde_json::Value::Object(event.properties.clone()),
        json!({"amount": 14.9, "currency": "USD", "payment_id": 42, "payment_method": "card", "is_trial": false, "plan": "pay-as-you-go"})
    );
    // Planning is deterministic: every process derives the same identity for the same event.
    assert_eq!(planned(&completed(false)), events);
    assert_eq!(event.target(), "analytics:payment_completed");
}

#[test]
fn bil_072_analytics_a_first_subscription_payment_adds_subscription_started_and_a_renewal_does_not() {
    let first = with_payment(&completed(false), |p| {
        p["pricing_plan_id"] = json!("plan-1");
        p["metadata"]["package"]["count"] = json!(12);
        p["metadata"]["analytics"] = json!({"payment_origin": "initial", "utm_source": "news", "utm_medium": 3, "ignored": "x"});
    });
    let events = planned(&first);
    assert_eq!(names(&events), vec!["payment_completed", "subscription_started"]);
    assert_eq!(events[1].dedup_name, r#"["subscription_started","payments","42"]"#);
    let properties = serde_json::Value::Object(events[0].properties.clone());
    assert_eq!(properties["billing_cycle"], "yearly");
    assert_eq!(properties["utm_source"], "news", "string attribution keys of an initial payment are kept");
    assert!(properties.get("utm_medium").is_none(), "a non-string attribution value is not");
    assert!(properties.get("ignored").is_none());

    let renewal = with_payment(&completed(true), |p| {
        p["pricing_plan_id"] = json!("plan-1");
        p["metadata"]["package"]["count"] = json!(1);
        p["metadata"]["analytics"] = json!({"payment_origin": "renewal", "utm_source": "news"});
    });
    let events = planned(&renewal);
    assert_eq!(names(&events), vec!["payment_completed"]);
    let properties = serde_json::Value::Object(events[0].properties.clone());
    assert_eq!(properties["billing_cycle"], "monthly");
    assert_eq!(properties["utm_source"], "news", "a renewal keeps attribution too");

    // An origin other than initial or renewal carries no attribution and starts nothing.
    let odd = with_payment(&completed(false), |p| {
        p["pricing_plan_id"] = json!("plan-1");
        p["metadata"]["analytics"] = json!({"payment_origin": "import", "utm_source": "news"});
    });
    let events = planned(&odd);
    assert_eq!(names(&events), vec!["payment_completed"]);
    assert!(events[0].properties.get("utm_source").is_none());
}

#[test]
fn bil_072_analytics_a_failed_payment_carries_its_reason_and_plan() {
    let mut payment = match completed(false) {
        BillingEvent::PaymentCompleted { payment, .. } => payment,
        _ => unreachable!(),
    };
    payment["status"] = json!("failed");
    let failed = BillingEvent::PaymentFailed {
        payment_id: 7,
        account: account(),
        amount: "14.90".parse().unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        payment,
    };
    let events = planned(&failed);
    assert_eq!(names(&events), vec!["payment_failed"]);
    let properties = serde_json::Value::Object(events[0].properties.clone());
    assert_eq!(properties["failure_reason"], "payment_failed");
    assert_eq!(properties["plan"], "pay-as-you-go");

    let missing_snapshot = BillingEvent::PaymentFailed {
        payment_id: 7,
        account: account(),
        amount: "14.90".parse().unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        payment: serde_json::Value::Null,
    };
    assert!(plan_analytics(&missing_snapshot, Micros(0)).is_err());
}

#[test]
fn bil_072_analytics_account_creation_is_one_user_signed_up_event_and_other_events_plan_none() {
    let granted = BillingEvent::FreemiumGranted { account: account(), package_id: 3 };
    let events = planned(&granted);
    assert_eq!(names(&events), vec!["user_signed_up"]);
    assert_eq!(events[0].dedup_name, format!(r#"["user_signed_up","users","{}"]"#, account()));
    assert_eq!(events[0].timestamp, None, "the source sends no timestamp for a signup");
    assert!(events[0].properties.is_empty());
    let cancel = BillingEvent::CancelRequested { payment_id: 7, account: account() };
    assert!(planned(&cancel).is_empty());
    // A completed event whose stored payment cannot be read is an error to retry, not silence.
    let broken = BillingEvent::PaymentCompleted {
        payment_id: 7,
        account: account(),
        amount: "1.00".parse().unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        package_name: None,
        renewal: false,
        payment: json!({"id": "not a payment"}),
    };
    assert!(plan_analytics(&broken, AT).is_err());
}
