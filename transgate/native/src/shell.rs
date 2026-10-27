//! A handle to the presenter that terminal code can wait on. The presenter keeps
//! the `AppDriver` on its own current-thread runtime and hands every new view to
//! a listener; a terminal shell mostly needs "wait until the view says X", so the
//! listener only wakes waiters and the waiter reads the current view.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use transgate_core::presentation::app::{AppIntent, AppVm};
use transgate_core::presentation::layout::Viewport;
use transgate_ffi::{FfiError, NativePorts, PresenterConfig, TransgatePresenter, ViewListener};

use crate::ports::local_offsets;

/// Wakes waiters when a view, a saved file or any other watched fact changed.
#[derive(Default)]
pub struct Signal {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl Signal {
    pub fn notify(&self) {
        *self.generation.lock().expect("the signal lock is never poisoned") += 1;
        self.changed.notify_all();
    }

    /// Re-runs `check` after every notification (and at least every 250 ms) until it
    /// answers `Some`, or `timeout` has passed.
    pub fn wait_for<T>(&self, timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            let seen = *self.generation.lock().expect("the signal lock is never poisoned");
            if let Some(found) = check() {
                return Some(found);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let guard = self.generation.lock().expect("the signal lock is never poisoned");
            drop(self.changed.wait_timeout_while(guard, left.min(Duration::from_millis(250)), |g| *g == seen).expect("the signal lock is never poisoned"));
        }
    }
}

struct Wake(Arc<Signal>);

impl ViewListener for Wake {
    fn view_changed(&self, _view: AppVm) {
        self.0.notify();
    }
}

/// Where the host lives: the API root and the GoTrue root, and the origin auth redirects return to.
#[derive(Clone, Debug)]
pub struct HostConfig {
    pub api_url: String,
    pub auth_url: String,
    pub auth_key: Option<String>,
}

impl HostConfig {
    /// `{origin}/api` and `{origin}/auth/v1`, as the single-origin deployment serves them.
    pub fn from_origin(origin: &str, auth_key: Option<String>) -> HostConfig {
        let origin = origin.trim_end_matches('/');
        HostConfig { api_url: format!("{origin}/api"), auth_url: format!("{origin}/auth/v1"), auth_key }
    }

    pub fn presenter(&self, site_url: &str, user_agent: &str) -> PresenterConfig {
        PresenterConfig {
            api_url: self.api_url.clone(),
            auth_url: self.auth_url.clone(),
            auth_key: self.auth_key.clone(),
            site_url: site_url.trim_end_matches('/').to_string(),
            captcha: false,
            user_agent: user_agent.to_string(),
        }
    }
}

pub struct Shell {
    presenter: Arc<TransgatePresenter>,
    signal: Arc<Signal>,
}

impl Shell {
    /// Starts the presenter and sends `Started` at `url` (`/home`, or the loopback
    /// callback target that carries the PKCE `code`). A terminal has no touch input
    /// and a wide screen, so it asks for the desktop layout, where the queue and the
    /// job list are one route.
    pub fn start(config: PresenterConfig, ports: Arc<dyn NativePorts>, signal: Arc<Signal>, url: &str) -> Result<Shell, FfiError> {
        let presenter = TransgatePresenter::new(config, ports, Arc::new(Wake(signal.clone())))?;
        presenter.dispatch(AppIntent::Started { url: url.to_string(), viewport: Viewport { touch: false, width: 1280 }, utc_offset_minutes: (local_offsets().now_minutes) as i16 })?;
        Ok(Shell { presenter, signal })
    }

    pub fn dispatch(&self, intent: AppIntent) -> Result<AppVm, FfiError> {
        self.presenter.dispatch(intent)
    }

    pub fn view(&self) -> Result<AppVm, FfiError> {
        self.presenter.current()
    }

    pub fn signal(&self) -> &Arc<Signal> {
        &self.signal
    }

    /// Waits until `check` accepts the current view, re-checking after every change.
    pub fn wait<T>(&self, timeout: Duration, mut check: impl FnMut(&AppVm) -> Option<T>) -> Option<T> {
        self.signal.wait_for(timeout, || check(&self.presenter.current().ok()?))
    }

    /// The first view that is not the session spinner.
    pub fn settled(&self, timeout: Duration) -> Option<AppVm> {
        self.wait(timeout, |view| (!matches!(view, AppVm::Restoring { .. })).then(|| view.clone()))
    }

    /// Stops every watch, poll and timer and ends the driver thread. Idempotent.
    pub fn dispose(&self) {
        self.presenter.dispose();
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.presenter.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiter_wakes_on_a_notification_and_times_out_without_one() {
        let signal = Arc::new(Signal::default());
        let flag = Arc::new(Mutex::new(false));
        let (s, f) = (signal.clone(), flag.clone());
        let notifier = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            *f.lock().unwrap() = true;
            s.notify();
        });
        let started = Instant::now();
        assert_eq!(signal.wait_for(Duration::from_secs(5), || (*flag.lock().unwrap()).then_some(7)), Some(7));
        assert!(started.elapsed() < Duration::from_secs(2));
        notifier.join().unwrap();
        assert_eq!(signal.wait_for(Duration::from_millis(30), || None::<u8>), None);
    }

    #[test]
    fn origin_derives_the_api_and_auth_roots() {
        let host = HostConfig::from_origin("https://app.example/", Some("k".into()));
        assert_eq!((host.api_url.as_str(), host.auth_url.as_str()), ("https://app.example/api", "https://app.example/auth/v1"));
        assert_eq!(host.presenter("http://127.0.0.1:7/", "ua").site_url, "http://127.0.0.1:7");
    }
}
