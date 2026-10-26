//! Long-lived driver work (AI watches, polls, delays) as keyed abortable tasks
//! on the page's microtask queue. Starting a task under a key that is already
//! running replaces it; dispose aborts everything. An aborted task drops its
//! future, which closes any HTTP attachment it held.
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;

use futures_util::future::{AbortHandle, Abortable};
use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

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
    pub fn spawn(&self, key: TaskKey, work: impl Future<Output = ()> + 'static) {
        let (handle, registration) = AbortHandle::new_pair();
        if let Some(previous) = self.0.borrow_mut().insert(key, handle) {
            previous.abort();
        }
        spawn_local(async move {
            let _ = Abortable::new(work, registration).await;
        });
    }

    /// Fire-and-forget work that only dispose may stop (requests feeding one result).
    pub fn detached(work: impl Future<Output = ()> + 'static) {
        spawn_local(work);
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

/// `setTimeout` as a future.
pub async fn sleep(ms: u64) {
    let promise = Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms.min(i32::MAX as u64) as i32);
        } else {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}
