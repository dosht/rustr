//! Jobs: the aggregate, routing and retry decisions, result ingestion
//! decisions, the ports the use cases call, and the use cases themselves.

pub mod app;
pub mod correlate;
pub mod domain;
pub mod duration;
pub mod events;
pub mod ingest;
pub mod ports;
pub mod retry;
pub mod routing;
#[cfg(test)]
pub(crate) mod testing;
