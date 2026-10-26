//! Operations entry point: apply the billing migrations with owner authority
//! and create the restricted login a running server must use.
//!
//! Environment:
//! - `BILLING_MIGRATION_DATABASE_URL`: a role that owns the schema (and may
//!   create roles). Used only here, never by a running server.
//! - `BILLING_RUNTIME_LOGIN`: login to create or re-key, default `billing_app`.
//! - `BILLING_RUNTIME_PASSWORD`: its password. Required.

use std::process::ExitCode;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = env("BILLING_MIGRATION_DATABASE_URL") else {
        eprintln!("billing-provision: set BILLING_MIGRATION_DATABASE_URL to the owner connection");
        return ExitCode::from(2);
    };
    let Some(password) = env("BILLING_RUNTIME_PASSWORD") else {
        eprintln!("billing-provision: set BILLING_RUNTIME_PASSWORD");
        return ExitCode::from(2);
    };
    let login = env("BILLING_RUNTIME_LOGIN").unwrap_or_else(|| "billing_app".into());
    let result = async {
        let owner = billing_postgres::connect_owner(&url).await?;
        billing_postgres::migrate(&owner).await?;
        billing_postgres::provision_runtime_login(&owner, &login, &password).await
    }
    .await;
    match result {
        Ok(()) => {
            println!("billing schema migrated; runtime login {login:?} is ready");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("billing-provision: {e}");
            ExitCode::FAILURE
        }
    }
}
