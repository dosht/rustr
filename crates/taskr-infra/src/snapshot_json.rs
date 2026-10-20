//! The storage format for a task list. Versioned so future migrations have a hook.

use serde::{Deserialize, Serialize};
use taskr_core::shared::storage::Revision;
use taskr_core::tasks::app::StorageError;
use taskr_core::tasks::domain::{TaskList, TaskListSnapshot, TaskSnapshot};

const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FileV1 {
    version: u32,
    #[serde(default)]
    revision: u64,
    next_id: u32,
    tasks: Vec<TaskV1>,
}

#[derive(Serialize, Deserialize)]
struct TaskV1 {
    id: u32,
    title: String,
    created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    done_at: Option<i64>,
}

pub fn encode(list: &TaskList, revision: Revision) -> String {
    let s = list.snapshot();
    let file = FileV1 {
        version: FORMAT_VERSION,
        revision: revision.0,
        next_id: s.next_id,
        tasks: s
            .tasks
            .into_iter()
            .map(|t| TaskV1 {
                id: t.id,
                title: t.title,
                created_at: t.created_at,
                done_at: t.done_at,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).expect("plain structs always serialize")
}

pub fn decode(text: &str) -> Result<(TaskList, Revision), StorageError> {
    let file: FileV1 = serde_json::from_str(text)
        .map_err(|e| StorageError(format!("unreadable task file: {e}")))?;
    if file.version != FORMAT_VERSION {
        return Err(StorageError(format!("unsupported task file version {}", file.version)));
    }
    let snapshot = TaskListSnapshot {
        next_id: file.next_id,
        tasks: file
            .tasks
            .into_iter()
            .map(|t| TaskSnapshot {
                id: t.id,
                title: t.title,
                created_at: t.created_at,
                done_at: t.done_at,
            })
            .collect(),
    };
    let list = TaskList::restore(snapshot)
        .map_err(|e| StorageError(format!("invalid task file: {}", e.0)))?;
    Ok((list, Revision(file.revision)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskr_core::tasks::domain::Title;

    #[test]
    fn round_trips() {
        let mut list = TaskList::new();
        let id = list.add(Title::parse("a").unwrap(), 1).unwrap();
        list.complete(id, 2).unwrap();
        assert_eq!(decode(&encode(&list, Revision(4))), Ok((list, Revision(4))));
    }

    #[test]
    fn files_written_before_revisions_read_as_revision_zero() {
        let text = r#"{"version":1,"next_id":2,"tasks":[{"id":1,"title":"a","created_at":0}]}"#;
        assert_eq!(decode(text).unwrap().1, Revision::NEW);
    }

    #[test]
    fn rejects_unknown_versions_and_broken_invariants() {
        assert!(decode(r#"{"version":2,"next_id":1,"tasks":[]}"#).is_err());
        let dup = r#"{"version":1,"next_id":3,"tasks":[
            {"id":1,"title":"a","created_at":0},{"id":2,"title":"A","created_at":0}]}"#;
        assert!(decode(dup).unwrap_err().0.contains("duplicate open title"));
    }
}
