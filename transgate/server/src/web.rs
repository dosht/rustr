//! Serve-time public configuration, tightly scoped app documents and static assets.
//! `install` must run after merging the API/billing routers; its HTML middleware
//! also protects the single-result billing confirmation response.

mod checkout;

use std::{collections::BTreeSet, io, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use billing_http::{ConfirmOutcome, ConfirmPage};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use tower::ServiceExt;
use tower_http::services::ServeDir;
use url::{Host, Url};

const MARKER: &str = "<!--tg-inject-->";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicConfig {
    api_url: &'static str,
    auth_url: String,
    auth_key: Option<String>,
    site_url: String,
    captcha: Option<Captcha>,
    telemetry: Telemetry,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Captcha { provider: &'static str, site_key: String }
#[derive(Clone, Debug, Serialize)]
struct Telemetry { posthog: Option<Posthog>, sentry: Option<Sentry> }
#[derive(Clone, Debug, Serialize)]
struct Posthog { key: String, host: String }
#[derive(Clone, Debug, Serialize)]
struct Sentry { dsn: String, environment: String }

#[derive(Clone, Debug)]
pub struct WebSettings {
    directory: PathBuf,
    config: PublicConfig,
    csp: HeaderValue,
    site_origin: String,
    checkout_fixture_origins: BTreeSet<String>,
}

fn value(lookup: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    lookup(key).filter(|v| !v.trim().is_empty())
}

fn required(lookup: &dyn Fn(&str) -> Option<String>, key: &str) -> Result<String, String> {
    value(lookup, key).ok_or_else(|| format!("{key} is required when TRANSGATE_WEB_DIR is configured"))
}

fn endpoint(raw: &str, key: &str, sentry: bool) -> Result<Url, String> {
    let bad = || format!("{key} must be an HTTPS URL (HTTP only for loopback), without credentials, query or fragment");
    // Reject whitespace/backslash normalization rather than changing deployment intent.
    if raw.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') { return Err(bad()); }
    let url = Url::parse(raw).map_err(|_| bad())?;
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(host)) => host == "localhost",
        None => false,
    };
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || url.host().is_none() || url.password().is_some() || url.query().is_some() || url.fragment().is_some()
        || (!sentry && !url.username().is_empty())
        || (sentry && (url.username().is_empty() || url.path().trim_matches('/').is_empty()))
    { return Err(bad()); }
    Ok(url)
}

fn pair(lookup: &dyn Fn(&str) -> Option<String>, left: &str, right: &str) -> Result<Option<(String, String)>, String> {
    match (value(lookup, left), value(lookup, right)) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) => Ok(Some((a, b))),
        _ => Err(format!("{left} and {right} must be configured together")),
    }
}

/// Disabled unless the deployment supplies a static directory. No privileged
/// GoTrue/service key is read or used as an anonymous-key fallback.
pub fn settings_from(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Option<WebSettings>, String> {
    let Some(directory) = value(lookup, "TRANSGATE_WEB_DIR") else { return Ok(None); };
    let site_url = required(lookup, "TRANSGATE_WEB_SITE_URL")?;
    let site = endpoint(&site_url, "TRANSGATE_WEB_SITE_URL", false)?;
    if site.path() != "/" { return Err("TRANSGATE_WEB_SITE_URL must be an origin without a path".into()); }
    let site_origin = site.origin().ascii_serialization();
    let mut checkout_fixture_origins = BTreeSet::new();
    if site.scheme() == "http" { checkout_fixture_origins.insert(site_origin.clone()); }
    if let Some(raw) = value(lookup, "IYZIPAY_URI") {
        let provider = endpoint(&raw, "IYZIPAY_URI", false)?;
        if provider.scheme() == "http" { checkout_fixture_origins.insert(provider.origin().ascii_serialization()); }
    }
    let auth_url = required(lookup, "TRANSGATE_WEB_AUTH_URL")?;
    let mut connect = BTreeSet::new();
    let mut scripts = BTreeSet::from(["'self'".to_owned(), "'wasm-unsafe-eval'".to_owned(), "https://www.youtube.com".to_owned()]);
    let mut frames = BTreeSet::from(["blob:".to_owned(), "https://www.youtube.com".to_owned()]);
    if auth_url != "/auth/v1" {
        let auth = endpoint(&auth_url, "TRANSGATE_WEB_AUTH_URL", false)?;
        if auth.origin() != site.origin() { connect.insert(auth.origin().ascii_serialization()); }
    }
    let captcha = value(lookup, "TRANSGATE_WEB_TURNSTILE_SITE_KEY").map(|site_key| Captcha { provider: "turnstile", site_key });
    let posthog = pair(lookup, "TRANSGATE_WEB_POSTHOG_KEY", "TRANSGATE_WEB_POSTHOG_HOST")?
        .map(|(key, host)| {
            let url = endpoint(&host, "TRANSGATE_WEB_POSTHOG_HOST", false)?;
            let origin = url.origin().ascii_serialization();
            connect.insert(origin.clone());
            scripts.insert(origin);
            Ok::<_, String>(Posthog { key, host })
        }).transpose()?;
    let sentry = pair(lookup, "TRANSGATE_WEB_SENTRY_DSN", "TRANSGATE_WEB_SENTRY_ENVIRONMENT")?
        .map(|(dsn, environment)| {
            // A Sentry DSN's username is the public project key, not a password.
            let url = endpoint(&dsn, "TRANSGATE_WEB_SENTRY_DSN", true)?;
            connect.insert(url.origin().ascii_serialization());
            Ok::<_, String>(Sentry { dsn, environment })
        }).transpose()?;
    let mut media = BTreeSet::new();
    for key in ["MEDIA_R2_ENDPOINT", "MEDIA_GCS_ENDPOINT"] {
        if let Some(raw) = value(lookup, key) {
            let origin = endpoint(&raw, key, false)?.origin().ascii_serialization();
            connect.insert(origin.clone());
            media.insert(origin);
        }
    }
    if captcha.is_some() {
        scripts.insert("https://challenges.cloudflare.com".into());
        frames.insert("https://challenges.cloudflare.com".into());
    }
    let join = |sources: BTreeSet<String>| {
        let capacity = sources.iter().map(String::len).sum::<usize>() + sources.len().saturating_sub(1);
        let mut joined = String::with_capacity(capacity);
        for (index, source) in sources.into_iter().enumerate() {
            if index > 0 { joined.push(' '); }
            joined.push_str(&source);
        }
        joined
    };
    let csp = format!(
        "default-src 'self'; script-src {}; style-src 'self'; img-src 'self' data: blob: https://i.ytimg.com; font-src 'self'; media-src 'self' blob: {}; connect-src 'self' {}; frame-src {}; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'",
        join(scripts), join(media), join(connect), join(frames),
    );
    let csp = HeaderValue::from_str(&csp).map_err(|_| "configured CSP origins are not valid header values".to_string())?;
    Ok(Some(WebSettings {
        directory: directory.into(), csp, site_origin, checkout_fixture_origins,
        config: PublicConfig {
            api_url: "/api", auth_url, auth_key: value(lookup, "TRANSGATE_WEB_AUTH_KEY"), site_url,
            captcha, telemetry: Telemetry { posthog, sentry },
        },
    }))
}

struct ScriptFormatter;

impl serde_json::ser::Formatter for ScriptFormatter {
    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            let escaped = match character {
                '<' => "\\u003c", '>' => "\\u003e", '&' => "\\u0026",
                '\u{2028}' => "\\u2028", '\u{2029}' => "\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..index])?;
            writer.write_all(escaped.as_bytes())?;
            start = index + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

fn script_json(value: &impl Serialize) -> String {
    let mut bytes = Vec::with_capacity(128);
    value.serialize(&mut serde_json::Serializer::with_formatter(&mut bytes, ScriptFormatter))
        .expect("public config and confirmation contain only JSON-compatible fields");
    String::from_utf8(bytes).expect("JSON formatter writes UTF-8 and ASCII escapes")
}

pub struct WebHost {
    root: PathBuf,
    before: String,
    after: String,
    config_script: String,
    document: Bytes,
    csp: HeaderValue,
    site_origin: String,
    checkout: Arc<checkout::Renderer>,
}

impl WebHost {
    /// Read and validate the index once, before accepting requests.
    pub async fn load(settings: WebSettings) -> io::Result<Arc<Self>> {
        let root = tokio::fs::canonicalize(&settings.directory).await?;
        let index = tokio::fs::canonicalize(root.join("index.html")).await?;
        if !index.starts_with(&root) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "web index escapes static directory")); }
        let template = tokio::fs::read_to_string(index).await?;
        if template.matches(MARKER).count() != 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "web index must contain exactly one <!--tg-inject--> marker"));
        }
        let (before, after) = template.split_once(MARKER).expect("validated one marker");
        let config_script = format!("<script type=\"application/json\" id=\"tg-config\">{}</script>", script_json(&settings.config));
        let document = Bytes::from(format!("{before}{config_script}{after}"));
        let checkout = Arc::new(checkout::Renderer::new(settings.checkout_fixture_origins)?);
        Ok(Arc::new(Self { root, before: before.into(), after: after.into(), config_script, document, csp: settings.csp, site_origin: settings.site_origin, checkout }))
    }

    pub fn checkout_document(&self) -> billing_http::CheckoutDocument {
        billing_http::CheckoutDocument { page: self.checkout.clone(), site_origin: self.site_origin.clone() }
    }

    fn document(&self, head: bool) -> Response {
        let mut response = Response::new(if head { Body::empty() } else { Body::from(self.document.clone()) });
        response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
        response.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_str(&self.document.len().to_string()).expect("byte count"));
        response
    }
}

impl ConfirmPage for WebHost {
    fn render(&self, outcome: &ConfirmOutcome) -> String {
        format!("{}{}<script type=\"application/json\" id=\"tg-confirm\">{}</script>{}", self.before, self.config_script, script_json(outcome), self.after)
    }
}

fn not_found(path: &str) -> Response {
    if path == "/api" || path.starts_with("/api/") {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"Not found", "code":"not_found"}))).into_response()
    } else { StatusCode::NOT_FOUND.into_response() }
}

async fn fallback(State(web): State<Option<Arc<WebHost>>>, request: Request) -> Response {
    let path = request.uri().path();
    if let Some(web) = web
        && transgate_core::presentation::route::is_app_path(path)
    {
        if request.method() == Method::GET || request.method() == Method::HEAD {
            return web.document(request.method() == Method::HEAD);
        }
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "GET, HEAD")]).into_response();
    }
    not_found(path)
}

async fn app_document(State(web): State<Arc<WebHost>>, request: Request) -> Response {
    web.document(request.method() == Method::HEAD)
}

/// The only app files whose names carry no content hash (copied from
/// `transgate/web/public/_app` unchanged), so they can change in place and must
/// be revalidated. Every other `/_app/*` file is a hashed Vite output (D20).
const UNHASHED_ASSETS: [&str; 2] = ["/_app/theme-init.js", "/_app/iyzico-secure-payment.svg"];

async fn asset(State(web): State<Arc<WebHost>>, request: Request) -> Response {
    let path = request.uri().path();
    let Ok(decoded) = percent_decode_str(path).decode_utf8() else { return StatusCode::NOT_FOUND.into_response(); };
    // ServeDir rejects traversal itself; this additional canonical confinement
    // also prevents a symlink inside a deployed assets tree escaping that tree.
    let relative = decoded.trim_start_matches('/');
    if relative.split('/').any(|part| part == ".." || part.contains('\\') || part.contains('\0')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(allowed) = tokio::fs::canonicalize(web.root.join("_app")).await else { return StatusCode::NOT_FOUND.into_response(); };
    if !allowed.starts_with(&web.root) { return StatusCode::NOT_FOUND.into_response(); }
    let Ok(target) = tokio::fs::canonicalize(web.root.join(relative)).await else { return StatusCode::NOT_FOUND.into_response(); };
    if !target.starts_with(allowed) || !tokio::fs::metadata(&target).await.is_ok_and(|m| m.is_file()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let content_type = match target.extension().and_then(|extension| extension.to_str()) {
        Some("wasm") => Some("application/wasm"),
        Some("ttf") => Some("font/ttf"),
        Some("otf") => Some("font/otf"),
        Some("txt") => Some("text/plain; charset=utf-8"),
        Some("svg") => Some("image/svg+xml"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        _ => None,
    };
    let revalidate = UNHASHED_ASSETS.contains(&path);
    let response = ServeDir::new(&web.root).append_index_html_on_directories(false).oneshot(request).await;
    let mut response = match response { Ok(response) => response.into_response(), Err(never) => match never {} };
    if response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED {
        response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(if revalidate { "no-cache" } else { "public, max-age=31536000, immutable" }));
        if let Some(content_type) = content_type { response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type)); }
    }
    response
}

async fn security(State(web): State<Option<Arc<WebHost>>>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html")) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(web) = web && !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
            headers.insert(header::CONTENT_SECURITY_POLICY, web.csp.clone());
        }
    }
    response
}

/// Merge GET confirmation explicitly: billing already owns POST at that path,
/// which would otherwise return 405 without reaching a router fallback.
pub fn install(mut app: Router, web: Option<Arc<WebHost>>) -> Router {
    if let Some(host) = &web {
        let documents = Router::new()
            .route("/checkout/confirm", get(app_document))
            .route("/_app/{*path}", get(asset))
            .with_state(host.clone());
        app = app.merge(documents);
    }
    let fallback_web = web.clone();
    app.fallback(move |request: Request| {
        let web = fallback_web.clone();
        async move { fallback(State(web), request).await }
    }).layer(middleware::from_fn_with_state(web, security))
}

#[cfg(test)]
mod tests;
