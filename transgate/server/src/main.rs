//! Process liveness plus, when configured, the billing module. `/health` is
//! liveness only and says nothing about billing; `/api/billing/ready` is the
//! billing readiness answer (503 until billing is configured and reachable).
//! Configure TRANSGATE_BIND with an IP socket address; the default is
//! 127.0.0.1:8788. Billing needs TRANSGATE_DATABASE_URL (the restricted runtime
//! login) and the settings it names; a half-configured billing refuses to start.

mod account;
mod ai;
mod callbacks;
mod crypto;
mod host;
mod mail;
mod ports;
mod workers;
mod web;

use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;

use axum::{Json, Router, routing::get};
use serde::Serialize;
use tokio::sync::oneshot;

#[derive(Serialize)]
struct Health {
    service: &'static str,
    version: &'static str,
    status: &'static str,
    check: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        status: "alive",
        check: "process-liveness",
    })
}

fn bind_address() -> io::Result<SocketAddr> {
    match std::env::var("TRANSGATE_BIND") {
        Ok(value) => value.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "TRANSGATE_BIND must be an IP socket address, such as 127.0.0.1:8788",
            )
        }),
        Err(std::env::VarError::NotPresent) => Ok(SocketAddr::from(([127, 0, 0, 1], 8788))),
        Err(std::env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TRANSGATE_BIND must contain valid Unicode",
        )),
    }
}

#[cfg(unix)]
fn shutdown_signal() -> io::Result<impl Future<Output = io::Result<()>>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt())
        .map_err(|error| io::Error::other(format!("registering SIGINT handler: {error}")))?;
    let mut terminate = signal(SignalKind::terminate())
        .map_err(|error| io::Error::other(format!("registering SIGTERM handler: {error}")))?;
    Ok(async move {
        let received = tokio::select! {
            received = interrupt.recv() => received,
            received = terminate.recv() => received,
        };
        received.ok_or_else(|| io::Error::other("shutdown signal stream closed unexpectedly"))
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> io::Result<impl Future<Output = io::Result<()>>> {
    Ok(async {
        tokio::signal::ctrl_c()
            .await
            .map_err(|error| io::Error::other(format!("receiving Ctrl-C: {error}")))
    })
}

async fn serve(
    bind: SocketAddr,
    billing: Option<transgate_billing::settings::Settings>,
    jobs: Option<host::JobsSettings>,
    web_settings: Option<web::WebSettings>,
) -> io::Result<()> {
    let shutdown = shutdown_signal()?;
    let web = match web_settings {
        Some(settings) => Some(web::WebHost::load(settings).await?),
        None => None,
    };
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| io::Error::other(format!("binding TRANSGATE_BIND: {error}")))?;
    let address = listener.local_addr()?;
    let mut app = Router::new().route("/health", get(health));
    let mut background = None;
    let mode = match billing {
        Some(settings) => {
            let owner_url = settings.migration_database_url.clone();
            let freemium = transgate_billing::billing_config(&settings).lists;
            let pages = web.as_ref().map(|page| transgate_billing::Pages {
                confirm: page.clone(),
                checkout: page.checkout_document(),
            });
            let composed = transgate_billing::compose(settings, pages)
                .await
                .map_err(|error| io::Error::other(format!("billing: {error}")))?;
            app = app.merge(composed.router.clone());
            match jobs {
                Some(jobs) => {
                    let job_host = host::compose(jobs, &composed, owner_url.as_deref(), freemium)
                        .await
                        .map_err(|error| io::Error::other(format!("jobs: {error}")))?;
                    app = app.merge(job_host.router);
                    background = Some(job_host.workers);
                    "billing and jobs configured"
                }
                None => "billing configured",
            }
        }
        None => {
            app = app.merge(transgate_billing::unconfigured_router());
            "billing not configured, process liveness only"
        }
    };
    let app = web::install(app, web);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async {
        // Axum's 'static shutdown future needs an owned notification.
        let _ = shutdown_rx.await;
    });
    let mut server = std::pin::pin!(std::future::IntoFuture::into_future(server));
    eprintln!("transgate-server listening on {address} ({mode})");

    let result = tokio::select! {
        result = &mut server => result,
        signal_result = shutdown => {
            eprintln!("transgate-server shutting down");
            shutdown_tx.send(()).map_err(|_| {
                io::Error::other("server stopped before shutdown could be requested")
            })?;
            let server_result = server.await;
            signal_result?;
            server_result
        }
    };
    if let Some(workers) = background {
        workers.abort();
    }
    result
}

fn run() -> io::Result<()> {
    let bind = bind_address()?;
    let lookup = |key: &str| std::env::var(key).ok().filter(|v| !v.trim().is_empty());
    let billing = transgate_billing::settings_from(&lookup)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("billing configuration: {error}")))?;
    let jobs = host::settings_from(&lookup).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("jobs configuration: {error}")))?;
    let web = web::settings_from(&lookup).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("web configuration: {error}")))?;
    if web.is_some() && jobs.is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "web configuration: TRANSGATE_WEB_DIR needs TRANSGATE_JOBS_ENABLED=1 and its billing configuration"));
    }
    if jobs.is_some() && billing.is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "jobs configuration: TRANSGATE_JOBS_ENABLED=1 needs the billing configuration: jobs run on its verified database pool"));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| io::Error::other(format!("creating Tokio runtime: {error}")))?;
    runtime.block_on(serve(bind, billing, jobs, web))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("transgate-server: {error}");
            ExitCode::FAILURE
        }
    }
}
