//! Transgate in the browser: the browser's `Platform` and page wiring around the
//! shared `AppDriver` of `transgate-client`, behind `transgate/web/src/lib/core/index.ts`.
//! Shells render `AppVm` and send `AppIntent`; every request, timer, watch, clipboard
//! write, download and the D17 checkout form run in the driver, fenced by core epochs.
//!
//! Browser-only: the crate body is empty off wasm32, because its ports await
//! `JsFuture`s, which are not `Send`, while the client's native `BoxFuture` is.
#![cfg(target_arch = "wasm32")]

mod assets;
mod browser;
mod platform;
mod presenter;
mod tasks;

pub use presenter::TransgatePresenter;
