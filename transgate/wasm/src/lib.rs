//! Transgate in the browser: the one presenter per tab (core `AppState` plus
//! its effect driver) behind `transgate/web/src/lib/core/index.ts`. Shells
//! render `AppVm` and send `AppIntent`; every request, timer, watch, clipboard
//! write, download and the D17 checkout form run here, fenced by core epochs.
//!
//! Browser-only: the crate body is empty off wasm32, because its ports await
//! `JsFuture`s, which are not `Send`, while the client's native `BoxFuture` is.
#![cfg(target_arch = "wasm32")]

mod assets;
mod browser;
mod driver;
mod platform;
mod presenter;
mod tasks;

pub use presenter::TransgatePresenter;
