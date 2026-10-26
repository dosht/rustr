//! The background tick: submit ready work, reconcile stuck attempts, continue
//! erasures and run the daily retention slot. Replicas may all run it: every
//! claim is a compare-and-swap or a fenced slot, so passes overlap safely.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::app::{PassReport, Service, ServiceError};

#[derive(Debug, Default)]
pub struct TickReport {
    pub dispatch: PassReport,
    pub recovery: PassReport,
    pub erasure: PassReport,
    pub retention: PassReport,
    pub errors: Vec<String>,
}

fn take(report: Result<PassReport, ServiceError>, name: &str, errors: &mut Vec<String>) -> PassReport {
    match report {
        Ok(report) => {
            errors.extend(report.failures.iter().map(|f| format!("{name}: {f}")));
            report
        }
        Err(e) => {
            errors.push(format!("{name}: {e}"));
            PassReport::default()
        }
    }
}

/// One pass of every worker, in the order that makes the most progress.
pub async fn tick(service: &Service) -> TickReport {
    let mut errors = Vec::new();
    let dispatch = take(service.dispatch_pass().await, "dispatch", &mut errors);
    let recovery = take(service.recovery_pass().await, "recovery", &mut errors);
    let erasure = take(service.erasure_pass().await, "erasure", &mut errors);
    let retention = take(service.retention_pass().await, "retention", &mut errors);
    TickReport { dispatch, recovery, erasure, retention, errors }
}

/// Runs `tick` every `every` until the returned handle is aborted. A tick's errors are printed
/// without request data; they never stop the loop.
pub fn spawn(service: Arc<Service>, every: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            for error in tick(&service).await.errors {
                eprintln!("taskr audio work: {error}");
            }
        }
    })
}
