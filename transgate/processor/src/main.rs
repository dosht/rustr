//! `transgate-processor`: the free-tools processor as its own deployable.
//! One instance only: sessions and allowances are in memory (OPS-D06).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use serde_json::json;
use transgate_processor::{Config, Processor, log};

#[tokio::main]
async fn main() {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            log::error("Invalid configuration", json!({}), &e);
            std::process::exit(2);
        }
    };
    let port = config.port;
    let processor = Processor::new(config);
    let listener = match tokio::net::TcpListener::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))).await {
        Ok(listener) => listener,
        // Hosts without IPv6 still get the service on every IPv4 address.
        Err(_) => match tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).await {
            Ok(listener) => listener,
            Err(e) => {
                log::error("Could not bind the listener", json!({ "port": port }), &e);
                std::process::exit(1);
            }
        },
    };
    log::info(
        &format!("free-tools-processor listening on port {port}"),
        json!({
            "allowedOrigins": processor.config().allowed_origins,
            "requestTimeoutMs": processor.config().request_timeout.as_millis() as u64,
        }),
    );
    let cleanup = processor.spawn_cleanup();
    let app = processor.router().into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown()).await {
        log::error("Server error", json!({}), &e);
    }
    cleanup.abort();
}

async fn shutdown() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }
    log::info("Shutting down", json!({}));
}
