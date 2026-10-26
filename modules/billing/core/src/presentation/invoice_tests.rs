use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::calendar::utc;
use crate::grants::PaymentStatus;
use crate::types::{AccountId, Micros};

fn d(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

fn payment(id: i64, amount: &str, currency: &str, metadata: Value) -> Payment {
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
        metadata,
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

fn customer(full_name: Option<&str>, email: &str) -> Customer {
    Customer {
        account: AccountId(Uuid::from_u128(1)),
        email: email.into(),
        full_name: full_name.map(str::to_string),
        created_at: Micros(0),
    }
}

fn info(full_name: Option<&str>, company: Option<&str>, address: Option<&str>, country: Option<&str>, tax_id: Option<&str>) -> BillingInfo {
    BillingInfo {
        id: 1,
        account: Some(AccountId(Uuid::from_u128(1))),
        full_name: full_name.map(str::to_string),
        company_name: company.map(str::to_string),
        address: address.map(str::to_string),
        country: country.map(str::to_string),
        tax_id: tax_id.map(str::to_string),
        parasut_contact_id: None,
        created_at: Micros(0),
    }
}

// BIL-075/076, pages/billing/index.tsx:159-178: the Bill To identity follows the source
// fallback chain exactly; blanks count as missing, but kept text is not trimmed.
#[test]
fn the_bill_to_name_follows_the_source_fallback_chain() {
    let ada = customer(Some("Ada Customer"), "ada@example.test");
    let profile = info(Some("Ada Billing"), Some("Acme Ltd"), Some("Line 1\n  \nLine 2\n"), Some("TR"), Some("123"));
    let b = bill_to(Some(&profile), &ada);
    assert_eq!(b.name.as_deref(), Some("Ada Billing"), "the billing full name wins");
    assert_eq!(b.company.as_deref(), Some("Acme Ltd"), "the company gets its own line");
    assert_eq!(b.address_lines, vec!["Line 1".to_string(), "Line 2".to_string()], "blank address lines are dropped");
    assert_eq!((b.country.as_deref(), b.tax_id.as_deref()), (Some("TR"), Some("123")));

    let company_only = bill_to(Some(&info(Some("   "), Some("Acme Ltd"), None, Some(""), None)), &ada);
    assert_eq!(company_only.name.as_deref(), Some("Acme Ltd"), "a blank full name falls to the company");
    assert_eq!(company_only.company, None, "the company is not repeated under itself");
    assert_eq!((company_only.country, company_only.tax_id), (None, None), "empty fields print nothing");

    let no_profile = bill_to(None, &ada);
    assert_eq!(no_profile.name.as_deref(), Some("Ada Customer"), "without billing names the customer name is used");
    assert!(no_profile.address_lines.is_empty());
    let email_only = bill_to(Some(&info(None, Some(" "), None, None, None)), &customer(Some(""), "ada@example.test"));
    assert_eq!(email_only.name.as_deref(), Some("ada@example.test"), "then the email");
    let nothing = bill_to(None, &customer(None, ""));
    assert_eq!(nothing.name, None, "and finally the document's 'Valued Customer'");
}

// Source toFixed(2): stored amounts of any scale print with exactly two decimals.
#[test]
fn invoice_amounts_print_two_decimals_half_up_whatever_the_stored_scale() {
    for (stored, printed) in [("120", "120.00"), ("14.9", "14.90"), ("14.90", "14.90"), ("1.005", "1.01"), ("0", "0.00")] {
        assert_eq!(invoice_amount(&d(stored)), printed, "{stored}");
    }
    let facts = invoice_facts(&payment(1, "120", "TRY", json!({})));
    assert_eq!((invoice_amount(&facts.subtotal), invoice_amount(&facts.tax), invoice_amount(&facts.total)), ("100.00".into(), "20.00".into(), "120.00".into()));
}

// CPN-018, BIL-075: TRY totals include 20 percent VAT; other currencies show no tax. The
// facts carry the invoice id, date, package and gateway the document prints.
#[test]
fn invoice_facts_carry_server_derived_tax_and_identity() {
    let try_payment = payment(42, "120.00", "TRY", json!({"package": {"name": "PREMIUM", "minutes": 1200}}));
    let f = invoice_facts(&try_payment);
    assert_eq!((f.invoice_id.as_str(), f.date.as_str()), ("TRAN00000042", "05/03/2024"));
    assert_eq!((f.package_name.as_str(), f.package_minutes, f.quantity), ("PREMIUM", Some(1200), 1));
    assert_eq!((f.subtotal.to_string(), f.tax.to_string(), f.total.to_string()), ("100.00".into(), "20.00".into(), "120.00".into()));
    assert_eq!((f.tax_kind, f.currency.as_str(), f.gateway.as_str()), (TaxKind::Vat20, "TRY", "iyzico"));

    let usd = invoice_facts(&payment(7, "14.90", "USD", json!({"package": {"name": "", "minutes": 0}})));
    assert_eq!((usd.package_name.as_str(), usd.package_minutes), ("Subscription Plan", None), "an empty name and zero minutes fall back");
    assert_eq!((usd.subtotal.to_string(), usd.tax.to_string(), usd.tax_kind), ("14.90".into(), "0.00".into(), TaxKind::None));

    let context = invoice_context(&customer(None, "ada@example.test"), None, &[try_payment.clone(), payment(7, "14.90", "USD", json!({}))]);
    assert_eq!(context.invoices.iter().map(|i| i.payment_id).collect::<Vec<_>>(), vec![42, 7], "the given newest-first order is kept");
    assert_eq!(context.bill_to.name.as_deref(), Some("ada@example.test"));
}
