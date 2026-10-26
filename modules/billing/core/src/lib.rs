//! Billing: packages with windows, a derived usage ledger, payments, grants,
//! freemium, coupons and currency, as pure rules plus use cases over ports.
//!
//! No I/O and no host dependency lives here. Storage, HTTP and providers are
//! adapters in sibling crates; Transgate and taskr are consumers that supply
//! catalog data, identity, customer records and delivery.

pub mod app;
pub mod calendar;
pub mod charge;
pub mod decimal;
pub mod erasure;
pub mod freemium;
pub mod grants;
pub mod ledger;
pub mod model;
pub mod notify;
pub mod ports;
pub mod presentation;
pub mod pricing;
pub mod types;

pub use types::{AccountId, BoxFuture, Clock, FixedClock, Micros, PackageId, PaymentId, SubjectRef};
