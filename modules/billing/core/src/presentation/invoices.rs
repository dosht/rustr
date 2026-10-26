//! The invoice history screen (WEB-037/038, WEB-BIL-015..017, BIL-075/076) as a pure
//! state machine: the owner's invoices, preview and download intents, and the
//! administrator's customer lookup with a temporary Bill To. Every financial fact
//! comes from billing; the temporary Bill To lives only in this state and in the
//! document request, never in any billing request or store.

use serde::{Deserialize, Serialize};

use crate::presentation::invoice::{BillTo, InvoiceContext, InvoiceFacts, invoice_amount};
use crate::presentation::purchase::PurchaseFailure;

/// The part of `POST /api/billing/admin/customer-invoices` this screen reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminLookupReply {
    pub customer: LookedUpCustomer,
    pub invoice_context: InvoiceContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookedUpCustomer {
    pub email: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum DocumentPurpose {
    Preview,
    Download,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum InvoicesEffect {
    /// `GET /api/billing/invoices`.
    LoadOwn,
    /// `POST /api/billing/admin/customer-invoices` with `{email}`.
    LookUp { email: String },
    /// Build the invoice document (`InvoiceDocument::capture(&facts, &bill_to, &temporary_bill_to)`)
    /// and preview or save it.
    Document { facts: InvoiceFacts, bill_to: BillTo, temporary_bill_to: String, purpose: DocumentPurpose },
}

#[derive(Clone, Debug, PartialEq)]
pub enum InvoicesEvent {
    Loaded(Result<InvoiceContext, PurchaseFailure>),
    Preview { payment_id: i64 },
    ClosePreview,
    Download { payment_id: i64 },
    LookupEmailTyped(String),
    LookupRequested,
    LookupAnswered(Result<AdminLookupReply, PurchaseFailure>),
    /// Back to the administrator's own invoices.
    LookupCleared,
    TemporaryBillToTyped(String),
    /// The document for `payment_id` could not be built, previewed or saved.
    DocumentFailed { payment_id: i64, failure: PurchaseFailure },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InvoiceRow {
    pub payment_id: i64,
    pub invoice_id: String,
    pub date: String,
    /// `<amount> <currency>`, two decimals.
    pub amount_text: String,
    pub package_name: String,
    pub gateway: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AdminVm {
    pub email: String,
    pub looking_up: bool,
    /// The customer whose invoices are shown, when a lookup succeeded.
    pub viewing_customer: Option<String>,
    /// 404: no customer with that email.
    pub not_found: bool,
    pub temporary_bill_to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InvoicesVm {
    pub loading: bool,
    pub rows: Vec<InvoiceRow>,
    pub preview: Option<i64>,
    /// Present only for an administrator (a server-verified capability).
    pub admin: Option<AdminVm>,
    pub failure: Option<PurchaseFailure>,
    /// The last preview or download failed; the rows stay usable.
    pub document_failure: Option<PurchaseFailure>,
}

pub struct InvoicesState {
    is_admin: bool,
    loading: bool,
    own: Option<InvoiceContext>,
    customer: Option<(String, InvoiceContext)>,
    preview: Option<i64>,
    email: String,
    looking_up: bool,
    not_found: bool,
    temporary_bill_to: String,
    failure: Option<PurchaseFailure>,
    document_failure: Option<PurchaseFailure>,
}

impl InvoicesState {
    /// `is_admin` is the session's verified administrator capability.
    pub fn new(is_admin: bool) -> (InvoicesState, Vec<InvoicesEffect>) {
        let state = InvoicesState {
            is_admin,
            loading: true,
            own: None,
            customer: None,
            preview: None,
            email: String::new(),
            looking_up: false,
            not_found: false,
            temporary_bill_to: String::new(),
            failure: None,
            document_failure: None,
        };
        (state, vec![InvoicesEffect::LoadOwn])
    }

    pub fn is_previewing(&self, payment_id: i64) -> bool {
        self.preview == Some(payment_id)
    }

    fn shown(&self) -> Option<&InvoiceContext> {
        self.customer.as_ref().map(|(_, c)| c).or(self.own.as_ref())
    }

    fn document(&mut self, payment_id: i64, purpose: DocumentPurpose) -> Vec<InvoicesEffect> {
        let Some(context) = self.shown() else { return vec![] };
        let Some(facts) = context.invoices.iter().find(|f| f.payment_id == payment_id) else { return vec![] };
        // Only an administrator's screen carries a temporary Bill To.
        let temporary_bill_to = if self.is_admin { self.temporary_bill_to.clone() } else { String::new() };
        let effect = InvoicesEffect::Document { facts: facts.clone(), bill_to: context.bill_to.clone(), temporary_bill_to, purpose };
        self.document_failure = None;
        vec![effect]
    }

    pub fn update(&mut self, event: InvoicesEvent) -> Vec<InvoicesEffect> {
        match event {
            InvoicesEvent::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(context) => {
                        self.own = Some(context);
                        self.failure = None;
                    }
                    Err(failure) => self.failure = Some(failure),
                }
                vec![]
            }
            InvoicesEvent::Preview { payment_id } => {
                let effects = self.document(payment_id, DocumentPurpose::Preview);
                if !effects.is_empty() {
                    self.preview = Some(payment_id);
                }
                effects
            }
            InvoicesEvent::ClosePreview => {
                self.preview = None;
                vec![]
            }
            InvoicesEvent::Download { payment_id } => self.document(payment_id, DocumentPurpose::Download),
            InvoicesEvent::LookupEmailTyped(email) if self.is_admin => {
                self.email = email;
                vec![]
            }
            InvoicesEvent::LookupRequested if self.is_admin && !self.looking_up => {
                let email = self.email.trim().to_string();
                if email.is_empty() {
                    return vec![];
                }
                self.looking_up = true;
                self.not_found = false;
                vec![InvoicesEffect::LookUp { email }]
            }
            InvoicesEvent::LookupAnswered(result) if self.looking_up => {
                self.looking_up = false;
                self.preview = None;
                match result {
                    Ok(reply) => {
                        self.customer = Some((reply.customer.email, reply.invoice_context));
                        self.failure = None;
                    }
                    Err(PurchaseFailure::Refused { code }) if code == "not_found" => {
                        self.customer = None;
                        self.not_found = true;
                    }
                    Err(failure) => self.failure = Some(failure),
                }
                vec![]
            }
            InvoicesEvent::LookupCleared => {
                self.customer = None;
                self.not_found = false;
                self.preview = None;
                vec![]
            }
            InvoicesEvent::TemporaryBillToTyped(text) if self.is_admin => {
                self.temporary_bill_to = text;
                vec![]
            }
            InvoicesEvent::DocumentFailed { payment_id, failure } => {
                if self.preview == Some(payment_id) {
                    self.preview = None;
                }
                self.document_failure = Some(failure);
                vec![]
            }
            // Administrator events from a non-administrator, or a stray answer.
            _ => vec![],
        }
    }

    pub fn view(&self) -> InvoicesVm {
        InvoicesVm {
            loading: self.loading,
            rows: self
                .shown()
                .map(|c| {
                    c.invoices
                        .iter()
                        .map(|f| InvoiceRow {
                            payment_id: f.payment_id,
                            invoice_id: f.invoice_id.clone(),
                            date: f.date.clone(),
                            amount_text: format!("{} {}", invoice_amount(&f.total), f.currency),
                            package_name: f.package_name.clone(),
                            gateway: f.gateway.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            preview: self.preview,
            admin: self.is_admin.then(|| AdminVm {
                email: self.email.clone(),
                looking_up: self.looking_up,
                viewing_customer: self.customer.as_ref().map(|(email, _)| email.clone()),
                not_found: self.not_found,
                temporary_bill_to: self.temporary_bill_to.clone(),
            }),
            failure: self.failure.clone(),
            document_failure: self.document_failure.clone(),
        }
    }
}

#[cfg(test)]
#[path = "invoices_tests.rs"]
mod tests;
