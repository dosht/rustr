use super::*;

fn renderer() -> Renderer { Renderer::new(BTreeSet::new()).unwrap() }
fn directive<'a>(policy: &'a str, name: &str) -> &'a str {
    policy.split(';').map(str::trim).find(|part| part.split_whitespace().next() == Some(name)).unwrap()
}
fn script_attributes(html: &str) -> Vec<(Option<String>, Option<String>)> {
    let mut scripts = Vec::new();
    rewrite_str(html, RewriteStrSettings {
        element_content_handlers: vec![element!("script", |element| {
            scripts.push((element.get_attribute("nonce"), element.get_attribute("src")));
            Ok(())
        })],
        ..RewriteStrSettings::default()
    }).unwrap();
    scripts
}
fn form(content: &str) -> CheckoutOutcome {
    CheckoutOutcome::Form { payment_id: 42, gateway: "iyzico".into(), content: content.into(), locale: "en".into() }
}

#[test]
fn every_provider_script_uses_the_header_nonce_without_rewriting_script_rawtext_or_loading_the_app() {
    let renderer = renderer();
    let provider = r#"<SCRIPT nonce="old">const marker = "<script nonce='literal'>";</SCRIPT><script SRC="https://provider.example.test/widget.js" nonce="stale"></script>"#;
    let response = renderer.render_nonce(&form(provider), "owned-checkout-nonce");
    let script = directive(&response.content_security_policy, "script-src");
    assert_eq!(script, "script-src 'nonce-owned-checkout-nonce' 'strict-dynamic'");
    assert_eq!(script_attributes(&response.html), vec![
        (Some("owned-checkout-nonce".into()), None),
        (Some("owned-checkout-nonce".into()), Some("https://provider.example.test/widget.js".into())),
    ]);
    assert!(response.html.contains("const marker = \"<script nonce='literal'>\";"), "script strings are not HTML tags");
    assert!(!response.html.contains("tg-config"));
    assert!(!response.html.contains("tg-inject"));
    assert!(!response.html.contains("/assets/"));
    assert_eq!(directive(&response.content_security_policy, "style-src"), "style-src https: 'unsafe-inline'");
    assert!(!response.html.contains("<style nonce="), "a style nonce would disable the provider's inline styles");
}

#[test]
fn refused_checkout_uses_localized_catalog_copy_and_never_the_private_failure_message() {
    let renderer = renderer();
    let refused = CheckoutOutcome::Failed {
        status: 401, code: "identity_unavailable".into(),
        message: "private-access-token </p><script>leak()</script>".into(), locale: Some("tr".into()),
    };
    let signed_out = renderer.render_nonce(&refused, "nonce-a");
    let (_, labels) = renderer.locale(Some("tr"));
    assert!(signed_out.html.contains("<html lang=\"tr\""));
    let mut expected = String::new();
    escaped(&mut expected, &labels.sign_in);
    assert!(signed_out.html.contains(&expected));
    assert!(!signed_out.html.contains("private-access-token"));
    assert!(script_attributes(&signed_out.html).is_empty());
    let failed = CheckoutOutcome::Failed { status: 502, code: "checkout_failed".into(), message: "private-gateway-body".into(), locale: Some("tr".into()) };
    let failed = renderer.render_nonce(&failed, "nonce-b");
    assert!(!failed.html.contains(&expected), "an ordinary refusal is not mislabeled as signed out");
    expected.clear();
    escaped(&mut expected, &labels.failed);
    assert!(failed.html.contains(&expected));
    assert!(!failed.html.contains("private-gateway-body"));
}

#[test]
fn translated_labels_are_text_not_script_or_attribute_markup() {
    let mut renderer = renderer();
    let labels = &mut renderer.labels[0].1;
    labels.title = Cow::Borrowed("</title><script>title()</script>");
    labels.heading = Cow::Borrowed("<script>heading()</script> & \"quoted\"");
    labels.failed = Cow::Borrowed("</p><img src=x onerror=leak()>");
    labels.back = Cow::Borrowed("<b>Back</b>");
    let failed = CheckoutOutcome::Failed { status: 500, code: "internal".into(), message: "not public".into(), locale: None };
    let response = renderer.render_nonce(&failed, "nonce");
    assert!(script_attributes(&response.html).is_empty());
    assert!(!response.html.contains("<img src=x"));
    assert!(response.html.contains("&lt;script&gt;heading()&lt;/script&gt; &amp; &quot;quoted&quot;"));
    assert!(response.html.contains("&lt;b&gt;Back&lt;/b&gt;"));
}

#[test]
fn checkout_policy_is_enforced_https_scheme_trust_with_only_exact_fixture_exceptions() {
    let baseline = renderer().render_nonce(&form("<script>start()</script>"), "nonce");
    for (name, expected) in [
        ("default-src", "default-src 'none'"),
        ("font-src", "font-src https: data:"),
        ("img-src", "img-src https: data:"),
        ("connect-src", "connect-src https:"),
        ("frame-src", "frame-src https:"),
        ("form-action", "form-action https:"),
        ("base-uri", "base-uri 'none'"),
        ("object-src", "object-src 'none'"),
        ("frame-ancestors", "frame-ancestors 'none'"),
    ] { assert_eq!(directive(&baseline.content_security_policy, name), expected); }
    assert!(!baseline.content_security_policy.contains("unsafe-eval"));
    assert!(!baseline.content_security_policy.contains("http:"));
    let fixture = Renderer::new(BTreeSet::from(["http://127.0.0.1:9191".into()])).unwrap();
    let fixture = fixture.render_nonce(&form(""), "nonce");
    for name in ["style-src", "font-src", "img-src", "connect-src", "frame-src", "form-action"] {
        assert!(directive(&fixture.content_security_policy, name).ends_with(" http://127.0.0.1:9191"));
    }
    assert!(!directive(&fixture.content_security_policy, "script-src").contains("http:"));
    assert!(!fixture.content_security_policy.contains("http: "));
    assert!(!fixture.content_security_policy.contains('*'));
}
