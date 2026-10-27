//! One module per command family. Each command drives the shared presenter
//! headlessly: dispatch intents, wait until the view says it is done, print what
//! the view model holds. No upload ordering, freemium bound or error rule is
//! repeated here.
use std::io::Write;
use std::time::Duration;

use transgate_core::presentation::msg::Msg;
use transgate_ffi::FfiError;

use crate::args::Command;
use crate::context::Context;
use crate::exit::{Exit, Failure, Outcome};

mod auth;
mod jobs;
mod upload;

/// How long a command waits for work the host does at its own pace (an upload,
/// a started transcription); the `--timeout` bounds only a host that is not answering.
pub const WORK_WAIT: Duration = Duration::from_secs(6 * 60 * 60);

pub fn run(ctx: &Context, command: &Command) -> Outcome {
    match command {
        Command::Login(args) => auth::login(ctx, args),
        Command::Logout => auth::logout(ctx),
        Command::Whoami => auth::whoami(ctx),
        Command::Upload(args) => upload::files(ctx, args),
        Command::Youtube(args) => upload::youtube(ctx, args),
        Command::Jobs(args) => jobs::list(ctx, args),
        Command::Show(args) => jobs::show(ctx, args.id),
        Command::Transcript(args) => jobs::transcript(ctx, args),
        Command::Export(args) => jobs::export(ctx, args),
        Command::Delete(args) => jobs::delete(ctx, args.id),
    }
}

/// A result line on stdout. A closed pipe ends the program quietly, as `head` expects.
pub fn out(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// A progress line on stderr.
pub fn note(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

pub fn presenter(error: FfiError) -> Failure {
    Failure::new(Exit::Refused, error.to_string())
}

/// `heading` then each paragraph, translated, as one stderr-ready message.
pub fn paragraphs(ctx: &Context, heading: &Msg, body: &[Msg]) -> String {
    let mut lines = vec![ctx.say(heading)];
    lines.extend(body.iter().map(|m| ctx.say(m)));
    lines.join("\n")
}
