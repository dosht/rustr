//! Composition root for the server: read configuration, pick adapters, serve.
//!
//! Configuration (environment):
//! - `TASKR_BIND`: listen address, default `127.0.0.1:8787`
//! - `DATABASE_URL`: Postgres. Without it, `TASKR_STORAGE=memory` keeps data
//!   in memory until the process stops; anything else refuses to start.
//! - `SUPABASE_JWT_SECRET` and/or `SUPABASE_JWKS_URL`: how to verify access tokens
//! - `TASKR_DEV_AUTH=1`: also accept `dev:<email>` tokens (local only)
//! - `TASKR_WEB_DIR`: serve the built web app from this folder
//! - `TASKR_CORS_ORIGIN`: allow this browser origin to call the API directly

use std::net::SocketAddr;
use std::sync::Arc;

use taskr_core::shared::access::Identity;
use taskr_core::shared::telemetry::{Level, LogEvent, Telemetry};
use taskr_core::tasks::app::TaskListRepository;
use taskr_core::workspaces::app::WorkspaceRepository;
use taskr_infra::clock::SystemClock;
use taskr_infra::telemetry::JsonLinesTelemetry;
use taskr_server::identity::{DevIdentity, Identities, SupabaseJwt};
use taskr_server::{BUILD, Server, router};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn fail(telemetry: &dyn Telemetry, why: &str) -> ! {
    telemetry.record(LogEvent::error("server.config").field("error", why));
    std::process::exit(2);
}

#[tokio::main]
async fn main() {
    let clock = Arc::new(SystemClock);
    let telemetry: Arc<dyn Telemetry> =
        Arc::new(JsonLinesTelemetry::stderr(Level::Info, clock.clone()));

    let (task_lists, workspaces, storage): (
        Arc<dyn TaskListRepository>,
        Arc<dyn WorkspaceRepository>,
        _,
    ) = match (env("DATABASE_URL"), env("TASKR_STORAGE").as_deref()) {
        (Some(url), _) => {
            let pool = taskr_postgres::connect(&url)
                .await
                .unwrap_or_else(|e| fail(telemetry.as_ref(), &e.0));
            (
                Arc::new(taskr_postgres::PgTaskListRepository::new(pool.clone())),
                Arc::new(taskr_postgres::PgWorkspaceRepository::new(pool)),
                "postgres",
            )
        }
        (None, Some("memory")) => (
            Arc::new(taskr_core::testing::InMemoryTaskListRepository::default()),
            Arc::new(taskr_core::testing::InMemoryWorkspaceRepository::default()),
            "memory",
        ),
        (None, _) => fail(
            telemetry.as_ref(),
            "set DATABASE_URL, or TASKR_STORAGE=memory for a throwaway server",
        ),
    };

    let real: Option<Arc<dyn Identity>> =
        match (env("SUPABASE_JWT_SECRET"), env("SUPABASE_JWKS_URL")) {
            (None, None) => None,
            (secret, jwks) => Some(Arc::new(SupabaseJwt::new(secret.as_deref(), jwks))),
        };
    let dev_auth = env("TASKR_DEV_AUTH").as_deref() == Some("1");
    if dev_auth {
        telemetry.record(
            LogEvent::warn("server.dev_auth").field("note", "dev:<email> tokens are accepted"),
        );
    }
    if real.is_none() && !dev_auth {
        fail(
            telemetry.as_ref(),
            "set SUPABASE_JWT_SECRET or SUPABASE_JWKS_URL (or TASKR_DEV_AUTH=1 locally)",
        );
    }
    let identity = Arc::new(Identities {
        dev: dev_auth.then(|| Arc::new(DevIdentity) as Arc<dyn Identity>),
        real,
    });

    let server = Server { task_lists, workspaces, identity, clock, telemetry: telemetry.clone() };
    let mut app = router(server);
    if let Some(dir) = env("TASKR_WEB_DIR") {
        let index = format!("{dir}/index.html");
        app = app.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index)));
    }
    if let Some(origin) = env("TASKR_CORS_ORIGIN") {
        let origin = origin.parse().unwrap_or_else(|_| {
            fail(telemetry.as_ref(), "TASKR_CORS_ORIGIN is not a valid origin")
        });
        app = app.layer(
            CorsLayer::new()
                .allow_origin([origin])
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        );
    }

    let bind: SocketAddr = env("TASKR_BIND")
        .unwrap_or_else(|| "127.0.0.1:8787".into())
        .parse()
        .unwrap_or_else(|_| fail(telemetry.as_ref(), "TASKR_BIND is not an address"));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|e| fail(telemetry.as_ref(), &e.to_string()));
    telemetry.record(
        LogEvent::info("server.started")
            .field("addr", bind)
            .field("storage", storage)
            .field("build", BUILD),
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap_or_else(|e| fail(telemetry.as_ref(), &e.to_string()));
}
