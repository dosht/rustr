//! Invoice facts for the owner's invoice context and the admin lookup (P5 D2,
//! D7). Billing derives every financial and identity fact here; the document
//! leaf only lays them out. The admin's temporary Bill To text is never part of
//! these facts and is never stored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calendar::format_dmy;
use crate::decimal::{Decimal, Rounding};
use crate::model::{BillingInfo, Payment};
use crate::ports::Customer;
use crate::pricing::{Currency, invoice_tax};
use crate::presentation::invoice_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaxKind {
    /// TRY: the stored amount includes 20 percent VAT (CPN-018).
    Vat20,
    None,
}

/// The Bill To identity the source computes from the latest billing profile
/// and the customer (`pages/billing/index.tsx:159-178`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillTo {
    /// `None` is the source's last fallback, "Valued Customer".
    pub name: Option<String>,
    /// Shown on its own line only when it is not already the name.
    pub company: Option<String>,
    pub address_lines: Vec<String>,
    pub country: Option<String>,
    pub tax_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvoiceFacts {
    pub payment_id: i64,
    pub invoice_id: String,
    /// `dd/MM/yyyy` of the payment's creation.
    pub date: String,
    pub package_name: String,
    pub package_minutes: Option<i64>,
    pub quantity: i64,
    pub currency: String,
    pub subtotal: Decimal,
    pub tax: Decimal,
    pub total: Decimal,
    pub tax_kind: TaxKind,
    pub gateway: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvoiceContext {
    pub bill_to: BillTo,
    /// The account's completed payments, newest first.
    pub invoices: Vec<InvoiceFacts>,
}

/// An invoice amount as the source prints it (`toFixed(2)`): two decimals, half up,
/// always padded. The document layout calls this for subtotal, tax and total; it
/// never rounds money itself.
pub fn invoice_amount(amount: &Decimal) -> String {
    amount.quantize(2, Rounding::HalfUp).map(|a| a.to_string()).unwrap_or_else(|_| amount.to_string())
}

/// JavaScript truthiness of an optional string: missing or empty is false.
fn given(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|t| !t.is_empty())
}

/// Present and not only whitespace, the source's `trim() !== ""` checks.
fn nonblank(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|t| !t.trim().is_empty())
}

pub fn bill_to(info: Option<&BillingInfo>, customer: &Customer) -> BillTo {
    let full_name = info.and_then(|i| nonblank(&i.full_name));
    let company = info.and_then(|i| nonblank(&i.company_name));
    let name = match (full_name, company) {
        (None, Some(company)) => Some(company.to_string()),
        (Some(full_name), _) => Some(full_name.to_string()),
        (None, None) => given(&customer.full_name)
            .map(str::to_string)
            .or_else(|| Some(customer.email.clone()).filter(|e| !e.is_empty())),
    };
    BillTo {
        company: company.filter(|c| name.as_deref() != Some(*c)).map(str::to_string),
        address_lines: info
            .and_then(|i| i.address.as_deref())
            .map(|a| a.split('\n').filter(|line| !line.trim().is_empty()).map(str::to_string).collect())
            .unwrap_or_default(),
        country: info.and_then(|i| given(&i.country)).map(str::to_string),
        tax_id: info.and_then(|i| given(&i.tax_id)).map(str::to_string),
        name,
    }
}

pub fn invoice_facts(payment: &Payment) -> InvoiceFacts {
    let currency = Currency::parse(&payment.currency).unwrap_or(Currency::Usd);
    let tax = invoice_tax(&payment.amount, currency);
    let package = payment.metadata.get("package");
    InvoiceFacts {
        payment_id: payment.id,
        invoice_id: invoice_id(payment.id),
        date: format_dmy(payment.created),
        package_name: payment
            .package_name()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Subscription Plan".into()),
        // Source `minutes || ""`: a zero or missing count prints nothing.
        package_minutes: package.and_then(|p| p.get("minutes")).and_then(Value::as_i64).filter(|m| *m != 0),
        quantity: 1,
        currency: payment.currency.clone(),
        subtotal: tax.subtotal,
        tax: tax.vat,
        total: payment.amount,
        tax_kind: if tax.label.is_some() { TaxKind::Vat20 } else { TaxKind::None },
        gateway: payment.gateway.clone(),
    }
}

/// `completed` is the account's completed payments, newest first.
pub fn invoice_context(customer: &Customer, info: Option<&BillingInfo>, completed: &[Payment]) -> InvoiceContext {
    InvoiceContext { bill_to: bill_to(info, customer), invoices: completed.iter().map(invoice_facts).collect() }
}

#[cfg(test)]
#[path = "invoice_tests.rs"]
mod tests;
