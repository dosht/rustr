//! Transgate in the browser: the one presenter per tab (core `AppState` plus
//! its effect driver) behind `transgate/web/src/lib/core/index.ts`. Shells
//! render `AppVm` and send `AppIntent`; every request, timer, watch, clipboard
//! write, download and the D17 checkout form run here, fenced by core epochs.
mod assets;
mod browser;
mod driver;
mod platform;
mod presenter;
mod tasks;

pub use presenter::TransgatePresenter;
