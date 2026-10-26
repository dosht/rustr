//! The one presenter per tab: core `AppState` plus the effect driver. State is
//! borrowed only inside `update`/`view`; listeners and platform calls always run
//! after the borrow ends, so a listener may dispatch again.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use js_sys::{Date, Function, Reflect};
use serde::{Deserialize, Serialize};
use transgate_client::api::HttpApi;
use transgate_client::export::DocumentFonts;
use transgate_client::attribution::{self, UTM_STORAGE_KEY};
use transgate_client::auth::{AuthPorts, AuthStorage, GoTrue, GoTrueConfig, SESSION_KEY};
use transgate_client::location::app_location;
use transgate_core::Micros;
use transgate_core::presentation::app::{AppConfig as CoreConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEvent, Epoch};
use transgate_core::presentation::layout::{Viewport, YouTubeLayout};
use uuid::Uuid;
use url::Url;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;

use crate::assets::{AssetUrls, FetchedAssets};
use crate::browser::{self, BrowserClock, BrowserStorage, CryptoRandom, NavigatorLock};
use crate::driver;
use crate::platform::Platform;
use crate::tasks::Tasks;

/// The public subset of the host-injected `tg-config` (`index.ts` `TransgateConfig`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    api_url: String,
    auth_url: String,
    #[serde(default)]
    auth_key: Option<String>,
    site_url: String,
    /// Turnstile is configured (`tg-config.captcha` present): sign-in forms need a token.
    #[serde(default)]
    captcha: bool,
    document_assets: AssetUrls,
}

pub(crate) fn now() -> Micros {
    Micros((Date::now() * 1000.0) as i64)
}

pub(crate) fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    Ok(value.serialize(&serde_wasm_bindgen::Serializer::json_compatible())?)
}

/// `/api` or `/auth/v1` resolve against the page; absolute values stay as given.
fn absolute(page: &str, value: &str) -> String {
    Url::parse(page).and_then(|base| base.join(value)).map(|u| u.to_string()).unwrap_or_else(|_| value.to_string()).trim_end_matches('/').to_string()
}

/// Whether the page is shown now; a page without a document counts as visible.
fn page_visible() -> bool {
    web_sys::window().and_then(|w| w.document()).is_none_or(|d| !d.hidden())
}

/// `ontouchstart in window || navigator.maxTouchPoints > 0`, and `innerWidth`
/// (`useDeviceType.ts`); later changes arrive as `AppIntent::Viewport` from the shell.
fn measured_viewport() -> Viewport {
    let Some(window) = web_sys::window() else { return Viewport::default() };
    let touch = Reflect::has(&window, &JsValue::from_str("ontouchstart")).unwrap_or(false) || window.navigator().max_touch_points() > 0;
    let width = window.inner_width().ok().and_then(|w| w.as_f64()).map(|w| w.max(0.0) as u32).unwrap_or(Viewport::default().width);
    Viewport { touch, width }
}

/// First-touch attribution (`utmCapture.ts`), run once per page load as the source did.
fn capture_attribution() {
    let Some(window) = web_sys::window() else { return };
    let search = window.location().search().unwrap_or_default();
    let referrer = window.document().map(|d| d.referrer()).unwrap_or_default();
    let now_iso: String = Date::new_0().to_iso_string().into();
    let storage = BrowserStorage;
    if let Some(record) = attribution::capture(storage.get(UTM_STORAGE_KEY).as_deref(), &search, &referrer, &now_iso) {
        storage.set(UTM_STORAGE_KEY, &record);
    }
}

type Listener = Closure<dyn FnMut(web_sys::Event)>;

pub(crate) struct Inner {
    state: RefCell<AppState>,
    listener: RefCell<Option<Function>>,
    disposed: Cell<bool>,
    window_listeners: RefCell<Vec<(&'static str, Listener)>>,
    pub(crate) platform: Platform,
    pub(crate) api: Arc<HttpApi>,
    pub(crate) auth: Arc<GoTrue>,
    pub(crate) tasks: Tasks,
    /// One parsed PDF writer per tab; public licensed assets, safe to keep across sessions.
    pub(crate) documents: DocumentFonts,
    /// The GoTrue user of the current session epoch. Requests of another epoch
    /// never borrow it, so a late request cannot run with a newer account's token.
    user: Cell<Option<(u64, Uuid)>>,
    /// Picked files the shell still holds for this presenter.
    pub(crate) handles: RefCell<std::collections::BTreeSet<u64>>,
    /// When the current UPL-008 draft poll began (ms since epoch).
    pub(crate) drafts_poll_started: Cell<f64>,
}

impl Inner {
    pub(crate) fn disposed(&self) -> bool {
        self.disposed.get()
    }

    pub(crate) fn epoch(&self) -> Epoch {
        self.state.borrow().epoch()
    }

    /// The delivery fence: the core epoch (session, consent, erasure) and disposal.
    pub(crate) fn is_current(&self, epoch: Epoch) -> bool {
        !self.disposed.get() && self.state.borrow().is_current(epoch)
    }

    /// Core owns invoice-context and preview-request invalidation; this adds the
    /// same session/disposal fence used by every private document delivery.
    pub(crate) fn is_current_invoice_preview(&self, epoch: Epoch, ticket: u64, payment_id: i64) -> bool {
        if self.disposed.get() {
            return false;
        }
        let state = self.state.borrow();
        state.is_current(epoch) && state.is_current_invoice_preview(ticket, payment_id)
    }

    /// Records that the current session epoch belongs to `user` (after core began it).
    pub(crate) fn bind_user(&self, user: Uuid) {
        self.user.set(Some((self.epoch().session, user)));
    }

    /// The user whose token may serve a request of `epoch`, if that epoch is still current.
    pub(crate) fn user(&self, epoch: Epoch) -> Option<Uuid> {
        match self.user.get() {
            Some((session, user)) if session == epoch.session && self.is_current(epoch) => Some(user),
            _ => None,
        }
    }

    /// Touch device (share sheet instead of a download, `useChatExport.ts`).
    pub(crate) fn touch(&self) -> bool {
        self.state.borrow().layouts().youtube == YouTubeLayout::Touch
    }

    pub(crate) fn locale(&self) -> String {
        match self.state.borrow().view(now()) {
            AppVm::Restoring { locale } | AppVm::Erased { locale, .. } | AppVm::CheckoutConfirm { locale, .. } => locale,
            AppVm::SignedOut(vm) => vm.locale,
            AppVm::SignedIn(vm) => vm.locale,
        }
    }

    fn step(&self, event: AppEvent) -> (Vec<AppEffect>, AppVm) {
        let at = now();
        let effects = self.state.borrow_mut().update(event, at);
        let view = self.state.borrow().view(at);
        (effects, view)
    }

    fn notify(&self, view: &AppVm) {
        let listener = self.listener.borrow().clone();
        if let (Some(listener), Ok(view)) = (listener, to_js(view)) {
            // A throwing listener must not stop the presenter.
            let _ = listener.call1(&JsValue::NULL, &view);
        }
    }

    fn run(self: &Rc<Self>, effects: Vec<AppEffect>) {
        for effect in effects {
            driver::run(self, effect);
        }
    }

    /// Feeds an asynchronous result back into core and publishes the new view.
    pub(crate) fn apply(self: &Rc<Self>, event: AppEvent) {
        if self.disposed.get() {
            return;
        }
        // The account an accepted auth event names owns the session core holds
        // after it; bound before its effects run, so they carry its token.
        let signed_in = match &event {
            AppEvent::Auth {
                epoch,
                event:
                    AuthEvent::Restored { facts: Some(facts) }
                    | AuthEvent::SignedIn { facts }
                    | AuthEvent::PasswordRecovery { facts }
                    | AuthEvent::SessionAdopted { facts },
            } if epoch.session == self.epoch().session => Some(facts.user_id),
            _ => None,
        };
        let (effects, view) = self.step(event);
        if let Some(user) = signed_in {
            self.bind_user(user);
        }
        self.notify(&view);
        self.run(effects);
    }

    fn on_window(self: &Rc<Self>, name: &'static str, handler: impl FnMut(web_sys::Event) + 'static) {
        let Some(window) = web_sys::window() else { return };
        let closure: Listener = Closure::new(handler);
        if window.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref()).is_ok() {
            self.window_listeners.borrow_mut().push((name, closure));
        }
    }
}

#[wasm_bindgen]
pub struct TransgatePresenter {
    inner: Rc<Inner>,
}

#[wasm_bindgen]
impl TransgatePresenter {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue, platform: Platform) -> Result<TransgatePresenter, JsError> {
        let config: Config = serde_wasm_bindgen::from_value(config)?;
        let page = browser::href();
        let ports = AuthPorts {
            storage: Arc::new(BrowserStorage),
            random: Arc::new(CryptoRandom),
            clock: Arc::new(BrowserClock),
            lock: Arc::new(NavigatorLock),
        };
        let auth_key = config.auth_key.filter(|k| !k.is_empty());
        let auth = Arc::new(GoTrue::new(GoTrueConfig { url: absolute(&page, &config.auth_url), api_key: auth_key }, ports));
        let api = Arc::new(HttpApi::new(absolute(&page, &config.api_url), auth.clone()));
        let mut urls = config.document_assets;
        for url in [&mut urls.base.latin, &mut urls.base.latin_italic, &mut urls.base.arabic, &mut urls.base.symbols, &mut urls.base.invoice_logo]
            .into_iter()
            .chain(urls.fallbacks.values_mut())
        {
            *url = absolute(&page, url);
        }
        let documents = DocumentFonts::new(Arc::new(FetchedAssets::new(urls)));
        let inner = Rc::new(Inner {
            state: RefCell::new(AppState::new(CoreConfig { site_url: config.site_url, captcha: config.captcha })),
            listener: RefCell::new(None),
            disposed: Cell::new(false),
            window_listeners: RefCell::new(Vec::new()),
            platform,
            api,
            auth,
            tasks: Tasks::default(),
            documents,
            user: Cell::new(None),
            handles: RefCell::new(Default::default()),
            drafts_poll_started: Cell::new(0.0),
        });
        Ok(TransgatePresenter { inner })
    }

    /// The single view listener the TS facade fans out to its subscribers.
    pub fn listen(&self, listener: Function) {
        *self.inner.listener.borrow_mut() = Some(listener);
    }

    /// Captures attribution, wires back/forward and other-tab session changes,
    /// then starts core with the current location.
    pub fn start(&self) {
        capture_attribution();
        let inner = self.inner.clone();
        self.inner.on_window("popstate", move |_| {
            inner.apply(AppEvent::User(AppIntent::RouteChanged { url: app_location(browser::href()) }));
        });
        let inner = self.inner.clone();
        self.inner.on_window("storage", move |event| {
            let key = event.dyn_ref::<web_sys::StorageEvent>().and_then(|e| e.key());
            // `null` key: storage was cleared in another tab.
            if key.is_none() || key.as_deref() == Some(SESSION_KEY) {
                driver::session_changed_elsewhere(&inner);
            }
        });
        let utc_offset_minutes = -(Date::new_0().get_timezone_offset() as i16);
        let started = AppIntent::Started { url: app_location(browser::href()), viewport: measured_viewport(), utc_offset_minutes };
        self.inner.apply(AppEvent::User(started));
        // `visibilitychange` bubbles from the document to the window (D11: polling
        // runs only while the page is visible); core is told the starting state too.
        let inner = self.inner.clone();
        self.inner.on_window("visibilitychange", move |_| {
            inner.apply(AppEvent::User(AppIntent::VisibilityChanged { visible: page_visible() }));
        });
        self.inner.apply(AppEvent::User(AppIntent::VisibilityChanged { visible: page_visible() }));
    }

    /// Applies a user intent and returns the resulting view. Its effects start
    /// after this returns; their views reach the listener, not this call.
    pub fn dispatch(&self, intent: JsValue) -> Result<JsValue, JsError> {
        let intent: AppIntent = serde_wasm_bindgen::from_value(intent)?;
        if self.inner.disposed() {
            return self.current();
        }
        let (effects, view) = self.inner.step(AppEvent::User(intent));
        let answer = to_js(&view);
        self.inner.run(effects);
        answer
    }

    pub fn current(&self) -> Result<JsValue, JsError> {
        to_js(&self.inner.state.borrow().view(now()))
    }

    /// Stops every watch, poll and timer, forgets file handles, removes window
    /// listeners and drops the view listener. Late results are discarded.
    pub fn dispose(&self) {
        if self.inner.disposed.replace(true) {
            return;
        }
        self.inner.listener.borrow_mut().take();
        if let Some(window) = web_sys::window() {
            for (name, closure) in self.inner.window_listeners.borrow_mut().drain(..) {
                let _ = window.remove_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
            }
        }
        driver::dispose(&self.inner);
    }
}
