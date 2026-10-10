use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use taskr_core::tasks::app::{StorageError, TaskListRepository};
use taskr_core::tasks::domain::TaskList;

use crate::snapshot_json;

pub struct JsonFileTaskListRepository {
    path: PathBuf,
}

impl JsonFileTaskListRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl TaskListRepository for JsonFileTaskListRepository {
    fn load(&self) -> Result<TaskList, StorageError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => snapshot_json::decode(&text),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(TaskList::new()),
            Err(e) => Err(io_error("read", &self.path, e)),
        }
    }

    /// Writes to a sibling temp file and renames it, so a crash mid-write
    /// never leaves a half-written task file behind.
    fn save(&self, list: &TaskList) -> Result<(), StorageError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error("create", dir, e))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, snapshot_json::encode(list)).map_err(|e| io_error("write", &tmp, e))?;
        fs::rename(&tmp, &self.path).map_err(|e| io_error("replace", &self.path, e))
    }
}

fn io_error(action: &str, path: &Path, e: std::io::Error) -> StorageError {
    StorageError(format!("could not {action} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskr_core::testing::task_list_repository_contract;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("taskr-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn honours_the_repository_contract() {
        let dir = temp_dir("contract");
        task_list_repository_contract(
            || JsonFileTaskListRepository::new(dir.join("nested/tasks.json")),
            |r| JsonFileTaskListRepository::new(r.path()),
        );
        assert!(!dir.join("nested/tasks.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_a_storage_error_not_a_panic() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.json");
        fs::write(&path, "{not json").unwrap();
        let err = JsonFileTaskListRepository::new(&path).load().unwrap_err();
        assert!(err.0.contains("unreadable task file"), "{}", err.0);
    }
}
