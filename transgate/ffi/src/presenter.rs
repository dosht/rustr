//! The one object a shell holds: a handle to an `AppDriver` that lives on its own
//! thread.
//!
//! The driver is single threaded (`Rc`, `RefCell`). The handle keeps it on one
//! dedicated thread running a current-thread runtime with a `LocalSet`, and sends
//! it closures over a channel, so the handle itself is `Send + Sync` as UniFFI
//! requires. Views reach the shell's [`ViewListener`] on a second thread, in order,
//! so a listener may call back into `dispatch` without waiting on the thread that
//! is delivering to it.
//!
//! `dispatch` and `current` wait for the driver thread's answer (microseconds: the
//! driver only updates state and starts work) so a shell calls them off its main
//! thread, as it already must for any blocking call.

use std::rc::Rc;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use tokio::sync::mpsc;
use tokio::task::LocalSet;
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, GoTrue, GoTrueConfig, OsRandom, SessionLock, SystemClock};
use transgate_client::driver::AppDriver;
use transgate_client::export::{DocumentAssetSource, DocumentFonts};
use transgate_client::future::BoxFuture;
use transgate_core::presentation::app::{AppConfig, AppIntent, AppVm};
use transgate_export::documents::{DocumentAssets, FallbackFont, FontBytes};

use crate::ports::{NativePlatform, NativePorts, PortStorage, ProcessLock};

#[derive(uniffi::Record, Clone, Debug)]
pub struct PresenterConfig {
    /// The API root, e.g. `https://app.transgate.example/api`.
    pub api_url: String,
    /// The GoTrue root, e.g. `https://app.transgate.example/auth/v1`.
    pub auth_url: String,
    pub auth_key: Option<String>,
    /// The origin auth redirects return to (a custom scheme on mobile).
    pub site_url: String,
    pub captcha: bool,
    pub user_agent: String,
}

#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq)]
pub enum FfiError {
    /// The presenter was disposed, or its thread ended.
    Disposed,
    /// `reason`, not `message`: a Kotlin `Throwable` already owns `message`.
    Start { reason: String },
    Translate { reason: String },
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiError::Disposed => write!(f, "the presenter is disposed"),
            FfiError::Start { reason } | FfiError::Translate { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for FfiError {}

/// Told of every new view, in order, on a thread of its own.
#[uniffi::export(with_foreign)]
pub trait ViewListener: Send + Sync {
    fn view_changed(&self, view: AppVm);
}

type Command = Box<dyn FnOnce(&Rc<AppDriver>) + Send>;

/// Fonts and the invoice logo from the shell's bundled assets (D10).
struct BundledAssets(Arc<dyn NativePorts>);

impl BundledAssets {
    fn get(&self, name: &str) -> Result<Vec<u8>, String> {
        self.0.asset(name.to_string()).ok_or_else(|| format!("the shell has no bundled asset {name}"))
    }
}

impl DocumentAssetSource for BundledAssets {
    fn base(&self) -> BoxFuture<'_, Result<DocumentAssets, String>> {
        Box::pin(async move {
            Ok(DocumentAssets {
                fonts: FontBytes {
                    latin: self.get("NotoSans.ttf")?,
                    latin_italic: self.get("NotoSans-Italic.ttf")?,
                    arabic: self.get("NotoSansArabic.ttf")?,
                    symbols: self.get("NotoSansSymbols2-Regular.ttf")?,
                    arrows: self.get("NotoSansSymbols.ttf")?,
                },
                invoice_logo: self.get("invoice-logo.jpg")?,
            })
        })
    }

    fn fallback(&self, family: FallbackFont) -> BoxFuture<'_, Result<Vec<u8>, String>> {
        Box::pin(async move { self.get(family.filename()) })
    }
}

#[derive(uniffi::Object)]
pub struct TransgatePresenter {
    commands: Mutex<Option<mpsc::UnboundedSender<Command>>>,
}

impl TransgatePresenter {
    fn send(&self, command: Command) -> Result<(), FfiError> {
        self.commands.lock().expect("the command lock is never poisoned").as_ref().ok_or(FfiError::Disposed)?.send(command).map_err(|_| FfiError::Disposed)
    }

    /// Runs `work` on the driver thread and waits for what it returns.
    fn call<T: Send + 'static>(&self, work: impl FnOnce(&Rc<AppDriver>) -> T + Send + 'static) -> Result<T, FfiError> {
        let (reply, answer) = std_mpsc::sync_channel(1);
        self.send(Box::new(move |driver| {
            let _ = reply.send(work(driver));
        }))?;
        answer.recv().map_err(|_| FfiError::Disposed)
    }
}

impl TransgatePresenter {
    pub fn with_session_lock(config: PresenterConfig, ports: Arc<dyn NativePorts>, listener: Arc<dyn ViewListener>,
        lock: Arc<dyn SessionLock>) -> Result<Arc<TransgatePresenter>, FfiError> {
        let (commands, mut inbox) = mpsc::unbounded_channel::<Command>();
        let (views, delivered) = std_mpsc::channel::<AppVm>();
        let (ready, started) = std_mpsc::sync_channel::<Result<(), String>>(1);

        thread::Builder::new()
            .name("transgate-views".into())
            .spawn(move || {
                while let Ok(view) = delivered.recv() {
                    listener.view_changed(view);
                }
            })
            .map_err(|e| FfiError::Start { reason: e.to_string() })?;

        thread::Builder::new()
            .name("transgate-driver".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => return drop(ready.send(Err(error.to_string()))),
                };
                LocalSet::new().block_on(&runtime, async move {
                    let auth_ports = AuthPorts {
                        storage: Arc::new(PortStorage(ports.clone())),
                        random: Arc::new(OsRandom),
                        clock: Arc::new(SystemClock),
                        lock,
                    };
                    let auth = Arc::new(GoTrue::new(GoTrueConfig { url: config.auth_url.trim_end_matches('/').into(), api_key: config.auth_key.filter(|k| !k.is_empty()) }, auth_ports));
                    let api = Arc::new(HttpApi::new(config.api_url, auth.clone()));
                    let documents = DocumentFonts::new(Arc::new(BundledAssets(ports.clone())));
                    let platform = NativePlatform { ports, user_agent: config.user_agent };
                    let driver = AppDriver::new(AppConfig { site_url: config.site_url, captcha: config.captcha }, api, auth, documents, Box::new(platform));
                    driver.set_listener(Some(Rc::new(move |view: &AppVm| {
                        let _ = views.send(view.clone());
                    })));
                    let _ = ready.send(Ok(()));
                    while let Some(command) = inbox.recv().await {
                        command(&driver);
                    }
                    driver.dispose();
                });
            })
            .map_err(|e| FfiError::Start { reason: e.to_string() })?;

        started.recv().map_err(|_| FfiError::Start { reason: "the driver thread ended before it started".into() })?.map_err(|reason| FfiError::Start { reason })?;
        Ok(Arc::new(TransgatePresenter { commands: Mutex::new(Some(commands)) }))
    }
}

#[uniffi::export]
impl TransgatePresenter {
    #[uniffi::constructor]
    pub fn new(
        config: PresenterConfig,
        ports: Arc<dyn NativePorts>,
        listener: Arc<dyn ViewListener>,
    ) -> Result<Arc<TransgatePresenter>, FfiError> {
        Self::with_session_lock(config, ports, listener, Arc::new(ProcessLock::default()))
    }

    /// Applies a user intent and returns the resulting view; its effects start after this
    /// returns and their views reach the listener. Send `Started` first, as the web does.
    pub fn dispatch(&self, intent: AppIntent) -> Result<AppVm, FfiError> {
        self.call(move |driver| driver.dispatch(intent))
    }

    pub fn current(&self) -> Result<AppVm, FfiError> {
        self.call(|driver| driver.current())
    }

    /// Another process wrote or cleared the stored session (the shell's own lifecycle).
    pub fn session_changed_elsewhere(&self) -> Result<(), FfiError> {
        self.call(|driver| driver.session_changed_elsewhere())
    }

    /// Stops every watch, poll and timer, forgets file handles and ends the threads.
    /// Late results are discarded. Idempotent.
    pub fn dispose(&self) {
        let sender = self.commands.lock().expect("the command lock is never poisoned").take();
        if let Some(sender) = sender {
            let _ = sender.send(Box::new(|driver| {
                driver.dispose();
            }));
        }
    }
}

impl Drop for TransgatePresenter {
    fn drop(&mut self) {
        self.dispose();
    }
}
