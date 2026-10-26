//! Transgate's client-side adapters, compiled for the browser (wasm32, through
//! `transgate-wasm`) and native shells (P8): GoTrue PKCE auth, the session HTTP
//! API, AI start/watch decoding, payment attribution, the D17 checkout form and
//! fenced document export over published host and core contracts. No product
//! rule lives here: every decision stays in `transgate-core` presentation.

pub mod ai;
pub mod api;
pub mod attribution;
pub mod billing;
pub mod auth;
pub mod checkout;
pub mod export;
pub mod future;
pub mod location;
