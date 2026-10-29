//! `login`, `logout`, `whoami` (WEB-049/050).
use std::io::{BufRead, IsTerminal};
use std::time::Duration;

use transgate_core::presentation::app::{AppIntent, AppVm, AuthIntent, balance_text};
use transgate_core::presentation::msg::{Msg, auth, common};
use transgate_native::{Loopback, OpenUrl, browser};

use super::{note, out, presenter};
use crate::args::Login;
use crate::context::{Context, Session};
use crate::exit::{Exit, Failure, Outcome, auth_exit};

/// How long the browser step of Google sign-in may take: a person is in the loop.
const BROWSER_WAIT: Duration = Duration::from_secs(300);

/// A sign-in verdict from the view: the account's email, or the error the form shows.
type Verdict = Result<(), Msg>;

pub fn login(ctx: &Context, args: &Login) -> Outcome {
    if args.google { google(ctx, args.no_browser) } else { password(ctx, args) }
}

fn password(ctx: &Context, args: &Login) -> Outcome {
    let email = args.email.clone().ok_or_else(|| Failure::usage("--email (or TRANSGATE_EMAIL) is required for password sign-in"))?;
    let password = read_password(ctx, args)?;
    let session = ctx.start_home()?;
    match ctx.settled(&session)? {
        AppVm::SignedIn(_) => return done(ctx, &email),
        AppVm::SignedOut(_) => {}
        AppVm::Erased { message, .. } => return Err(Failure::new(Exit::Refused, ctx.say(&message))),
        AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } => return Err(ctx.unavailable()),
    }
    for intent in [AuthIntent::EmailChanged { text: email.clone() }, AuthIntent::PasswordChanged { text: password }, AuthIntent::Submitted] {
        session.shell.dispatch(AppIntent::Auth(intent)).map_err(presenter)?;
    }
    verdict(ctx, &session, ctx.timeout)?;
    done(ctx, &email)
}

fn google(ctx: &Context, no_browser: bool) -> Outcome {
    let loopback = Loopback::bind().map_err(|e| Failure::new(Exit::Refused, e.to_string()))?;
    let origin = loopback.origin().map_err(|e| Failure::new(Exit::Refused, e.to_string()))?;
    let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opened_signal = opened.clone();
    let vault = ctx.vault.clone();
    let open: OpenUrl = Box::new(move |url| {
        if !vault.has_error() && (no_browser || !browser::open(url)) {
            out(url);
        }
        opened_signal.store(true, std::sync::atomic::Ordering::Release);
    });
    let first = ctx.start(&ctx.home_url(), &origin, open)?;
    match ctx.settled(&first)? {
        AppVm::SignedIn(_) => return account(ctx, &first),
        AppVm::SignedOut(_) => {}
        AppVm::Erased { message, .. } => return Err(Failure::new(Exit::Refused, ctx.say(&message))),
        AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } => return Err(ctx.unavailable()),
    }
    first.shell.dispatch(AppIntent::Auth(AuthIntent::GoogleChosen)).map_err(presenter)?;
    first.shell.signal().wait_for(ctx.timeout, || (ctx.vault.has_error() || opened.load(std::sync::atomic::Ordering::Acquire)).then_some(())).ok_or_else(|| ctx.unavailable())?;
    if ctx.vault.has_error() { return Err(ctx.unavailable()); }
    // The browser returns to the loopback origin with a PKCE code. The verifier core
    // stored in the vault makes a second presenter, started at the callback target,
    // exchange it: `Started` is the one place core reads a callback.
    let page = ctx.say(&auth("sign_in.loading_button_label"));
    let target = loopback
        .wait_callback(BROWSER_WAIT, &page)
        .map_err(|e| Failure::new(Exit::Refused, e.to_string()))?
        .ok_or_else(|| Failure::new(Exit::Auth, ctx.say(&common("Please sign in and try again."))))?;
    drop(first);
    let second = ctx.start(&target, &origin, Box::new(|_| {}))?;
    verdict(ctx, &second, ctx.timeout)?;
    account(ctx, &second)
}

/// Prints the signed-in account's email once its profile has loaded.
fn account(ctx: &Context, session: &Session) -> Outcome {
    let email = session
        .shell
        .wait(ctx.timeout, |view| match view {
            AppVm::SignedIn(vm) => vm.profile.as_ref().map(|p| p.email.clone()),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())?;
    done(ctx, &email)
}

/// Waits for the sign-in form to settle: signed in, or an error shown by the form.
fn verdict(ctx: &Context, session: &Session, timeout: Duration) -> Outcome {
    let answer = session
        .shell
        .wait(timeout, |view| match view {
            AppVm::SignedIn(_) => Some(Ok(())),
            AppVm::SignedOut(vm) if !vm.auth.busy => vm.auth.error.clone().map(Err),
            _ => None,
        })
        .ok_or_else(|| ctx.unavailable())?;
    let answer: Verdict = answer;
    answer.map_err(|error| Failure::new(auth_exit(&error), ctx.say(&error)))
}

/// Prints the account, after checking the session reached storage: a keyring that
/// refused the write would otherwise look signed in until the next command.
fn done(ctx: &Context, email: &str) -> Outcome {
    if let Some(problem) = ctx.vault.take_error() {
        return Err(Failure::new(Exit::Refused, problem));
    }
    out(email);
    Ok(())
}

fn read_password(ctx: &Context, args: &Login) -> Outcome<String> {
    if args.password_stdin {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).map_err(|e| Failure::usage(format!("reading the password: {e}")))?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    if let Some(password) = std::env::var("TRANSGATE_PASSWORD").ok().filter(|p| !p.is_empty()) {
        return Ok(password);
    }
    if std::io::stdin().is_terminal() {
        let label = format!("{}: ", ctx.say(&auth("sign_in.password_label")));
        return rpassword::prompt_password(label).map_err(|e| Failure::usage(format!("reading the password: {e}")));
    }
    Err(Failure::usage("no password: use --password-stdin, TRANSGATE_PASSWORD, or run on a terminal"))
}

pub fn logout(ctx: &Context) -> Outcome {
    let session = ctx.start_home()?;
    match ctx.settled(&session)? {
        AppVm::SignedIn(_) => {}
        AppVm::SignedOut(_) => return Ok(()),
        AppVm::Erased { .. } => return Ok(()),
        AppVm::Restoring { .. } | AppVm::CheckoutConfirm { .. } => return Err(ctx.unavailable()),
    }
    session.shell.dispatch(AppIntent::SignOutRequested).map_err(presenter)?;
    session.shell.wait(ctx.timeout, |view| matches!(view, AppVm::SignedOut(_)).then_some(())).ok_or_else(|| ctx.unavailable())?;
    if let Some(problem) = ctx.vault.take_error() {
        return Err(Failure::new(Exit::Refused, problem));
    }
    Ok(())
}

pub fn whoami(ctx: &Context) -> Outcome {
    let (session, vm) = ctx.signed_in()?;
    if let Some(profile) = &vm.profile {
        out(&profile.email);
    }
    // The balance arrives after the profile; it is printed when it has, and omitted when it has not.
    let unknown = balance_text(None);
    let balance = session.shell.wait(Duration::from_secs(5), |view| match view {
        AppVm::SignedIn(vm) if vm.balance != unknown => Some(vm.balance.clone()),
        _ => None,
    });
    match balance {
        Some(balance) => out(&ctx.say(&balance)),
        None => note(&ctx.say(&unknown)),
    }
    Ok(())
}
