//! `transgate`: the command line shell (P8 D5, D13). It starts the shared
//! presenter (`transgate-ffi`'s `TransgatePresenter`, which runs the `AppDriver` on
//! a current-thread runtime), forwards core intents, and prints core view models
//! through `translate()`.
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{CommandFactory, Parser, error::ErrorKind};
use transgate_native::{HostConfig, Vault};

mod args;
mod commands;
mod context;
mod exit;

pub use exit::Exit;

use args::{Cli, Command};
use context::Context;

/// The first of the 16 catalogs named by `LC_ALL`, `LC_MESSAGES` or `LANG` (`tr_TR.UTF-8` is `tr`), else English.
fn environment_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter_map(|value| value.split(['_', '.', '-', '@']).next().map(str::to_string))
        .find(|code| transgate_i18n::LOCALES.contains(&code.as_str()))
        .unwrap_or_else(|| "en".into())
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let Some(host) = cli.host.clone().filter(|h| !h.trim().is_empty()) else {
        Cli::command().error(ErrorKind::MissingRequiredArgument, "--host (or TRANSGATE_HOST) is required").exit();
    };
    let out_dir = match &cli.command {
        Command::Export(export) => export.out.clone(),
        _ => PathBuf::from("."),
    };
    let host = HostConfig::from_origin(&host, cli.auth_key.clone().filter(|k| !k.is_empty()));
    let vault = Arc::new(Vault::from_env().for_host(&host));
    let ctx = Context {
        locale: cli.locale.clone().unwrap_or_else(environment_locale),
        host,
        vault,
        timeout: Duration::from_secs(cli.timeout),
        out_dir,
    };
    let result = commands::run(&ctx, &cli.command);
    let result = match ctx.vault.take_error() {
        Some(error) => Err(crate::exit::Failure::new(crate::exit::Exit::Refused, error)),
        None => result,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "{}", failure.message);
            failure.exit.into()
        }
    }
}
