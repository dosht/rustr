use std::future::Future;
use std::pin::Pin;

/// The return type of every async port. Boxed so ports stay object safe and
/// the core needs no async runtime or helper crate.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Browser futures (fetch, timers) are not `Send`, and wasm32 is single threaded.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Runs a future to completion on the current thread. For shells whose ports
/// are local and finish immediately (TUI, mobile facade, tests); networked
/// shells use a real runtime.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread::{self, Thread};

    struct Unpark(Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => thread::park(),
        }
    }
}
