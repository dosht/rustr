//! In-process live attachments of running text generations (D4).
//!
//! An optimization, not reconnect authority: the worker publishes frames, each
//! subscriber has one bounded `llm_core::AttachmentQueue`, and a subscriber
//! that falls behind is detached (its `next` ends) and falls back to the
//! durable snapshot. Only an attachment present before the first delta (the
//! original requester) receives raw provisional deltas; later attachments get
//! sanitized replacements only. Nothing here authorizes: the caller checks
//! ownership and consent before subscribing.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Poll, Waker};

use llm_core::{ApplicationFrame, AttachmentQueue, StreamIdentity, TextEvent};

use crate::ai::GenerationId;

struct Slot {
    queue: AttachmentQueue,
    raw: bool,
    waker: Option<Waker>,
    closed: bool,
}

struct Channel {
    identity: StreamIdentity,
    started: bool,
    subscribers: Vec<Arc<Mutex<Slot>>>,
}

/// One per process; shared by the worker and the host's attachments.
pub struct LiveHub {
    channels: Mutex<HashMap<GenerationId, Channel>>,
    delta_bytes: usize,
    output_bytes: usize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl LiveHub {
    pub fn new(delta_bytes: usize, output_bytes: usize) -> LiveHub {
        LiveHub { channels: Mutex::new(HashMap::new()), delta_bytes, output_bytes }
    }

    /// The worker opens its generation's channel; dropping the publisher closes it.
    pub fn open(self: &Arc<Self>, generation: GenerationId, identity: StreamIdentity) -> Publisher {
        lock(&self.channels).insert(generation, Channel { identity, started: false, subscribers: Vec::new() });
        Publisher { hub: Arc::clone(self), generation }
    }

    /// Attaches to a generation running in this process; `None` when it is not
    /// (another replica, finished or not started): poll the durable snapshot.
    pub fn subscribe(&self, generation: GenerationId) -> Option<Subscription> {
        let mut channels = lock(&self.channels);
        let channel = channels.get_mut(&generation)?;
        let slot = Arc::new(Mutex::new(Slot {
            queue: AttachmentQueue::new(channel.identity.clone(), self.delta_bytes, self.output_bytes),
            raw: !channel.started,
            waker: None,
            closed: false,
        }));
        channel.subscribers.push(Arc::clone(&slot));
        Some(Subscription { slot })
    }

    fn publish(&self, generation: GenerationId, frame: &ApplicationFrame) {
        let mut channels = lock(&self.channels);
        let Some(channel) = channels.get_mut(&generation) else { return };
        let is_delta = matches!(frame.event, TextEvent::Delta(_));
        let is_checkpoint = matches!(frame.event, TextEvent::Snapshot { .. });
        channel.started |= is_delta;
        let mut wake = Vec::new();
        channel.subscribers.retain(|slot| {
            let mut s = lock(slot);
            if s.closed {
                return false;
            }
            // The original attachment keeps raw deltas until the final replaces
            // them; later attachments only ever receive sanitized replacements.
            if (is_delta && !s.raw) || (is_checkpoint && s.raw) {
                return true;
            }
            if s.queue.push(frame.clone()).is_err() {
                s.closed = true;
            }
            if let Some(w) = s.waker.take() {
                wake.push(w);
            }
            !s.closed
        });
        drop(channels);
        for w in wake {
            w.wake();
        }
    }

    fn close(&self, generation: GenerationId) {
        let Some(channel) = lock(&self.channels).remove(&generation) else { return };
        for slot in channel.subscribers {
            let mut s = lock(&slot);
            s.closed = true;
            if let Some(w) = s.waker.take() {
                drop(s);
                w.wake();
            }
        }
    }
}

/// The worker's side of one channel.
pub struct Publisher {
    hub: Arc<LiveHub>,
    generation: GenerationId,
}

impl Publisher {
    pub fn send(&self, frame: &ApplicationFrame) {
        self.hub.publish(self.generation, frame);
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.hub.close(self.generation);
    }
}

/// One attachment. Dropping it detaches; it never cancels the generation.
pub struct Subscription {
    slot: Arc<Mutex<Slot>>,
}

impl Subscription {
    /// The next frame; `None` once detached, closed or too slow: the caller
    /// then continues from the durable snapshot in replacement mode.
    pub fn recv(&mut self) -> impl Future<Output = Option<ApplicationFrame>> + '_ {
        std::future::poll_fn(move |cx| {
            let mut s = lock(&self.slot);
            if let Some(frame) = s.queue.pop() {
                return Poll::Ready(Some(frame));
            }
            if s.closed || s.queue.is_detached() {
                return Poll::Ready(None);
            }
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        })
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut s = lock(&self.slot);
        s.closed = true;
        s.queue.detach();
    }
}
