//! The command line. Flag and command names are developer vocabulary, not product
//! copy; everything a person reads from a result comes from the catalogs.
use clap::{Args, Parser, Subcommand};
use transgate_core::languages::Language;

#[derive(Parser, Debug)]
#[command(name = "transgate", version, about = "Transgate from the terminal", propagate_version = true)]
pub struct Cli {
    /// Origin of the Transgate host, which serves /api and /auth/v1.
    #[arg(long, env = "TRANSGATE_HOST", global = true)]
    pub host: Option<String>,
    /// The identity service's public API key, when the host needs one.
    #[arg(long, env = "TRANSGATE_AUTH_KEY", global = true, hide_env_values = true)]
    pub auth_key: Option<String>,
    /// Interface language of the output (one of the 16 catalogs).
    #[arg(long, env = "TRANSGATE_LOCALE", global = true, value_parser = locale)]
    pub locale: Option<String>,
    /// Seconds a command waits for the host before it gives up.
    #[arg(long, default_value_t = 60, global = true, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,
    #[command(subcommand)]
    pub command: Command,
}

fn locale(code: &str) -> Result<String, String> {
    transgate_i18n::LOCALES.contains(&code).then(|| code.to_string()).ok_or_else(|| format!("expected one of {}", transgate_i18n::LOCALES.join(", ")))
}

/// A language by ISO code (`tr`) or stored name (`Turkish`).
pub fn language(text: &str) -> Result<Language, String> {
    Language::from_code(text).or_else(|| Language::from_name(text)).ok_or_else(|| format!("unknown language {text:?}"))
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Sign in with a password, or with Google through a loopback redirect.
    Login(Login),
    /// Sign out and forget the stored session.
    Logout,
    /// Show the signed-in account.
    Whoami,
    /// Upload audio or video files and start transcribing them.
    Upload(Upload),
    /// Add a YouTube video and start transcribing it.
    Youtube(Youtube),
    /// List your jobs, newest first.
    Jobs(Jobs),
    /// Show one job's title, duration and status.
    Show(JobId),
    /// Print a job's transcript, or one of its translations.
    Transcript(Transcript),
    /// Save a job's export as a zip.
    Export(Export),
    /// Delete a job.
    Delete(JobId),
}

#[derive(Args, Debug)]
pub struct Login {
    /// Email address (password sign-in).
    #[arg(long, env = "TRANSGATE_EMAIL")]
    pub email: Option<String>,
    /// Read the password from the first line of standard input.
    #[arg(long)]
    pub password_stdin: bool,
    /// Sign in with Google: listens on 127.0.0.1 for the redirect (RFC 8252).
    #[arg(long, conflicts_with_all = ["email", "password_stdin"])]
    pub google: bool,
    /// Print the Google URL instead of opening a browser.
    #[arg(long, requires = "google")]
    pub no_browser: bool,
}

#[derive(Args, Debug)]
pub struct Upload {
    /// Files to upload.
    #[arg(required = true)]
    pub files: Vec<std::path::PathBuf>,
    /// Language spoken in the files (code or name); auto-detected when omitted.
    #[arg(long, value_parser = language)]
    pub language: Option<Language>,
    /// Translate into this language; needs --language as the source.
    #[arg(long, value_parser = language, requires = "language")]
    pub translate_to: Option<Language>,
    /// Label the speakers.
    #[arg(long)]
    pub speakers: bool,
}

#[derive(Args, Debug)]
pub struct Youtube {
    pub url: String,
    /// Language spoken in the video (code or name); detected when omitted.
    #[arg(long, value_parser = language)]
    pub language: Option<Language>,
}

#[derive(Args, Debug)]
pub struct Jobs {
    /// Load every page, not only the first.
    #[arg(long)]
    pub all: bool,
    /// Print at most this many jobs.
    #[arg(long)]
    pub limit: Option<usize>,
    /// Print the job rows as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct JobId {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct Transcript {
    pub id: i64,
    /// Print this translation (language code or name) instead of the original.
    #[arg(long, value_parser = language)]
    pub translation: Option<Language>,
    /// Print the lines as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct Export {
    pub id: i64,
    /// Every format (subtitles included) instead of what the transcript page shows.
    #[arg(long)]
    pub all_formats: bool,
    /// Directory to save the zip in.
    #[arg(long, default_value = ".")]
    pub out: std::path::PathBuf,
}
