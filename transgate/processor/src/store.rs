//! In-memory sessions (`src/lib/sessionStore.ts`). Sessions do not survive a
//! restart and are not shared between replicas: the service runs as one
//! instance (OPS-D06), while its files persist and are purged as orphans.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub trait Session: Clone + Send + 'static {
    fn created_at(&self) -> SystemTime;
    /// Every file the session may own; missing ones are ignored on cleanup.
    fn files(&self, id: &str, work_dir: &std::path::Path) -> Vec<PathBuf>;
}

pub struct Store<S> {
    sessions: Mutex<HashMap<String, S>>,
}

impl<S: Session> Default for Store<S> {
    fn default() -> Self {
        Store { sessions: Mutex::new(HashMap::new()) }
    }
}

impl<S: Session> Store<S> {
    pub fn get(&self, id: &str) -> Option<S> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    pub fn create(&self, id: String, session: S) {
        self.sessions.lock().unwrap().insert(id, session);
    }

    /// Updates a session that still exists; a removed session is never recreated.
    pub fn update(&self, id: &str, change: impl FnOnce(&mut S)) -> bool {
        match self.sessions.lock().unwrap().get_mut(id) {
            Some(session) => {
                change(session);
                true
            }
            None => false,
        }
    }

    /// Reads and changes a session in one step, e.g. to claim processing.
    pub fn with<R>(&self, id: &str, f: impl FnOnce(Option<&mut S>) -> R) -> R {
        f(self.sessions.lock().unwrap().get_mut(id))
    }

    pub fn delete(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }

    /// Ids of sessions created more than `ttl` before `now`.
    pub fn expired(&self, now: SystemTime, ttl: Duration) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| now.duration_since(s.created_at()).is_ok_and(|age| age > ttl))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

