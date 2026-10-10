//! Composition root for the CLI: pick adapters, build the use cases, map
//! results to stdout and exit codes. No business rules here.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use taskr_core::shared::telemetry::{Level, Telemetry};
use taskr_core::tasks::app::{Scope, TaskSummary, Tasks, TasksError};
use taskr_core::tasks::presentation::{Align, Intent, TaskListController, TaskListVm, describe};
use taskr_infra::clock::SystemClock;
use taskr_infra::json_file::JsonFileTaskListRepository;
use taskr_infra::paths;
use taskr_infra::telemetry::JsonLinesTelemetry;

const EXIT_REJECTED: u8 = 1;
const EXIT_STORAGE: u8 = 3;

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
    let repo = Arc::new(JsonFileTaskListRepository::new(paths::tasks_file(&home)));
    let tasks = Tasks::new(repo, clock, telemetry);

    match run(cli.command, tasks) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure { message, code }) => {
            eprintln!("taskr: {message}");
            ExitCode::from(code)
        }
    }
}

struct Failure {
    message: String,
    code: u8,
}

impl From<TasksError> for Failure {
    fn from(e: TasksError) -> Self {
        let code = match e {
            TasksError::Rejected(_) => EXIT_REJECTED,
            TasksError::Storage(_) => EXIT_STORAGE,
        };
        Failure { message: describe(&e), code }
    }
}

fn run(command: Command, tasks: Tasks) -> Result<(), Failure> {
    match command {
        Command::Add { title } => print_change("Added", tasks.add(&title.join(" "))?),
        Command::Done { id } => print_change("Done", tasks.complete(id)?),
        Command::Reopen { id } => print_change("Reopened", tasks.reopen(id)?),
        Command::List { scope, filter, json: true } => {
            let needle = filter.unwrap_or_default().to_lowercase();
            let rows: Vec<TaskSummary> = tasks
                .list(scope.into())?
                .into_iter()
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
        }
        Command::List { scope, filter, json: false } => {
            let mut screen = TaskListController::new(tasks);
            screen.dispatch(Intent::ScopeChanged(scope.into()));
            screen.dispatch(Intent::FilterChanged(filter.unwrap_or_default()));
            let vm = screen.dispatch(Intent::Opened);
            // Listing only runs the load effect, which can only fail on storage.
            if let Some(message) = vm.error {
                return Err(Failure { message, code: EXIT_STORAGE });
            }
            print!("{}", render_table(&vm));
        }
    }
    Ok(())
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
