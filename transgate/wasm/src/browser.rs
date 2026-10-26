//! Browser implementations of the client's ports, and the page mechanics Rust
//! owns directly: storage, randomness, the clock, the cross-tab session lock,
//! calendar offsets, history and location.
use billing_core::presentation::confirm::ConfirmPayload;
use js_sys::{Date, Function, Promise, Reflect};
use transgate_client::auth::{AuthStorage, Clock, LOCK_NAME, LockGuard, Random, SessionLock};
use transgate_client::checkout::{CHECKOUT_PATH, CheckoutForm};
use transgate_client::future::BoxFuture;
use transgate_core::jobs::app::CalendarContext;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::Storage;

fn local_storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// `localStorage`. When storage is unavailable (blocked, private mode) the
/// session simply does not persist; nothing is written elsewhere.
pub struct BrowserStorage;

impl AuthStorage for BrowserStorage {
    fn get(&self, key: &str) -> Option<String> {
        local_storage()?.get_item(key).ok().flatten()
    }
    fn set(&self, key: &str, value: &str) {
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(key, value);
        }
    }
    fn remove(&self, key: &str) {
        if let Some(storage) = local_storage() {
            let _ = storage.remove_item(key);
        }
    }
}

/// `crypto.getRandomValues`, the browser CSPRNG.
pub struct CryptoRandom;

impl Random for CryptoRandom {
    fn fill(&self, bytes: &mut [u8]) {
        let crypto = web_sys::window().and_then(|w| w.crypto().ok()).expect("the page provides crypto");
        crypto.get_random_values_with_u8_array(bytes).expect("crypto.getRandomValues succeeds");
    }
}

pub struct BrowserClock;

impl Clock for BrowserClock {
    fn now_secs(&self) -> i64 {
        (Date::now() / 1000.0) as i64
    }
}

/// `navigator.locks` under [`LOCK_NAME`], shared by every tab of the origin.
/// Read through `Reflect` so no unstable web-sys API is needed. Where the Web
/// Locks API is missing, refresh still never overwrites or clears a rotated
/// session (the client compares before writing), but two tabs may both refresh.
pub struct NavigatorLock;

fn locks() -> Option<JsValue> {
    let navigator = web_sys::window()?.navigator();
    Reflect::get(&navigator, &JsValue::from_str("locks")).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

impl SessionLock for NavigatorLock {
    fn acquire(&self) -> BoxFuture<'_, LockGuard> {
        Box::pin(async move {
            let Some(locks) = locks() else { return LockGuard::new(|| {}) };
            let Some(request) = Reflect::get(&locks, &JsValue::from_str("request")).ok().and_then(|f| f.dyn_into::<Function>().ok()) else {
                return LockGuard::new(|| {});
            };
            // The lock is held until `hold` settles; `granted` settles when the callback runs.
            let mut release = None;
            let hold = Promise::new(&mut |resolve, _| release = Some(resolve));
            let mut grant = None;
            let granted = Promise::new(&mut |resolve, _| grant = Some(resolve));
            let (Some(release), Some(grant)) = (release, grant) else { return LockGuard::new(|| {}) };
            let callback = Closure::once_into_js(move |_lock: JsValue| -> JsValue {
                let _ = grant.call0(&JsValue::NULL);
                hold.into()
            });
            if request.call2(&locks, &JsValue::from_str(LOCK_NAME), &callback).is_err() {
                return LockGuard::new(|| {});
            }
            let _ = JsFuture::from(granted).await;
            LockGuard::new(move || {
                let _ = release.call0(&JsValue::NULL);
            })
        })
    }
}

/// The zone offsets the history window needs (east positive): now, and at the
/// same local time one calendar month earlier, computed with JavaScript's own
/// `setMonth` so month-end overflow and DST match the source exactly.
pub fn calendar() -> CalendarContext {
    let now = Date::new_0();
    let then = Date::new_0();
    if let Ok(set_month) = Reflect::get(&then, &JsValue::from_str("setMonth")).and_then(|f| f.dyn_into::<Function>()) {
        let _ = set_month.call1(&then, &JsValue::from_f64(f64::from(then.get_month()) - 1.0));
    }
    CalendarContext {
        offset_now_minutes: -(now.get_timezone_offset() as i32),
        offset_then_minutes: -(then.get_timezone_offset() as i32),
    }
}

/// The current `location.href`.
pub fn href() -> String {
    web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default()
}

/// Reads only the host-embedded confirmation island; absent or unreadable data
/// is `None`. No auth restore, provider call, or checkout/confirmation request.
pub fn confirmation() -> Option<ConfirmPayload> {
    let element = web_sys::window()?.document()?.get_element_by_id("tg-confirm")?;
    if element.tag_name() != "SCRIPT" || element.get_attribute("type").as_deref() != Some("application/json") {
        return None;
    }
    serde_json::from_str(&element.text_content()?).ok()
}

/// In-app navigation: one history entry, or a replacement (canonical URLs,
/// consumed callback codes).
pub fn navigate(path: &str, replace: bool) {
    let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else { return };
    let _ = if replace {
        history.replace_state_with_url(&JsValue::NULL, "", Some(path))
    } else {
        history.push_state_with_url(&JsValue::NULL, "", Some(path))
    };
}

/// Leaves the app: Google authorization or a locale switch.
pub fn assign(url: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.location().assign(url);
    }
}

/// D17: posts one hidden same-origin form to `/checkout` in this tab; the
/// resulting document belongs to the host. The token is set as an input value
/// only, never in the action URL. `false` means nothing was submitted.
pub fn submit_checkout(form: &CheckoutForm) -> bool {
    let submit = || -> Result<(), JsValue> {
        let window = web_sys::window().ok_or(JsValue::NULL)?;
        let document = window.document().ok_or(JsValue::NULL)?;
        let origin = window.location().origin()?;
        let element: web_sys::HtmlFormElement = document.create_element("form")?.dyn_into()?;
        element.set_method("post");
        element.set_action(&format!("{origin}{CHECKOUT_PATH}"));
        element.set_enctype("application/x-www-form-urlencoded");
        element.set_target("_self");
        element.set_hidden(true);
        for (name, value) in [("order", form.order.as_str()), ("access_token", form.access_token.as_str())] {
            let input: web_sys::HtmlInputElement = document.create_element("input")?.dyn_into()?;
            input.set_type("hidden");
            input.set_name(name);
            input.set_value(value);
            element.append_child(&input)?;
        }
        document.body().ok_or(JsValue::NULL)?.append_child(&element)?;
        element.submit()
    };
    submit().is_ok()
}
