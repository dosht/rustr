use std::fmt;

use crate::shared::clock::Millis;

pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_OPEN_TASKS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u32);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleError {
    Empty,
    TooLong { max: usize },
    ControlCharacter,
}

impl Title {
    pub fn parse(raw: &str) -> Result<Title, TitleError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TitleError::Empty);
        }
        if trimmed.chars().count() > MAX_TITLE_CHARS {
            return Err(TitleError::TooLong { max: MAX_TITLE_CHARS });
        }
        if trimmed.chars().any(char::is_control) {
            return Err(TitleError::ControlCharacter);
        }
        Ok(Title(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn same_as(&self, other: &Title) -> bool {
        self.0.to_lowercase() == other.0.to_lowercase()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Open,
    Done { at: Millis },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    title: Title,
    created_at: Millis,
    status: TaskStatus,
}

impl Task {
    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn title(&self) -> &Title {
        &self.title
    }

    pub fn created_at(&self) -> Millis {
        self.created_at
    }

    pub fn status(&self) -> TaskStatus {
        self.status
    }

    pub fn is_open(&self) -> bool {
        self.status == TaskStatus::Open
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskError {
    InvalidTitle(TitleError),
    DuplicateOpenTitle,
    TooManyOpenTasks { max: usize },
    NotFound(TaskId),
    AlreadyDone(TaskId),
    AlreadyOpen(TaskId),
}

impl From<TitleError> for TaskError {
    fn from(e: TitleError) -> Self {
        TaskError::InvalidTitle(e)
    }
}

/// The aggregate root. The whole list is one consistency boundary because the
/// invariants ("no two open tasks share a title", "at most N open tasks") span tasks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskList {
    tasks: Vec<Task>,
    next_id: u32,
}

impl Default for TaskList {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskList {
    pub fn new() -> Self {
        Self { tasks: Vec::new(), next_id: 1 }
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_open()).count()
    }

    pub fn add(&mut self, title: Title, now: Millis) -> Result<TaskId, TaskError> {
        self.ensure_room_for_one_more_open()?;
        self.ensure_no_open_task_titled(&title)?;
        let id = TaskId(self.next_id);
        self.next_id += 1;
        self.tasks.push(Task { id, title, created_at: now, status: TaskStatus::Open });
        Ok(id)
    }

    pub fn complete(&mut self, id: TaskId, now: Millis) -> Result<(), TaskError> {
        let task = self.task_mut(id)?;
        if !task.is_open() {
            return Err(TaskError::AlreadyDone(id));
        }
        task.status = TaskStatus::Done { at: now };
        Ok(())
    }

    pub fn reopen(&mut self, id: TaskId) -> Result<(), TaskError> {
        let task = self.get(id).ok_or(TaskError::NotFound(id))?;
        if task.is_open() {
            return Err(TaskError::AlreadyOpen(id));
        }
        let title = task.title.clone();
        self.ensure_room_for_one_more_open()?;
        self.ensure_no_open_task_titled(&title)?;
        self.task_mut(id)?.status = TaskStatus::Open;
        Ok(())
    }

    pub fn snapshot(&self) -> TaskListSnapshot {
        TaskListSnapshot {
            next_id: self.next_id,
            tasks: self
                .tasks
                .iter()
                .map(|t| TaskSnapshot {
                    id: t.id.0,
                    title: t.title.0.clone(),
                    created_at: t.created_at,
                    done_at: match t.status {
                        TaskStatus::Open => None,
                        TaskStatus::Done { at } => Some(at),
                    },
                })
                .collect(),
        }
    }

    /// Rebuilds the aggregate from stored data, re-checking every invariant so a
    /// hand-edited or corrupted file can never produce an invalid list.
    pub fn restore(snapshot: TaskListSnapshot) -> Result<TaskList, RestoreError> {
        let mut list = TaskList::new();
        let mut max_id = 0;
        for s in snapshot.tasks {
            let id = TaskId(s.id);
            if s.id == 0 || list.get(id).is_some() {
                return Err(RestoreError(format!("duplicate or zero task id {}", s.id)));
            }
            let title = Title::parse(&s.title)
                .map_err(|e| RestoreError(format!("task {}: invalid title ({e:?})", s.id)))?;
            let status = match s.done_at {
                None => {
                    list.ensure_no_open_task_titled(&title).map_err(|_| {
                        RestoreError(format!("task {}: duplicate open title", s.id))
                    })?;
                    TaskStatus::Open
                }
                Some(at) => TaskStatus::Done { at },
            };
            max_id = max_id.max(s.id);
            list.tasks.push(Task { id, title, created_at: s.created_at, status });
        }
        list.next_id = snapshot.next_id.max(max_id + 1);
        Ok(list)
    }

    fn task_mut(&mut self, id: TaskId) -> Result<&mut Task, TaskError> {
        self.tasks.iter_mut().find(|t| t.id == id).ok_or(TaskError::NotFound(id))
    }

    fn ensure_room_for_one_more_open(&self) -> Result<(), TaskError> {
        if self.open_count() >= MAX_OPEN_TASKS {
            return Err(TaskError::TooManyOpenTasks { max: MAX_OPEN_TASKS });
        }
        Ok(())
    }

    fn ensure_no_open_task_titled(&self, title: &Title) -> Result<(), TaskError> {
        if self.tasks.iter().any(|t| t.is_open() && t.title.same_as(title)) {
            return Err(TaskError::DuplicateOpenTitle);
        }
        Ok(())
    }
}

/// Plain data for persistence adapters, so the domain never depends on serde.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TaskListSnapshot {
    pub next_id: u32,
    pub tasks: Vec<TaskSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: u32,
    pub title: String,
    pub created_at: Millis,
    pub done_at: Option<Millis>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    fn title(s: &str) -> Title {
        Title::parse(s).unwrap()
    }

    #[test]
    fn title_is_trimmed() {
        assert_eq!(title("  Buy milk  ").as_str(), "Buy milk");
    }

    #[test]
    fn title_rejects_empty_long_and_control_characters() {
        assert_eq!(Title::parse("   "), Err(TitleError::Empty));
        assert_eq!(
            Title::parse(&"x".repeat(MAX_TITLE_CHARS + 1)),
            Err(TitleError::TooLong { max: MAX_TITLE_CHARS })
        );
        assert_eq!(Title::parse("a\u{7}b"), Err(TitleError::ControlCharacter));
        assert!(Title::parse(&"é".repeat(MAX_TITLE_CHARS)).is_ok());
    }

    #[test]
    fn add_assigns_increasing_ids() {
        let mut list = TaskList::new();
        assert_eq!(list.add(title("a"), 0), Ok(TaskId(1)));
        assert_eq!(list.add(title("b"), 0), Ok(TaskId(2)));
    }

    #[test]
    fn open_titles_are_unique_ignoring_case() {
        let mut list = TaskList::new();
        list.add(title("Buy milk"), 0).unwrap();
        assert_eq!(list.add(title("buy MILK"), 0), Err(TaskError::DuplicateOpenTitle));
    }

    #[test]
    fn a_done_title_can_be_reused() {
        let mut list = TaskList::new();
        let id = list.add(title("Buy milk"), 0).unwrap();
        list.complete(id, 5).unwrap();
        assert!(list.add(title("Buy milk"), 6).is_ok());
    }

    #[test]
    fn open_task_limit_is_enforced() {
        let mut list = TaskList::new();
        for i in 0..MAX_OPEN_TASKS {
            list.add(title(&format!("t{i}")), 0).unwrap();
        }
        assert_eq!(
            list.add(title("one more"), 0),
            Err(TaskError::TooManyOpenTasks { max: MAX_OPEN_TASKS })
        );
    }

    #[test]
    fn complete_twice_is_rejected() {
        let mut list = TaskList::new();
        let id = list.add(title("a"), 0).unwrap();
        list.complete(id, 1).unwrap();
        assert_eq!(list.get(id).unwrap().status(), TaskStatus::Done { at: 1 });
        assert_eq!(list.complete(id, 2), Err(TaskError::AlreadyDone(id)));
        assert_eq!(list.complete(TaskId(99), 2), Err(TaskError::NotFound(TaskId(99))));
    }

    #[test]
    fn reopen_respects_the_unique_open_title_rule() {
        let mut list = TaskList::new();
        let first = list.add(title("Call mom"), 0).unwrap();
        list.complete(first, 1).unwrap();
        list.add(title("call mom"), 2).unwrap();
        assert_eq!(list.reopen(first), Err(TaskError::DuplicateOpenTitle));
    }

    #[test]
    fn reopen_an_open_task_is_rejected() {
        let mut list = TaskList::new();
        let id = list.add(title("a"), 0).unwrap();
        assert_eq!(list.reopen(id), Err(TaskError::AlreadyOpen(id)));
    }

    #[test]
    fn snapshot_round_trips() {
        let mut list = TaskList::new();
        let a = list.add(title("a"), 10).unwrap();
        list.add(title("b"), 20).unwrap();
        list.complete(a, 30).unwrap();
        assert_eq!(TaskList::restore(list.snapshot()), Ok(list));
    }

    #[test]
    fn restore_rejects_broken_invariants() {
        let dup = TaskListSnapshot {
            next_id: 3,
            tasks: vec![
                TaskSnapshot { id: 1, title: "a".into(), created_at: 0, done_at: None },
                TaskSnapshot { id: 2, title: "A".into(), created_at: 0, done_at: None },
            ],
        };
        assert!(TaskList::restore(dup).is_err());
    }

    #[test]
    fn restore_never_reuses_an_id() {
        let snap = TaskListSnapshot {
            next_id: 1,
            tasks: vec![TaskSnapshot { id: 7, title: "a".into(), created_at: 0, done_at: Some(1) }],
        };
        let mut list = TaskList::restore(snap).unwrap();
        assert_eq!(list.add(title("b"), 0), Ok(TaskId(8)));
    }
}
