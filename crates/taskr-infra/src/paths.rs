use std::path::PathBuf;

pub const HOME_ENV: &str = "TASKR_HOME";

/// Where the CLI and TUI keep their data, shared so both see the same tasks.
/// `TASKR_HOME` wins, then the platform data dir, then the current directory.
pub fn data_dir() -> PathBuf {
    std::env::var_os(HOME_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("taskr")))
        .unwrap_or_else(|| PathBuf::from(".taskr"))
}

pub fn tasks_file(dir: &std::path::Path) -> PathBuf {
    dir.join("tasks.json")
}

pub fn log_file(dir: &std::path::Path) -> PathBuf {
    dir.join("taskr.log.jsonl")
}
