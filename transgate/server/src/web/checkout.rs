//! D17/D19: one trusted billing result becomes a separate checkout document.
//! No application config, credentials, telemetry or application runtime is embedded.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Write;
use std::io;

use billing_http::{CheckoutOutcome, CheckoutPage, RenderedCheckout};
use lol_html::{RewriteStrSettings, element, rewrite_str};
use serde::Deserialize;

const DOCUMENT: &str = include_str!("../../../web/checkout/document.html");
const FAILURE: &str = include_str!("../../../web/checkout/failure.html");
const TOKENS: &str = include_str!("../../../web/src/tokens.css");
const BODY: &str = "<!--tg-checkout-body-->";
const CATALOGS: [(&str, &str); 16] = [
    ("en", include_str!("../../../web/locales/en/common.json")),
    ("tr", include_str!("../../../web/locales/tr/common.json")),
    ("pt", include_str!("../../../web/locales/pt/common.json")),
    ("es", include_str!("../../../web/locales/es/common.json")),
    ("de", include_str!("../../../web/locales/de/common.json")),
    ("id", include_str!("../../../web/locales/id/common.json")),
    ("it", include_str!("../../../web/locales/it/common.json")),
    ("fr", include_str!("../../../web/locales/fr/common.json")),
    ("el", include_str!("../../../web/locales/el/common.json")),
    ("ru", include_str!("../../../web/locales/ru/common.json")),
    ("da", include_str!("../../../web/locales/da/common.json")),
    ("sv", include_str!("../../../web/locales/sv/common.json")),
    ("no", include_str!("../../../web/locales/no/common.json")),
    ("fi", include_str!("../../../web/locales/fi/common.json")),
    ("kk", include_str!("../../../web/locales/kk/common.json")),
    ("az", include_str!("../../../web/locales/az/common.json")),
];

#[derive(Deserialize)]
struct Labels<'a> {
    #[serde(borrow, rename = "Checkout")]
    title: Cow<'a, str>,
    #[serde(borrow, rename = "We couldn't start your checkout")]
    heading: Cow<'a, str>,
    #[serde(borrow, rename = "Back to plans")]
    back: Cow<'a, str>,
    #[serde(borrow, rename = "Please sign in and try again.")]
    sign_in: Cow<'a, str>,
    #[serde(borrow, rename = "Something went wrong while preparing your payment. No charge was made — please try again.")]
    failed: Cow<'a, str>,
}

struct Document {
    before_lang: &'static str,
    before_title: &'static str,
    before_css: &'static str,
    before_body: &'static str,
    after_body: &'static str,
}
struct Failure {
    before_heading: &'static str,
    before_message: &'static str,
    before_back: &'static str,
    after_back: &'static str,
}

fn split(source: &'static str, marker: &str) -> io::Result<(&'static str, &'static str)> {
    source.split_once(marker).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("checkout template is missing {marker}")))
}
fn unique(source: &str, markers: &[&str]) -> io::Result<()> {
    if markers.iter().any(|marker| source.matches(marker).count() != 1) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "checkout template markers must each occur once"));
    }
    Ok(())
}

fn escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"), '<' => out.push_str("&lt;"), '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"), '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
}

impl Document {
    fn new() -> io::Result<Self> {
        unique(DOCUMENT, &["{{lang}}", "{{title}}", "{{tokens_css}}", BODY])?;
        let (before_lang, rest) = split(DOCUMENT, "{{lang}}")?;
        let (before_title, rest) = split(rest, "{{title}}")?;
        let (before_css, rest) = split(rest, "{{tokens_css}}")?;
        let (before_body, after_body) = split(rest, BODY)?;
        Ok(Self { before_lang, before_title, before_css, before_body, after_body })
    }
    fn render(&self, locale: &str, title: &str, body: &str) -> String {
        let mut html = String::with_capacity(DOCUMENT.len() + TOKENS.len() + body.len() + title.len());
        html.push_str(self.before_lang);
        escaped(&mut html, locale);
        html.push_str(self.before_title);
        escaped(&mut html, title);
        html.push_str(self.before_css);
        html.push_str(TOKENS);
        html.push_str(self.before_body);
        html.push_str(body);
        html.push_str(self.after_body);
        html
    }
}
impl Failure {
    fn new() -> io::Result<Self> {
        unique(FAILURE, &["{{heading}}", "{{message}}", "{{back_label}}"])?;
        let (before_heading, rest) = split(FAILURE, "{{heading}}")?;
        let (before_message, rest) = split(rest, "{{message}}")?;
        let (before_back, after_back) = split(rest, "{{back_label}}")?;
        Ok(Self { before_heading, before_message, before_back, after_back })
    }
    fn render(&self, labels: &Labels<'_>, sign_in: bool) -> String {
        let message = if sign_in { &labels.sign_in } else { &labels.failed };
        let mut html = String::with_capacity(FAILURE.len() + labels.heading.len() + message.len() + labels.back.len());
        html.push_str(self.before_heading);
        escaped(&mut html, &labels.heading);
        html.push_str(self.before_message);
        escaped(&mut html, message);
        html.push_str(self.before_back);
        escaped(&mut html, &labels.back);
        html.push_str(self.after_back);
        html
    }
}

pub(super) struct Renderer {
    document: Document,
    failure: Failure,
    labels: Vec<(&'static str, Labels<'static>)>,
    fixture_sources: String,
}

impl Renderer {
    pub(super) fn new(fixture_origins: BTreeSet<String>) -> io::Result<Self> {
        let labels = CATALOGS.into_iter().map(|(locale, json)| {
            let labels = serde_json::from_str(json).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("checkout {locale} catalog is unusable: {error}")))?;
            Ok((locale, labels))
        }).collect::<io::Result<Vec<_>>>()?;
        let capacity = fixture_origins.iter().map(|origin| origin.len() + 1).sum();
        let mut fixture_sources = String::with_capacity(capacity);
        for origin in fixture_origins {
            fixture_sources.push(' ');
            fixture_sources.push_str(&origin);
        }
        Ok(Self { document: Document::new()?, failure: Failure::new()?, labels, fixture_sources })
    }

    fn locale(&self, requested: Option<&str>) -> (&'static str, &Labels<'static>) {
        let (locale, labels) = self.labels.iter().find(|(locale, _)| Some(*locale) == requested).unwrap_or(&self.labels[0]);
        (*locale, labels)
    }

    fn policy(&self, nonce: &str) -> String {
        let fixture = &self.fixture_sources;
        format!(
            "default-src 'none'; script-src 'nonce-{nonce}' 'strict-dynamic'; \
             style-src https: 'unsafe-inline'{fixture}; font-src https: data:{fixture}; \
             img-src 'self' https: data:{fixture}; connect-src https:{fixture}; frame-src https:{fixture}; \
             form-action https:{fixture}; base-uri 'none'; object-src 'none'; frame-ancestors 'none'"
        )
    }

    fn render_nonce(&self, outcome: &CheckoutOutcome, nonce: &str) -> RenderedCheckout {
        let requested = match outcome {
            CheckoutOutcome::Form { locale, .. } => Some(locale.as_str()),
            CheckoutOutcome::Failed { locale, .. } => locale.as_deref(),
        };
        let (locale, labels) = self.locale(requested);
        let body = match outcome {
            CheckoutOutcome::Form { content, .. } => {
                let rewritten = rewrite_str(content, RewriteStrSettings {
                    element_content_handlers: vec![element!("script", |element| {
                        element.set_attribute("nonce", nonce)?;
                        Ok(())
                    })],
                    ..RewriteStrSettings::default()
                });
                match rewritten {
                    Ok(html) => html,
                    Err(_) => {
                        // Never execute unrewritten provider markup or echo it in a diagnostic.
                        // The payment initialization is not repeated and its stored facts remain.
                        eprintln!("transgate: checkout provider markup could not be safely rewritten");
                        self.failure.render(labels, false)
                    }
                }
            }
            CheckoutOutcome::Failed { status, .. } => self.failure.render(labels, *status == 401),
        };
        RenderedCheckout { html: self.document.render(locale, &labels.title, &body), content_security_policy: self.policy(nonce) }
    }
}

fn nonce() -> String {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system must provide checkout nonce entropy");
    let mut nonce = String::with_capacity(64);
    for byte in bytes { write!(nonce, "{byte:02x}").expect("writing checkout nonce to a string"); }
    nonce
}

impl CheckoutPage for Renderer {
    fn render(&self, outcome: &CheckoutOutcome) -> RenderedCheckout {
        self.render_nonce(outcome, &nonce())
    }
}

#[cfg(test)]
mod tests;
