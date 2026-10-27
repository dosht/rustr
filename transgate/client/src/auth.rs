//! GoTrue (Supabase Auth) flows shared by the browser (wasm) and native shells.
//!
//! The source browser client (`@supabase/ssr` 0.3.0) runs every redirect flow as
//! PKCE: magic link, Google, signup confirmation and recovery return to
//! `/{locale}/home?code=…`, which is exchanged with a stored S256 verifier. A
//! recovery verifier carries the `/PASSWORD_RECOVERY` suffix so the exchange
//! reports a recovery session (auth-ui then shows the update-password form).
//!
//! Tokens stay inside this module and its storage port; callers see only
//! [`AuthSession`] facts. Platform ports supply persistent storage, secure
//! randomness, the clock and a cross-tab lock around refresh. Refresh tokens are
//! single use, so a refresh happens under the lock after rereading storage, and
//! its result is written only if storage still holds the session it rotated.
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::future::{BoxFuture, MaybeSend, MaybeSync};

/// Session storage key. Also the presence hint P6 reads before verifying `/api/session`.
pub const SESSION_KEY: &str = "transgate.session";
/// PKCE verifier key, kept across the redirect (supabase-js `<key>-code-verifier` convention).
pub const VERIFIER_KEY: &str = "transgate.session.code-verifier";
/// Cross-tab lock name around refresh.
pub const LOCK_NAME: &str = "transgate.session";
const RECOVERY_SUFFIX: &str = "/PASSWORD_RECOVERY";
/// Refresh this long before expiry (supabase-js `EXPIRY_MARGIN_MS`, 90 s).
pub const EXPIRY_MARGIN_SECS: i64 = 90;

/// Persistent string storage: browser localStorage, native secure storage.
pub trait AuthStorage: MaybeSend + MaybeSync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn remove(&self, key: &str);
}

/// Cryptographically secure randomness: `crypto.getRandomValues`, the OS RNG.
pub trait Random: MaybeSend + MaybeSync {
    fn fill(&self, bytes: &mut [u8]);
}

/// Wall clock in Unix seconds, for token expiry.
pub trait Clock: MaybeSend + MaybeSync {
    fn now_secs(&self) -> i64;
}

/// Exclusive section shared by every tab/process of this origin (`navigator.locks`).
pub trait SessionLock: MaybeSend + MaybeSync {
    fn acquire(&self) -> BoxFuture<'_, LockGuard>;
}

#[cfg(not(target_arch = "wasm32"))]
type Release = Box<dyn FnOnce() + Send>;
#[cfg(target_arch = "wasm32")]
type Release = Box<dyn FnOnce()>;

/// Held lock; dropping it releases the lock.
pub struct LockGuard(Option<Release>);

impl LockGuard {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(release)))
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new(release: impl FnOnce() + 'static) -> Self {
        Self(Some(Box::new(release)))
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            release();
        }
    }
}

/// OS randomness for native shells and tests.
#[cfg(not(target_arch = "wasm32"))]
pub struct OsRandom;
#[cfg(not(target_arch = "wasm32"))]
impl Random for OsRandom {
    fn fill(&self, bytes: &mut [u8]) {
        getrandom::fill(bytes).expect("the operating system provides randomness");
    }
}

/// System clock for native shells.
#[cfg(not(target_arch = "wasm32"))]
pub struct SystemClock;
#[cfg(not(target_arch = "wasm32"))]
impl Clock for SystemClock {
    fn now_secs(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

pub struct AuthPorts {
    pub storage: Arc<dyn AuthStorage>,
    pub random: Arc<dyn Random>,
    pub clock: Arc<dyn Clock>,
    pub lock: Arc<dyn SessionLock>,
}

#[derive(Clone, Debug)]
pub struct GoTrueConfig {
    /// The GoTrue root: `https://<project>.supabase.co/auth/v1`, or a bare GoTrue
    /// such as `http://127.0.0.1:9999`. Paths are appended without a prefix.
    pub url: String,
    /// The public anon key when the deployment requires one; never a service key.
    pub api_key: Option<String>,
}

/// The facts a shell may know about the signed-in person. No token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthSession {
    pub user_id: Uuid,
    pub email: String,
    /// Access-token expiry, Unix seconds.
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    SignedIn(AuthSession),
    /// A recovery link was exchanged; the person must now set a new password.
    PasswordRecovery(AuthSession),
    PasswordUpdated(AuthSession),
    /// A magic link, confirmation or recovery email was requested.
    EmailSent,
    SignedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthErrorKind {
    InvalidCredentials,
    EmailNotConfirmed,
    RateLimited,
    WeakPassword,
    UserExists,
    /// Missing verifier, unknown/used/expired flow code or verifier mismatch.
    InvalidOrExpiredCode,
    CaptchaFailed,
    /// No usable session: signed out, or the refresh token was rejected.
    SessionMissing,
    /// The request never got an answer.
    Network,
    /// GoTrue answered 5xx or with an unreadable body.
    Unavailable,
    /// Any other refusal; `message` carries GoTrue's text.
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthError {
    pub kind: AuthErrorKind,
    pub status: Option<u16>,
    /// GoTrue's message, shown as the source auth-ui showed `error.message`.
    pub message: Option<String>,
}

impl AuthError {
    fn new(kind: AuthErrorKind, status: Option<u16>, message: Option<String>) -> Self {
        Self { kind, status, message }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "auth {:?}", self.kind)?;
        if let Some(message) = &self.message {
            write!(f, ": {message}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AuthError {}

/// What an app URL says about an auth redirect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callback {
    None,
    Code(String),
    Error { code: Option<String>, description: Option<String> },
}

const CALLBACK_PARAMS: [&str; 4] = ["code", "error", "error_code", "error_description"];

fn pairs(text: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(text.as_bytes()).into_owned().collect()
}

/// Reads `?code=` or an `error`/`error_description` from the query or fragment.
pub fn parse_callback(url: &str) -> Callback {
    let Ok(url) = Url::parse(url) else { return Callback::None };
    let mut all = pairs(url.query().unwrap_or(""));
    all.extend(pairs(url.fragment().unwrap_or("")));
    let get = |name: &str| all.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    if let Some(code) = get("code").filter(|c| !c.is_empty()) {
        return Callback::Code(code);
    }
    if get("error").is_some() || get("error_code").is_some() {
        return Callback::Error { code: get("error_code").or_else(|| get("error")), description: get("error_description") };
    }
    Callback::None
}

/// The URL without auth callback parameters, so a reload never replays a code.
pub fn strip_callback(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else { return url.to_string() };
    // Rewrites only a part that holds a callback parameter; others stay verbatim.
    let clean = |text: Option<&str>| -> Option<String> {
        let text = text?;
        let all = pairs(text);
        if !all.iter().any(|(k, _)| CALLBACK_PARAMS.contains(&k.as_str())) {
            return Some(text.to_string());
        }
        let kept: Vec<_> = all.into_iter().filter(|(k, _)| !CALLBACK_PARAMS.contains(&k.as_str())).collect();
        (!kept.is_empty()).then(|| url::form_urlencoded::Serializer::new(String::new()).extend_pairs(kept).finish())
    };
    let query = clean(parsed.query());
    let fragment = clean(parsed.fragment());
    parsed.set_query(query.as_deref());
    parsed.set_fragment(fragment.as_deref());
    parsed.to_string()
}

/// RFC 7636 S256 challenge: base64url(SHA-256(verifier)) without padding.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSession {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    user_id: Uuid,
    email: String,
}

impl StoredSession {
    fn facts(&self) -> AuthSession {
        AuthSession { user_id: self.user_id, email: self.email.clone(), expires_at: self.expires_at }
    }
}

#[derive(Deserialize)]
struct GoTrueFailure {
    error_code: Option<String>,
    msg: Option<String>,
    message: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn failure(status: u16, text: &str) -> AuthError {
    let body: Option<GoTrueFailure> = serde_json::from_str(text).ok();
    let code = body.as_ref().and_then(|b| b.error_code.clone().or_else(|| b.error.clone()));
    let message = body.as_ref().and_then(|b| b.msg.clone().or_else(|| b.error_description.clone()).or_else(|| b.message.clone()));
    let kind = match code.as_deref() {
        Some("invalid_credentials") => AuthErrorKind::InvalidCredentials,
        Some("email_not_confirmed") => AuthErrorKind::EmailNotConfirmed,
        Some("over_request_rate_limit" | "over_email_send_rate_limit" | "over_sms_send_rate_limit") => AuthErrorKind::RateLimited,
        Some("weak_password") => AuthErrorKind::WeakPassword,
        Some("user_already_exists" | "email_exists") => AuthErrorKind::UserExists,
        Some("flow_state_not_found" | "flow_state_expired" | "bad_code_verifier" | "otp_expired") => AuthErrorKind::InvalidOrExpiredCode,
        Some("captcha_failed") => AuthErrorKind::CaptchaFailed,
        Some(
            "refresh_token_not_found" | "refresh_token_already_used" | "session_not_found" | "session_expired" | "bad_jwt"
            | "invalid_grant",
        ) => AuthErrorKind::SessionMissing,
        _ if status == 429 => AuthErrorKind::RateLimited,
        _ if status >= 500 => AuthErrorKind::Unavailable,
        _ => AuthErrorKind::Rejected,
    };
    AuthError::new(kind, Some(status), message)
}

fn as_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))
}

fn session_from(value: &Value, now: i64) -> Option<StoredSession> {
    let user = value.get("user")?;
    Some(StoredSession {
        access_token: value.get("access_token")?.as_str()?.to_string(),
        refresh_token: value.get("refresh_token")?.as_str()?.to_string(),
        expires_at: value
            .get("expires_at")
            .and_then(as_i64)
            .or_else(|| value.get("expires_in").and_then(as_i64).map(|secs| now + secs))?,
        user_id: Uuid::parse_str(user.get("id")?.as_str()?).ok()?,
        email: user.get("email").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

fn with_redirect(path: &str, redirect: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new()).append_pair("redirect_to", redirect).finish();
    format!("{path}?{query}")
}

fn with_captcha(body: &mut Value, captcha: Option<&str>) {
    if let (Some(token), Some(map)) = (captcha, body.as_object_mut()) {
        map.insert("gotrue_meta_security".into(), json!({ "captcha_token": token }));
    }
}

fn unexpected() -> AuthError {
    AuthError::new(AuthErrorKind::Unavailable, None, Some("unexpected GoTrue answer".into()))
}

pub struct GoTrue {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
    storage: Arc<dyn AuthStorage>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
    lock: Arc<dyn SessionLock>,
}

impl GoTrue {
    pub fn new(config: GoTrueConfig, ports: AuthPorts) -> Self {
        Self {
            http: crate::http::http_client(),
            base: config.url.trim_end_matches('/').to_string(),
            api_key: config.api_key,
            storage: ports.storage,
            random: ports.random,
            clock: ports.clock,
            lock: ports.lock,
        }
    }

    fn load(&self) -> Option<StoredSession> {
        serde_json::from_str(&self.storage.get(SESSION_KEY)?).ok()
    }

    fn store(&self, session: &StoredSession) {
        self.storage.set(SESSION_KEY, &serde_json::to_string(session).expect("plain struct"));
    }

    fn expiring(&self, session: &StoredSession) -> bool {
        session.expires_at - self.clock.now_secs() <= EXPIRY_MARGIN_SECS
    }

    /// A fresh verifier, persisted for the redirect; returns its S256 challenge.
    fn begin_pkce(&self, recovery: bool) -> String {
        let mut bytes = [0u8; 32];
        self.random.fill(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let stored = if recovery { format!("{verifier}{RECOVERY_SUFFIX}") } else { verifier.clone() };
        self.storage.set(VERIFIER_KEY, &stored);
        challenge(&verifier)
    }

    async fn request(&self, method: Method, path: &str, body: Option<Value>, bearer: Option<&str>) -> Result<Value, AuthError> {
        let mut request = self.http.request(method, format!("{}{path}", self.base));
        if let Some(key) = &self.api_key {
            request = request.header("apikey", key);
        }
        if let Some(token) = bearer.or(self.api_key.as_deref()) {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let network = |e: reqwest::Error| AuthError::new(AuthErrorKind::Network, None, Some(e.to_string()));
        let response = request.send().await.map_err(network)?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(network)?;
        if !(200..300).contains(&status) {
            return Err(failure(status, &text));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|_| unexpected())
    }

    /// The stored session's facts, if any (it may need a refresh before use).
    pub fn current(&self) -> Option<AuthSession> {
        self.load().map(|s| s.facts())
    }

    pub async fn sign_in_password(&self, email: &str, password: &str, captcha: Option<&str>) -> Result<AuthOutcome, AuthError> {
        let mut body = json!({ "email": email, "password": password });
        with_captcha(&mut body, captcha);
        let answer = self.request(Method::POST, "/token?grant_type=password", Some(body), None).await?;
        let session = session_from(&answer, self.clock.now_secs()).ok_or_else(unexpected)?;
        self.store(&session);
        Ok(AuthOutcome::SignedIn(session.facts()))
    }

    /// Signup. With email confirmation on, GoTrue answers with the user only and
    /// the confirmation link later returns a PKCE code.
    pub async fn sign_up(&self, email: &str, password: &str, redirect: &str, captcha: Option<&str>) -> Result<AuthOutcome, AuthError> {
        let challenge = self.begin_pkce(false);
        let mut body = json!({ "email": email, "password": password, "data": {},
                               "code_challenge": challenge, "code_challenge_method": "s256" });
        with_captcha(&mut body, captcha);
        let answer = self.request(Method::POST, &with_redirect("/signup", redirect), Some(body), None).await?;
        match session_from(&answer, self.clock.now_secs()) {
            Some(session) => {
                self.storage.remove(VERIFIER_KEY);
                self.store(&session);
                Ok(AuthOutcome::SignedIn(session.facts()))
            }
            None => Ok(AuthOutcome::EmailSent),
        }
    }

    /// Magic link (`signInWithOtp`, `shouldCreateUser` true as in the source).
    pub async fn magic_link(&self, email: &str, redirect: &str, captcha: Option<&str>) -> Result<AuthOutcome, AuthError> {
        let challenge = self.begin_pkce(false);
        let mut body = json!({ "email": email, "data": {}, "create_user": true,
                               "code_challenge": challenge, "code_challenge_method": "s256" });
        with_captcha(&mut body, captcha);
        self.request(Method::POST, &with_redirect("/otp", redirect), Some(body), None).await?;
        Ok(AuthOutcome::EmailSent)
    }

    /// Password recovery email (`resetPasswordForEmail`).
    pub async fn recover(&self, email: &str, redirect: &str, captcha: Option<&str>) -> Result<AuthOutcome, AuthError> {
        let challenge = self.begin_pkce(true);
        let mut body = json!({ "email": email, "code_challenge": challenge, "code_challenge_method": "s256" });
        with_captcha(&mut body, captcha);
        self.request(Method::POST, &with_redirect("/recover", redirect), Some(body), None).await?;
        Ok(AuthOutcome::EmailSent)
    }

    /// The Google redirect URL; the shell navigates to it.
    pub fn google_authorize_url(&self, redirect: &str) -> String {
        let challenge = self.begin_pkce(false);
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("provider", "google")
            .append_pair("redirect_to", redirect)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "s256")
            .finish();
        format!("{}/authorize?{query}", self.base)
    }

    /// Exchanges a callback `code` with the stored verifier. The verifier is
    /// consumed whatever the answer, as supabase-js does.
    pub async fn exchange_code(&self, code: &str) -> Result<AuthOutcome, AuthError> {
        let Some(stored) = self.storage.get(VERIFIER_KEY) else {
            return Err(AuthError::new(AuthErrorKind::InvalidOrExpiredCode, None, Some("code verifier not found in storage".into())));
        };
        let (verifier, recovery) = match stored.strip_suffix(RECOVERY_SUFFIX) {
            Some(verifier) => (verifier.to_string(), true),
            None => (stored, false),
        };
        let body = json!({ "auth_code": code, "code_verifier": verifier });
        let answer = self.request(Method::POST, "/token?grant_type=pkce", Some(body), None).await;
        self.storage.remove(VERIFIER_KEY);
        let session = session_from(&answer?, self.clock.now_secs()).ok_or_else(unexpected)?;
        self.store(&session);
        Ok(if recovery { AuthOutcome::PasswordRecovery(session.facts()) } else { AuthOutcome::SignedIn(session.facts()) })
    }

    pub async fn update_password(&self, password: &str) -> Result<AuthOutcome, AuthError> {
        let missing = || AuthError::new(AuthErrorKind::SessionMissing, None, None);
        let user = self.current().ok_or_else(missing)?.user_id;
        let token = self.access_token(user).await?.ok_or_else(missing)?;
        self.request(Method::PUT, "/user", Some(json!({ "password": password })), Some(&token)).await?;
        Ok(AuthOutcome::PasswordUpdated(self.current().ok_or_else(missing)?))
    }

    /// Global sign-out. As supabase-js, a 401/403/404 (token already invalid)
    /// still clears local state; any other failure keeps the session.
    pub async fn sign_out(&self) -> Result<AuthOutcome, AuthError> {
        let Some(session) = self.load() else {
            self.storage.remove(VERIFIER_KEY);
            return Ok(AuthOutcome::SignedOut);
        };
        match self.request(Method::POST, "/logout?scope=global", None, Some(&session.access_token)).await {
            Ok(_) => {}
            Err(e) if matches!(e.status, Some(401 | 403 | 404)) => {}
            Err(e) => return Err(e),
        }
        if self.load().is_some_and(|s| s.user_id == session.user_id) {
            self.storage.remove(SESSION_KEY);
        }
        self.storage.remove(VERIFIER_KEY);
        Ok(AuthOutcome::SignedOut)
    }

    /// A usable access token for `user`, refreshing under the cross-tab lock when
    /// it is about to expire. `Ok(None)` means `user` no longer has a session in
    /// this origin: signed out, refresh rejected, or another account signed in.
    pub async fn access_token(&self, user: Uuid) -> Result<Option<String>, AuthError> {
        Ok(self.rotate(user, false).await?.map(|s| s.access_token))
    }

    /// Renews `user`'s session now, whatever its expiry: the host rejected its
    /// access token. A rotation another tab made while this one waited for the
    /// lock is adopted instead of spending the used refresh token again.
    pub async fn renew(&self, user: Uuid) -> Result<Option<AuthSession>, AuthError> {
        Ok(self.rotate(user, true).await?.map(|s| s.facts()))
    }

    async fn rotate(&self, user: Uuid, force: bool) -> Result<Option<StoredSession>, AuthError> {
        let Some(seen) = self.load().filter(|s| s.user_id == user) else { return Ok(None) };
        if !force && !self.expiring(&seen) {
            return Ok(Some(seen));
        }
        let _held = self.lock.acquire().await;
        // Another tab may have rotated the session while this one waited.
        let Some(current) = self.load().filter(|s| s.user_id == user) else { return Ok(None) };
        let rotated_elsewhere = current.refresh_token != seen.refresh_token;
        if (!force || rotated_elsewhere) && !self.expiring(&current) {
            return Ok(Some(current));
        }
        let body = json!({ "refresh_token": current.refresh_token });
        let answer = self.request(Method::POST, "/token?grant_type=refresh_token", Some(body), None).await;
        let still_ours = |s: &StoredSession| s.refresh_token == current.refresh_token;
        match answer {
            Ok(answer) => {
                let rotated = session_from(&answer, self.clock.now_secs()).ok_or_else(unexpected)?;
                if !self.load().as_ref().is_some_and(still_ours) {
                    return Ok(self.load().filter(|s| s.user_id == user));
                }
                self.store(&rotated);
                Ok(Some(rotated))
            }
            Err(e) if e.kind == AuthErrorKind::SessionMissing => {
                if self.load().as_ref().is_some_and(still_ours) {
                    self.storage.remove(SESSION_KEY);
                }
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}
