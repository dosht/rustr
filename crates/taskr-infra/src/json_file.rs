use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use taskr_core::shared::access::WorkspaceId;
use taskr_core::shared::events::DomainEvent;
use taskr_core::shared::future::BoxFuture;
use taskr_core::shared::storage::{Revision, SaveError};
use taskr_core::tasks::app::{StorageError, TaskListRepository};
use taskr_core::tasks::domain::TaskList;

use crate::snapshot_json;

/// One JSON file per workspace under a data directory. The local workspace
/// keeps the original `tasks.json` name, so existing data still loads.
///
/// No outbox: this is the single-device, signed-out mode, so domain events
/// have no consumer and are dropped. The revision check narrows, but does not
/// close, the race between two processes (CLI and TUI) saving at once.
pub struct JsonFileTaskListRepository {
    dir: PathBuf,
}

impl JsonFileTaskListRepository {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, workspace: &WorkspaceId) -> PathBuf {
        if *workspace == WorkspaceId::local() {
            return crate::paths::tasks_file(&self.dir);
        }
        let safe: String = workspace
            .0
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.dir.join("workspaces").join(format!("{safe}.json"))
    }

    fn read(&self, path: &Path) -> Result<(TaskList, Revision), StorageError> {
        match fs::read_to_string(path) {
            Ok(text) => snapshot_json::decode(&text),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok((TaskList::new(), Revision::NEW)),
            Err(e) => Err(io_error("read", path, e)),
        }
    }

    /// Writes to a sibling temp file and renames it, so a crash mid-write
    /// never leaves a half-written task file behind.
    fn write(&self, path: &Path, list: &TaskList, expected: Revision) -> Result<(), SaveError> {
        let (_, current) = self.read(path)?;
        if current != expected {
            return Err(SaveError::Conflict);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error("create", dir, e))?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, snapshot_json::encode(list, expected.next()))
            .map_err(|e| io_error("write", &tmp, e))?;
        fs::rename(&tmp, path).map_err(|e| io_error("replace", path, e))?;
        Ok(())
    }
}

impl TaskListRepository for JsonFileTaskListRepository {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
        Box::pin(async move { self.read(&self.path_for(workspace)) })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        list: &'a TaskList,
        expected: Revision,
        _events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move { self.write(&self.path_for(workspace), list, expected) })
    }
}

fn io_error(action: &str, path: &Path, e: std::io::Error) -> StorageError {
    StorageError(format!("could not {action} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use taskr_core::testing::{block_on, task_list_repository_contract};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("taskr-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn honours_the_repository_contract() {
        let dir = temp_dir("contract");
        let next = AtomicU32::new(0);
        task_list_repository_contract(
            || JsonFileTaskListRepository::new(dir.join("nested")),
            |r| JsonFileTaskListRepository::new(r.dir()),
            |_| match next.fetch_add(1, Ordering::SeqCst) {
                0 => WorkspaceId::local(),
                n => WorkspaceId(format!("team/{n}")),
            },
        );
        assert!(dir.join("nested/tasks.json").exists());
        assert!(!dir.join("nested/tasks.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_a_storage_error_not_a_panic() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tasks.json"), "{not json").unwrap();
        let err = block_on(JsonFileTaskListRepository::new(&dir).load(&WorkspaceId::local()))
            .unwrap_err();
        assert!(err.0.contains("unreadable task file"), "{}", err.0);
    }

    #[test]
    fn workspace_ids_cannot_escape_the_data_dir() {
        let repo = JsonFileTaskListRepository::new("/data");
        let path = repo.path_for(&WorkspaceId("../../etc/passwd".into()));
        assert_eq!(path, PathBuf::from("/data/workspaces/______etc_passwd.json"));
    }
}
