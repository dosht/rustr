//! taskr's audio-work sample: a small second consumer of the shared media
//! ports. It owns its own aggregate and schema (`taskr_work`, its own migration history) in the
//! billing sample's database, on the billing sample's verified runtime pool, so that account
//! erasure is one transaction over billing, the billing sample and these rows. It imports
//! nothing from Transgate and nothing in `taskr-core` changes. Ordinary tasks are untouched.
//! `work` and `outcome` are pure: the aggregate decides, adapters execute.
//! `store` is the Postgres persistence, `app` the use cases over the shared
//! ports, `http` the thin Axum routes, `workers` the background tick and `compose` the
//! configuration and wiring of the real adapters.

pub mod app;
pub mod compose;
pub mod correlate;
pub mod http;
pub mod outcome;
pub mod store;
pub mod summary;
pub mod work;
pub mod workers;
