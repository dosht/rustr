use super::*;
use axum::{body::to_bytes, http::Request as HttpRequest, routing::post};
use serde_json::{Value, json};
use std::collections::HashMap;

struct Fixture { directory: PathBuf }
impl Fixture {
    async fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("transgate-web-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(directory.join("_app")).await.unwrap();
        tokio::fs::write(directory.join("index.html"), "<!doctype html><head><!--tg-inject--></head><body>app</body>").await.unwrap();
        tokio::fs::write(directory.join("_app/player-Ab12Cd34.wasm"), b"\0asmfixture").await.unwrap();
        tokio::fs::write(directory.join("_app/theme-init.js"), b"document.documentElement.dataset.theme='light';").await.unwrap();
        Self { directory }
    }
    fn settings(&self, extra: &[(&str, &str)]) -> WebSettings {
        let mut pairs = vec![
            ("TRANSGATE_WEB_DIR", self.directory.to_str().unwrap()),
            ("TRANSGATE_WEB_SITE_URL", "https://app.example.test"),
            ("TRANSGATE_WEB_AUTH_URL", "/auth/v1"),
        ];
        pairs.extend_from_slice(extra);
        settings(&pairs).unwrap().unwrap()
    }
    async fn host(&self) -> Arc<WebHost> { WebHost::load(self.settings(&[])).await.unwrap() }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.directory); }
}
fn settings(pairs: &[(&str, &str)]) -> Result<Option<WebSettings>, String> {
    let map: HashMap<_, _> = pairs.iter().map(|(k, v)| (*k, v.to_string())).collect();
    settings_from(&|key| map.get(key).cloned())
}
async fn send(app: &Router, method: &str, path: &str) -> Response {
    app.clone().oneshot(HttpRequest::builder().method(method).uri(path).body(Body::empty()).unwrap()).await.unwrap()
}
async fn text(response: Response) -> String {
    String::from_utf8(to_bytes(response.into_body(), 1024 * 1024).await.unwrap().to_vec()).unwrap()
}
fn embedded(html: &str, id: &str) -> Value {
    let marker = format!("id=\"{id}\">");
    let content = html.split_once(&marker).unwrap().1.split_once("</script>").unwrap().0;
    serde_json::from_str(content).unwrap()
}

#[test]
fn closing_script_and_unicode_json_round_trip_without_an_html_breakout() {
    let attack = "</script><script>alert('x')</script>&>\u{2028}\u{2029}";
    let value = json!({"message":attack});
    let encoded = script_json(&value);
    assert!(!encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), value);
    assert!(encoded.contains("\\u003c/script\\u003e"));
}

#[tokio::test]
async fn confirm_message_cannot_break_out_of_the_tagged_json_handoff() {
    let fixture = Fixture::new().await;
    let host = fixture.host().await;
    let message = "Provider declined </script><script>bad()</script> & \u{2028}";
    let outcome = ConfirmOutcome::Failed { status: 409, code: "payment_conflict".into(), message: message.into() };
    let html = host.render(&outcome);
    assert_eq!(embedded(&html, "tg-confirm"), json!({"outcome":"failed", "status":409,"code":"payment_conflict","message":message}));
    assert_eq!(html.matches("</script>").count(), 2);
}

#[tokio::test]
async fn only_core_app_routes_receive_documents_and_api_misses_remain_json() {
    let fixture = Fixture::new().await;
    let host = fixture.host().await;
    let post_host = host.clone();
    let billing = Router::new().route("/checkout/confirm", post(move || {
        let host = post_host.clone();
        async move { (StatusCode::BAD_REQUEST, axum::response::Html(host.render(&ConfirmOutcome::Failed { status: 400, code: "declined".into(), message: "Declined".into() }))) }
    }));
    let app = install(billing, Some(host));
    for path in ["/home", "/home/42", "/en/home", "/tr/billing", "/checkout/confirm", "/de/checkout/confirm"] {
        let response = send(&app, "GET", path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(text(response).await.contains("id=\"tg-config\""));
    }
    for path in ["/", "/en", "/tr/", "/pricing", "/dashboard", "/dashboard/reports", "/does-not-exist", "/assets/player-Ab12Cd34.wasm", "/theme-init.js", "/_app/missing.js"] {
        assert_eq!(send(&app, "GET", path).await.status(), StatusCode::NOT_FOUND, "{path}");
    }
    for path in ["/api", "/api/unknown"] {
        let response = send(&app, "GET", path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(serde_json::from_str::<Value>(&text(response).await).unwrap(), json!({"error":"Not found","code":"not_found"}));
    }
    assert_eq!(send(&app, "POST", "/home").await.status(), StatusCode::METHOD_NOT_ALLOWED);
    let head = send(&app, "HEAD", "/home").await;
    assert_eq!(head.status(), StatusCode::OK);
    assert!(text(head).await.is_empty());
    let confirmation = send(&app, "POST", "/checkout/confirm").await;
    assert_eq!(confirmation.status(), StatusCode::BAD_REQUEST);
    assert!(confirmation.headers().contains_key(header::CONTENT_SECURITY_POLICY));
    assert_eq!(embedded(&text(confirmation).await, "tg-confirm")["outcome"], "failed");
    let cold = text(send(&app, "GET", "/checkout/confirm").await).await;
    assert!(!cold.contains("tg-confirm"));
    let off = install(Router::new(), None);
    assert_eq!(send(&off, "GET", "/home").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn app_assets_keep_range_mime_and_immutable_cache_while_theme_is_no_cache() {
    let fixture = Fixture::new().await;
    let app = install(Router::new(), Some(fixture.host().await));
    let response = send(&app, "GET", "/_app/player-Ab12Cd34.wasm").await;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/wasm");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), Bytes::from_static(b"\0asmfixture"));
    let range = app.clone().oneshot(HttpRequest::builder().uri("/_app/player-Ab12Cd34.wasm").header(header::RANGE, "bytes=0-3").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.headers()[header::CONTENT_RANGE], "bytes 0-3/11");
    assert_eq!(to_bytes(range.into_body(), 100).await.unwrap(), Bytes::from_static(b"\0asm"));
    assert_eq!(send(&app, "GET", "/_app/theme-init.js").await.headers()[header::CACHE_CONTROL], "no-cache");
    tokio::fs::write(fixture.directory.join("_app/iyzico-secure-payment.svg"), b"<svg xmlns='http://www.w3.org/2000/svg'/>").await.unwrap();
    let badge = send(&app, "GET", "/_app/iyzico-secure-payment.svg").await;
    assert_eq!(badge.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(badge.headers()[header::CACHE_CONTROL], "no-cache", "an unhashed name can change in place, so it is never immutable");
    assert_eq!(send(&app, "POST", "/_app/player-Ab12Cd34.wasm").await.status(), StatusCode::METHOD_NOT_ALLOWED);
    for (name, mime) in [("font-Ab12Cd34.ttf", "font/ttf"), ("font-Ab12Cd34.otf", "font/otf"), ("font-OFL-Ab12Cd34.txt", "text/plain; charset=utf-8"), ("rating-Ab12Cd34.svg", "image/svg+xml"), ("logo-Ab12Cd34.jpg", "image/jpeg")] {
        tokio::fs::write(fixture.directory.join("_app").join(name), b"asset fixture").await.unwrap();
        let response = send(&app, "GET", &format!("/_app/{name}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], mime);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    }
}

#[tokio::test]
async fn assets_cannot_escape_through_encoded_traversal_or_deployed_symlinks() {
    let fixture = Fixture::new().await;
    let app = install(Router::new(), Some(fixture.host().await));
    for path in ["/_app/../index.html", "/_app/%2e%2e/index.html", "/_app/%2e%2e%2findex.html", "/_app/%5c..%5cindex.html"] {
        assert_eq!(send(&app, "GET", path).await.status(), StatusCode::NOT_FOUND, "{path}");
    }
    #[cfg(unix)] {
        std::os::unix::fs::symlink(fixture.directory.join("index.html"), fixture.directory.join("_app/leak.html")).unwrap();
        assert_eq!(send(&app, "GET", "/_app/leak.html").await.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn html_csp_scopes_endpoint_origins_without_paths_wildcards_or_provider_exceptions() {
    let fixture = Fixture::new().await;
    let settings = fixture.settings(&[
        ("TRANSGATE_WEB_AUTH_URL", "https://auth.example.test/auth/v1"),
        ("TRANSGATE_WEB_TURNSTILE_SITE_KEY", "public-site-key"),
        ("TRANSGATE_WEB_POSTHOG_KEY", "public-ph"),
        ("TRANSGATE_WEB_POSTHOG_HOST", "https://telemetry.example.test/ingest"),
        ("TRANSGATE_WEB_SENTRY_DSN", "https://public@sentry.example.test/42"),
        ("TRANSGATE_WEB_SENTRY_ENVIRONMENT", "fixture"),
        ("MEDIA_R2_ENDPOINT", "http://127.0.0.1:9000/bucket"),
        ("MEDIA_GCS_ENDPOINT", "https://storage.example.test/bucket"),
        ("TRANSGATE_WEB_AUTH_KEY", "</script>&public"),
        ("GOTRUE_SERVICE_ROLE_KEY", "never-publish-this"),
    ]);
    let app = install(Router::new(), Some(WebHost::load(settings).await.unwrap()));
    let response = send(&app, "GET", "/home").await;
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    let directive = |name: &str| csp.split(';').map(str::trim).find(|part| part.starts_with(&format!("{name} "))).unwrap();
    let connect = directive("connect-src");
    for origin in ["https://auth.example.test", "https://telemetry.example.test", "https://sentry.example.test", "http://127.0.0.1:9000", "https://storage.example.test"] { assert!(connect.contains(origin)); }
    let media = directive("media-src");
    assert!(media.contains("http://127.0.0.1:9000"));
    assert!(media.contains("https://storage.example.test"));
    assert!(!media.contains("auth.example.test"));
    let script = directive("script-src");
    assert!(script.contains("https://www.youtube.com"));
    assert!(script.contains("https://challenges.cloudflare.com"));
    assert!(script.contains("https://telemetry.example.test"));
    assert!(!script.contains("storage.example.test"));
    assert!(!script.contains("sentry.example.test"));
    assert!(!script.split_whitespace().any(|source| source == "blob:"), "PDF preview does not authorize Blob scripts");
    let frames = directive("frame-src");
    assert!(frames.split_whitespace().any(|source| source == "blob:"), "owned PDF preview frames are permitted");
    assert!(frames.contains("https://www.youtube.com"));
    assert!(frames.contains("https://challenges.cloudflare.com"));
    assert_eq!(directive("object-src"), "object-src 'none'");
    for forbidden in ["*", "unsafe-inline", "nonce-", "iyzico", "/bucket", "/ingest", "public@"] { assert!(!csp.contains(forbidden), "{forbidden}"); }
    assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
    let html = text(response).await;
    let config = embedded(&html, "tg-config");
    assert_eq!(config["apiUrl"], "/api");
    assert_eq!(config["authKey"], "</script>&public");
    assert!(!html.contains("never-publish-this"));
}

#[tokio::test]
async fn startup_refuses_missing_or_duplicate_injection_markers() {
    let fixture = Fixture::new().await;
    for template in ["<head></head>", "<!--tg-inject--><!--tg-inject-->"] {
        tokio::fs::write(fixture.directory.join("index.html"), template).await.unwrap();
        assert!(WebHost::load(fixture.settings(&[])).await.is_err());
    }
}

#[test]
fn deployment_rejects_credential_query_and_non_loopback_http_injection() {
    let base = [
        ("TRANSGATE_WEB_DIR", "/fixture"),
        ("TRANSGATE_WEB_SITE_URL", "https://app.example.test"),
        ("TRANSGATE_WEB_AUTH_URL", "/auth/v1"),
    ];
    for raw in ["https://user:secret@auth.example.test", "https://auth.example.test?x=1", "https://auth.example.test/#fragment", "http://auth.example.test", "http://localhost.evil.test", "//evil.test/auth/v1", "/auth/v1?redirect=evil", "https://auth.example.test\\@evil.test", "https://auth.example.test\n"] {
        let mut input = base.to_vec();
        input.push(("TRANSGATE_WEB_AUTH_URL", raw));
        assert!(settings(&input).is_err(), "{raw}");
    }
    for (a, b) in [("TRANSGATE_WEB_POSTHOG_KEY", "TRANSGATE_WEB_POSTHOG_HOST"), ("TRANSGATE_WEB_SENTRY_DSN", "TRANSGATE_WEB_SENTRY_ENVIRONMENT")] {
        for key in [a, b] {
            let mut input = base.to_vec(); input.push((key, "configured"));
            assert!(settings(&input).is_err());
        }
    }
    for raw in ["http://127.0.0.1:9999/auth/v1", "http://[::1]:9999/auth/v1", "http://localhost:9999/auth/v1"] {
        let mut input = base.to_vec(); input.push(("TRANSGATE_WEB_AUTH_URL", raw));
        assert!(settings(&input).is_ok(), "{raw}");
    }
    let mut input = base.to_vec(); input.push(("GOTRUE_SERVICE_ROLE_KEY", "private"));
    assert_eq!(settings(&input).unwrap().unwrap().config.auth_key, None);
}

#[tokio::test]
async fn checkout_local_http_exceptions_do_not_include_unrelated_auth_or_storage_origins() {
    let fixture = Fixture::new().await;
    let settings = fixture.settings(&[
        ("TRANSGATE_WEB_SITE_URL", "http://127.0.0.1:8080"),
        ("IYZIPAY_URI", "http://127.0.0.1:9001/provider"),
        ("MEDIA_R2_ENDPOINT", "http://127.0.0.1:9002/bucket"),
        ("TRANSGATE_WEB_AUTH_URL", "http://127.0.0.1:9003/auth/v1"),
    ]);
    let host = WebHost::load(settings).await.unwrap();
    let document = host.checkout_document();
    assert_eq!(document.site_origin, "http://127.0.0.1:8080");
    let rendered = document.page.render(&billing_http::CheckoutOutcome::Failed {
        status: 401, code: "unauthorized".into(), message: "private".into(), locale: None,
    });
    let sources: BTreeSet<_> = rendered.content_security_policy.split(';').map(str::trim)
        .find(|part| part.starts_with("connect-src ")).unwrap().split_whitespace().skip(1).collect();
    assert_eq!(sources, BTreeSet::from(["https:", "http://127.0.0.1:8080", "http://127.0.0.1:9001"]));
    assert!(settings_from(&|key| match key {
        "TRANSGATE_WEB_DIR" => Some("/fixture".into()),
        "TRANSGATE_WEB_SITE_URL" => Some("https://app.example.test".into()),
        "TRANSGATE_WEB_AUTH_URL" => Some("/auth/v1".into()),
        "IYZIPAY_URI" => Some("http://provider.example.test".into()),
        _ => None,
    }).is_err(), "D19 never permits non-loopback plaintext payment destinations");
}
