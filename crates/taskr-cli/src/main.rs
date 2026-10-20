//! Composition root for the CLI: pick adapters, build the use cases, map
//! results to stdout and exit codes. No business rules here.
//!
//! Two modes, same commands. Signed out, tasks live in local files. After
//! `taskr login`, the same commands go to a taskr server through the same
//! `TasksApi` port, in the workspace picked with `--workspace`.

mod remote;

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use taskr_core::session::app::{AuthClient, SessionStore, describe_auth};
use taskr_core::shared::access::{AccessError, Role, WorkspaceId, describe_access};
use taskr_core::shared::clock::Clock;
use taskr_core::shared::telemetry::{Level, Telemetry};
use taskr_core::tasks::app::{LocalTasks, Scope, TaskSummary, Tasks, TasksApi, TasksError};
use taskr_core::tasks::presentation::{Align, Intent, TaskListController, TaskListVm, describe};
use taskr_core::workspaces::app::{WorkspacesApi, WorkspacesError};
use taskr_core::workspaces::presentation::describe as describe_workspaces;
use taskr_infra::clock::SystemClock;
use taskr_infra::json_file::JsonFileTaskListRepository;
use taskr_infra::paths;
use taskr_infra::session_file::FileSessionStore;
use taskr_infra::telemetry::JsonLinesTelemetry;

use remote::Remote;

const EXIT_REJECTED: u8 = 1;
const EXIT_STORAGE: u8 = 3;
const EXIT_SIGNED_OUT: u8 = 4;

#[derive(Parser)]
#[command(
    name = "taskr",
    version,
    about = "Small task list, shared with the TUI, web and mobile apps"
)]
struct Cli {
    /// Data directory (defaults to $TASKR_HOME or the platform data dir)
    #[arg(long, global = true)]
    home: Option<PathBuf>,

    /// Print telemetry events to stderr as JSON lines
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Workspace name or id, when signed in (defaults to the first one)
    #[arg(short, long, global = true)]
    workspace: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a task
    Add { title: Vec<String> },
    /// List tasks
    #[command(alias = "ls")]
    List {
        #[arg(long, value_enum, default_value_t = ScopeArg::Open)]
        scope: ScopeArg,
        /// Only tasks whose title contains this text
        #[arg(long)]
        filter: Option<String>,
        /// Machine readable output
        #[arg(long)]
        json: bool,
    },
    /// Mark a task as done
    Done { id: u32 },
    /// Mark a done task as open again
    Reopen { id: u32 },
    /// Sign in to a taskr server; later commands use it until `logout`
    Login {
        /// API root, for example http://localhost:8787/api
        #[arg(long)]
        server: String,
        #[arg(long)]
        email: String,
        /// Supabase Auth URL, for example https://<project>.supabase.co/auth/v1
        #[arg(long, conflicts_with = "dev")]
        auth_url: Option<String>,
        /// Supabase anon (publishable) key
        #[arg(long, env = "TASKR_SUPABASE_KEY", hide_env_values = true)]
        api_key: Option<String>,
        /// Use dev tokens (only against a server started with TASKR_DEV_AUTH=1)
        #[arg(long)]
        dev: bool,
        /// Create the account first
        #[arg(long)]
        sign_up: bool,
    },
    /// Forget the server session and go back to local tasks
    Logout,
    /// Show who you are signed in as, and your workspaces
    Whoami,
    /// List, add, change or remove workspace members
    Members {
        #[command(subcommand)]
        action: Option<MembersCommand>,
    },
}

#[derive(Subcommand)]
enum MembersCommand {
    List,
    /// Add a member, or change their role
    Set {
        email: String,
        role: RoleArg,
    },
    Remove {
        email: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum RoleArg {
    Owner,
    Editor,
    Viewer,
}

impl From<RoleArg> for Role {
    fn from(r: RoleArg) -> Role {
        match r {
            RoleArg::Owner => Role::Owner,
            RoleArg::Editor => Role::Editor,
            RoleArg::Viewer => Role::Viewer,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ScopeArg {
    Open,
    Done,
    All,
}

impl From<ScopeArg> for Scope {
    fn from(s: ScopeArg) -> Scope {
        match s {
            ScopeArg::Open => Scope::Open,
            ScopeArg::Done => Scope::Done,
            ScopeArg::All => Scope::All,
        }
    }
}

#[derive(Serialize)]
struct TaskJson<'a> {
    id: u32,
    title: &'a str,
    done: bool,
    created_at: i64,
    done_at: Option<i64>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let home = cli.home.clone().unwrap_or_else(paths::data_dir);
    let clock = Arc::new(SystemClock);
    let min_level = if cli.verbose { Level::Debug } else { Level::Error };
    let telemetry: Arc<dyn Telemetry> =
        Arc::new(JsonLinesTelemetry::stderr(min_level, clock.clone()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a single-threaded runtime always starts");

    let ctx = Context { home, clock, telemetry, workspace: cli.workspace };
    match runtime.block_on(run(cli.command, ctx)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure { message, code }) => {
            eprintln!("taskr: {message}");
            ExitCode::from(code)
        }
    }
}

struct Context {
    home: PathBuf,
    clock: Arc<SystemClock>,
    telemetry: Arc<dyn Telemetry>,
    workspace: Option<String>,
}

struct Failure {
    message: String,
    code: u8,
}

impl Failure {
    fn rejected(message: impl Into<String>) -> Self {
        Failure { message: message.into(), code: EXIT_REJECTED }
    }

    fn signed_out() -> Self {
        Failure {
            message: "Not signed in to a server. Run `taskr login` first.".into(),
            code: EXIT_SIGNED_OUT,
        }
    }
}

fn access_code(a: &AccessError) -> u8 {
    match a {
        AccessError::Unauthenticated => EXIT_SIGNED_OUT,
        _ => EXIT_REJECTED,
    }
}

impl From<TasksError> for Failure {
    fn from(e: TasksError) -> Self {
        let code = match &e {
            TasksError::Rejected(_) => EXIT_REJECTED,
            TasksError::Access(a) => access_code(a),
            TasksError::Storage(_) => EXIT_STORAGE,
        };
        Failure { message: describe(&e), code }
    }
}

impl From<WorkspacesError> for Failure {
    fn from(e: WorkspacesError) -> Self {
        let code = match &e {
            WorkspacesError::Rejected(_) => EXIT_REJECTED,
            WorkspacesError::Access(a) => access_code(a),
            WorkspacesError::Storage(_) => EXIT_STORAGE,
        };
        Failure { message: describe_workspaces(&e), code }
    }
}

/// Where task commands go: the local files, or a server workspace.
struct Target {
    api: Arc<dyn TasksApi>,
    workspace: WorkspaceId,
}

async fn target(ctx: &Context) -> Result<Target, Failure> {
    match Remote::open(&ctx.home) {
        None => {
            if ctx.workspace.is_some() {
                return Err(Failure::signed_out());
            }
            let repo = Arc::new(JsonFileTaskListRepository::new(&ctx.home));
            let tasks = Tasks::new(repo, ctx.clock.clone(), ctx.telemetry.clone());
            Ok(Target { api: Arc::new(LocalTasks::new(tasks)), workspace: WorkspaceId::local() })
        }
        Some(remote) => {
            let workspace = remote.workspace(ctx.workspace.as_deref()).await?;
            Ok(Target { api: remote.api.clone(), workspace })
        }
    }
}

async fn run(command: Command, ctx: Context) -> Result<(), Failure> {
    match command {
        Command::Login { server, email, auth_url, api_key, dev, sign_up } => {
            let config = remote::Config { server, auth_url, api_key, dev };
            let password = if dev { String::new() } else { read_password()? };
            let auth: Box<dyn AuthClient> = config
                .auth()
                .ok_or_else(|| Failure::rejected("Pass --auth-url (Supabase Auth) or --dev."))?;
            let session = if sign_up {
                match auth.sign_up(&email, &password).await {
                    Ok(taskr_core::session::app::SignUp::SignedIn(s)) => s,
                    Ok(taskr_core::session::app::SignUp::ConfirmationSent) => {
                        println!("Check your email to confirm the account, then run login again.");
                        return Ok(());
                    }
                    Err(e) => return Err(Failure::rejected(describe_auth(&e))),
                }
            } else {
                auth.sign_in(&email, &password)
                    .await
                    .map_err(|e| Failure { message: describe_auth(&e), code: EXIT_SIGNED_OUT })?
            };
            config.save(&ctx.home).map_err(|e| Failure { message: e, code: EXIT_STORAGE })?;
            FileSessionStore::new(paths::session_file(&ctx.home)).save(&session);
            let remote = Remote::open(&ctx.home).ok_or_else(Failure::signed_out)?;
            print_me(&remote).await
        }
        Command::Logout => {
            FileSessionStore::new(paths::session_file(&ctx.home)).clear();
            remote::Config::remove(&ctx.home);
            println!("Signed out. Commands use local tasks again.");
            Ok(())
        }
        Command::Whoami => {
            let remote = Remote::open(&ctx.home).ok_or_else(Failure::signed_out)?;
            print_me(&remote).await
        }
        Command::Members { action } => {
            let remote = Remote::open(&ctx.home).ok_or_else(Failure::signed_out)?;
            let ws = remote.workspace(ctx.workspace.as_deref()).await?;
            match action.unwrap_or(MembersCommand::List) {
                MembersCommand::List => {}
                MembersCommand::Set { email, role } => {
                    remote.api.set_member(&ws, email, role.into()).await?;
                }
                MembersCommand::Remove { email } => remote.api.remove_member(&ws, email).await?,
            }
            for m in remote.api.members(&ws).await? {
                println!("{:<7} {}", m.role.label(), m.email);
            }
            Ok(())
        }
        Command::Add { title } => {
            let t = target(&ctx).await?;
            print_change("Added", t.api.add(&t.workspace, title.join(" ")).await?);
            Ok(())
        }
        Command::Done { id } => {
            let t = target(&ctx).await?;
            print_change("Done", t.api.complete(&t.workspace, id).await?);
            Ok(())
        }
        Command::Reopen { id } => {
            let t = target(&ctx).await?;
            print_change("Reopened", t.api.reopen(&t.workspace, id).await?);
            Ok(())
        }
        Command::List { scope, filter, json: true } => {
            let t = target(&ctx).await?;
            let scope = Scope::from(scope);
            let needle = filter.unwrap_or_default().to_lowercase();
            let rows: Vec<TaskSummary> = t
                .api
                .list(&t.workspace)
                .await?
                .into_iter()
                .filter(|t| scope.includes(t.is_done()))
                .filter(|t| t.title.to_lowercase().contains(&needle))
                .collect();
            let out: Vec<TaskJson> = rows
                .iter()
                .map(|t| TaskJson {
                    id: t.id,
                    title: &t.title,
                    done: t.is_done(),
                    created_at: t.created_at,
                    done_at: t.done_at,
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&out).expect("serializable"));
            Ok(())
        }
        Command::List { scope, filter, json: false } => {
            let t = target(&ctx).await?;
            let clock: Arc<dyn Clock> = ctx.clock.clone();
            let screen = TaskListController::new(t.api, t.workspace, clock);
            screen.dispatch(Intent::ScopeChanged(scope.into())).await;
            screen.dispatch(Intent::FilterChanged(filter.unwrap_or_default())).await;
            let vm = screen.dispatch(Intent::Opened).await;
            // Listing only runs the load effect, which can only fail on storage.
            if let Some(message) = vm.error {
                let code = if message == describe_access(&AccessError::Unauthenticated) {
                    EXIT_SIGNED_OUT
                } else {
                    EXIT_STORAGE
                };
                return Err(Failure { message, code });
            }
            print!("{}", render_table(&vm));
            Ok(())
        }
    }
}

async fn print_me(remote: &Remote) -> Result<(), Failure> {
    let me = remote.api.me().await?;
    println!("Signed in as {} on {}", me.email, remote.server());
    for w in me.workspaces {
        println!("  {:<24} {:<7} {}", w.name, w.role.label(), w.id);
    }
    Ok(())
}

/// From `TASKR_PASSWORD`, or one line of stdin, so scripts can pipe it in.
fn read_password() -> Result<String, Failure> {
    if let Ok(p) = std::env::var("TASKR_PASSWORD") {
        return Ok(p);
    }
    eprint!("Password: ");
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| Failure::rejected(format!("could not read the password: {e}")))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn print_change(verb: &str, task: TaskSummary) {
    println!("{verb} #{} {}", task.id, task.title);
}

/// Plain text table driven by the shared column spec, the same one the TUI uses.
fn render_table(vm: &TaskListVm) -> String {
    let tabs: Vec<String> = vm
        .tabs
        .iter()
        .map(|t| if t.selected { format!("[{}]", t.label) } else { t.label.clone() })
        .collect();
    let mut out = format!("{}\n", tabs.join("  "));
    if let Some(msg) = &vm.empty_message {
        out.push_str(msg);
        out.push('\n');
        return out;
    }
    let widths: Vec<usize> = (0..vm.columns.len())
        .map(|i| {
            vm.rows
                .iter()
                .map(|r| r.cells[i].chars().count())
                .chain([vm.columns[i].title.len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| match vm.columns[i].align {
                Align::Left => format!("{c:<w$}", w = widths[i]),
                Align::Right => format!("{c:>w$}", w = widths[i]),
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    out.push_str(&format!("    {}\n", line(vm.columns.iter().map(|c| c.title).collect())));
    for row in &vm.rows {
        let mark = if row.done { "[x]" } else { "[ ]" };
        out.push_str(&format!("{mark} {}\n", line(row.cells.iter().map(String::as_str).collect())));
    }
    out
}
