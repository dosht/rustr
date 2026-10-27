//! Long-lived driver work (AI watches, polls, delays) as keyed abortable tasks.
//! Starting a task under a key that is already running replaces it; dispose aborts
//! everything. An aborted task drops its future, which closes any HTTP attachment
//! it held. Execution itself is the platform's (`Platform::spawn`).
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;

use futures_util::future::{AbortHandle, Abortable};

use super::platform::Platform;

/// What a long-lived task is for; one task per key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TaskKey {
    /// One attachment per AI generation.
    Watch(uuid::Uuid),
    /// A named delay or poll owned by one presentation part (e.g. "drafts", "job").
    Timer(&'static str),
}

#[derive(Default)]
pub struct Tasks(RefCell<HashMap<TaskKey, AbortHandle>>);

impl Tasks {
    /// Runs `work` under `key`, aborting a previous task with the same key.
    pub fn spawn(&self, platform: &dyn Platform, key: TaskKey, work: impl Future<Output = ()> + 'static) {
        let (handle, registration) = AbortHandle::new_pair();
        if let Some(previous) = self.0.borrow_mut().insert(key, handle) {
            previous.abort();
        }
        platform.spawn(Box::pin(async move {
            let _ = Abortable::new(work, registration).await;
        }));
    }

    pub fn abort(&self, key: &TaskKey) {
        if let Some(handle) = self.0.borrow_mut().remove(key) {
            handle.abort();
        }
    }

    pub fn abort_all(&self) {
        for (_, handle) in self.0.borrow_mut().drain() {
            handle.abort();
        }
    }
}
