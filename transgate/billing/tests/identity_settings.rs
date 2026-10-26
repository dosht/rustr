//! Transgate's identity verification and configuration rules. Tokens are signed
//! here with a test secret through the JWT library; nothing is accepted
//! because of how it looks.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, header};
use billing_core::types::AccountId;
use billing_http::{AuthError, BillingIdentity};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use transgate_billing::identity::JwtIdentity;
use transgate_billing::settings_from;
use uuid::Uuid;

const SECRET: &str = "test-jwt-secret-not-a-credential";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn token(claims: serde_json::Value, secret: &str, algorithm: Algorithm) -> String {
    encode(&Header::new(algorithm), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap()
}

fn claims(sub: &str, email: Option<&str>, expires_in: i64, audience: &str) -> serde_json::Value {
    let exp = (now() as i64 + expires_in) as u64;
    let mut value = json!({"sub": sub, "aud": audience, "exp": exp});
    if let Some(email) = email {
        value["email"] = json!(email);
    }
    value
}

fn bearer(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {value}")).unwrap());
    headers
}

fn identity(dev: bool) -> JwtIdentity {
    JwtIdentity::new(Some(SECRET), None, dev, BTreeSet::from(["Admin@Example.test".to_string()])).unwrap()
}

#[tokio::test]
async fn a_valid_token_keeps_its_uuid_subject_and_marks_admins_from_the_allow_list_only() {
    let id = identity(false);
    let sub = Uuid::new_v4();
    let t = token(claims(&sub.to_string(), Some("ada@example.test"), 3600, "authenticated"), SECRET, Algorithm::HS256);
    let caller = id.authenticate(&bearer(&t)).await.unwrap();
    assert_eq!(caller.account, AccountId(sub), "a real UUID subject is the account, unchanged");
    assert_eq!(caller.email, "ada@example.test");
    assert!(!caller.is_admin);
    let admin = token(claims(&Uuid::new_v4().to_string(), Some("ADMIN@example.test"), 3600, "authenticated"), SECRET, Algorithm::HS256);
    assert!(id.authenticate(&bearer(&admin)).await.unwrap().is_admin, "case-insensitive allow-list");
    // A claim in the token body cannot grant admin.
    let mut forged = claims(&Uuid::new_v4().to_string(), Some("ada@example.test"), 3600, "authenticated");
    forged["is_admin"] = json!(true);
    forged["role"] = json!("admin");
    let t = token(forged, SECRET, Algorithm::HS256);
    assert!(!id.authenticate(&bearer(&t)).await.unwrap().is_admin);
}

#[tokio::test]
async fn missing_malformed_expired_wrongly_signed_or_wrong_audience_credentials_are_unauthenticated() {
    let id = identity(false);
    let sub = Uuid::new_v4().to_string();
    let rejected = |headers: HeaderMap| {
        let id = &id;
        async move { matches!(id.authenticate(&headers).await, Err(AuthError::Unauthenticated(_))) }
    };
    assert!(rejected(HeaderMap::new()).await, "no header: no anonymous fallback");
    assert!(rejected(bearer("")).await);
    assert!(rejected(bearer("just-a-string-pretending-to-be-a-user")).await, "the bearer string is not the user");
    let mut basic = HeaderMap::new();
    basic.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic dXNlcjpwYXNz"));
    assert!(rejected(basic).await);
    let expired = token(claims(&sub, Some("a@example.test"), -3600, "authenticated"), SECRET, Algorithm::HS256);
    assert!(rejected(bearer(&expired)).await);
    let wrong_key = token(claims(&sub, Some("a@example.test"), 3600, "authenticated"), "another-secret", Algorithm::HS256);
    assert!(rejected(bearer(&wrong_key)).await);
    let wrong_audience = token(claims(&sub, Some("a@example.test"), 3600, "someone-else"), SECRET, Algorithm::HS256);
    assert!(rejected(bearer(&wrong_audience)).await);
    let no_email = token(claims(&sub, None, 3600, "authenticated"), SECRET, Algorithm::HS256);
    assert!(rejected(bearer(&no_email)).await);
    // A different algorithm than a configured key supports.
    let hs512 = token(claims(&sub, Some("a@example.test"), 3600, "authenticated"), SECRET, Algorithm::HS512);
    assert!(rejected(bearer(&hs512)).await);
    // An unsigned token (`alg: none`) is not a token.
    let unsigned = format!(
        "{}.{}.",
        "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0",
        "eyJzdWIiOiJhYWFhYWFhYS1hYWFhLWFhYWEtYWFhYS1hYWFhYWFhYWFhYWEiLCJlbWFpbCI6ImFAYi50ZXN0In0"
    );
    assert!(rejected(bearer(&unsigned)).await);
}

#[tokio::test]
async fn a_development_token_works_only_when_deliberately_enabled_and_maps_deterministically() {
    let off = identity(false);
    assert!(matches!(off.authenticate(&bearer("dev:ada@example.test")).await, Err(AuthError::Unauthenticated(_))));

    let on = identity(true);
    let first = on.authenticate(&bearer("dev:ada@example.test")).await.unwrap();
    let again = on.authenticate(&bearer("dev:ada@example.test")).await.unwrap();
    let other = on.authenticate(&bearer("dev:grace@example.test")).await.unwrap();
    assert_eq!(first.account, again.account, "a retry reaches the same account");
    assert_ne!(first.account, other.account);
    assert_eq!(first.email, "ada@example.test");
    // It never replaces verification of real tokens, and a malformed dev token is not a user.
    assert!(matches!(on.authenticate(&bearer("not-a-jwt")).await, Err(AuthError::Unauthenticated(_))));
    assert!(matches!(on.authenticate(&bearer("dev:no-at-sign")).await, Err(AuthError::Unauthenticated(_))));
    let wrong = token(claims(&Uuid::new_v4().to_string(), Some("a@example.test"), 3600, "authenticated"), "another-secret", Algorithm::HS256);
    assert!(matches!(on.authenticate(&bearer(&wrong)).await, Err(AuthError::Unauthenticated(_))));
    // Admin rights for a dev identity still come from the allow-list.
    assert!(on.authenticate(&bearer("dev:admin@example.test")).await.unwrap().is_admin);
    assert!(!first.is_admin);
}

#[test]
fn an_identity_with_no_verification_and_no_dev_mode_cannot_be_built() {
    assert!(JwtIdentity::new(None, None, false, BTreeSet::new()).is_err());
    assert!(JwtIdentity::new(None, Some("https://auth.example.test/jwks.json".into()), false, BTreeSet::new()).is_ok());
}

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |key| map.get(key).cloned()
}

const BASE: [(&str, &str); 13] = [
    ("TRANSGATE_DATABASE_URL", "postgres://runtime@localhost/db"),
    ("TRANSGATE_PUBLIC_URL", "https://app.example.test"),
    ("SUPABASE_JWT_SECRET", "s"),
    ("LEMONSQUEEZY_API_KEY", "k"),
    ("LEMONSQUEEZY_STORE_ID", "1"),
    ("LEMONSQUEEZY_VARIANT_ID", "2"),
    ("TRANSGATE_MAIL_URL", "https://mail.example.test/send"),
    ("TRANSGATE_MAIL_API_KEY", "m"),
    ("TRANSGATE_MAIL_FROM", "billing@example.test"),
    ("TRANSGATE_SALES_EMAIL", "sales@example.test"),
    ("TRANSGATE_SUPPORT_EMAIL", "support@example.test"),
    ("SUPABASE_URL", "https://project.supabase.example"),
    ("SUPABASE_SERVICE_ROLE_KEY", "service"),
];

fn with<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    BASE.iter().copied().chain(extra.iter().copied()).collect()
}

#[test]
fn without_a_database_billing_is_not_configured_and_nothing_else_is_demanded() {
    let none = env(&[("TRANSGATE_PUBLIC_URL", "https://x")]);
    assert!(settings_from(&none).unwrap().is_none());
}

#[test]
fn a_half_configured_group_is_an_error_that_names_what_is_missing_never_a_fake() {
    let lemon_secret = [("LEMONSQUEEZY_WEBHOOK_SECRET", "w"), ("LEMONSQUEEZY_ENVIRONMENT", "test")];
    let ok = settings_from(&env(&with(&lemon_secret))).unwrap().unwrap();
    assert!(ok.lemon.is_some() && ok.iyzico.is_none());
    assert!(!ok.identity.dev_tokens);

    // Lemon without its signing secret.
    let missing = settings_from(&env(&with(&[("LEMONSQUEEZY_ENVIRONMENT", "test")]))).unwrap_err();
    assert!(missing.contains("LemonSqueezy") && missing.contains("LEMONSQUEEZY_WEBHOOK_SECRET"), "{missing}");
    // Iyzico with one setting only.
    let mut pairs = with(&lemon_secret);
    pairs.push(("IYZIPAY_API_KEY", "k"));
    let partial = settings_from(&env(&pairs)).unwrap_err();
    assert!(partial.contains("Iyzico") && partial.contains("IYZIPAY_MERCHANT_ID") && partial.contains("IYZIPAY_URI"), "{partial}");
    // An unknown environment name.
    let mut pairs = with(&[("LEMONSQUEEZY_WEBHOOK_SECRET", "w"), ("LEMONSQUEEZY_ENVIRONMENT", "staging")]);
    assert!(settings_from(&env(&pairs)).unwrap_err().contains("LEMONSQUEEZY_ENVIRONMENT"));
    pairs.retain(|(k, _)| *k != "LEMONSQUEEZY_ENVIRONMENT");
    // No gateway at all.
    let bare = [("TRANSGATE_DATABASE_URL", "postgres://x"), ("TRANSGATE_PUBLIC_URL", "https://x"), ("SUPABASE_JWT_SECRET", "s")];
    assert!(settings_from(&env(&bare)).unwrap_err().contains("no payment gateway"));
    // No public URL.
    let no_url = [("TRANSGATE_DATABASE_URL", "postgres://x")];
    assert!(settings_from(&env(&no_url)).unwrap_err().contains("TRANSGATE_PUBLIC_URL"));
}

#[test]
fn identity_is_required_and_the_dev_token_needs_an_explicit_one() {
    let no_identity: Vec<(&str, &str)> = BASE.iter().copied().filter(|(k, _)| *k != "SUPABASE_JWT_SECRET").collect();
    let pairs: Vec<(&str, &str)> = no_identity
        .iter()
        .copied()
        .chain([("LEMONSQUEEZY_WEBHOOK_SECRET", "w"), ("LEMONSQUEEZY_ENVIRONMENT", "test")])
        .collect();
    assert!(settings_from(&env(&pairs)).unwrap_err().contains("no identity"));
    let with_dev: Vec<(&str, &str)> = pairs.iter().copied().chain([("TRANSGATE_DEV_AUTH", "1")]).collect();
    assert!(settings_from(&env(&with_dev)).unwrap().unwrap().identity.dev_tokens);
    let wrong_value: Vec<(&str, &str)> = pairs.iter().copied().chain([("TRANSGATE_DEV_AUTH", "true")]).collect();
    assert!(settings_from(&env(&wrong_value)).is_err(), "only the literal 1 enables it");
}

#[test]
fn eligibility_lists_and_the_rate_come_from_configuration_and_default_to_empty_and_44() {
    let defaults = settings_from(&env(&with(&[("LEMONSQUEEZY_WEBHOOK_SECRET", "w"), ("LEMONSQUEEZY_ENVIRONMENT", "test")]))).unwrap().unwrap();
    assert!(defaults.eligibility.special_offer_emails.is_empty() && defaults.eligibility.special_clients.is_empty());
    assert_eq!(defaults.rate.to_string(), "44");
    let sub = Uuid::new_v4();
    let sub_text = sub.to_string();
    let configured = settings_from(&env(&with(&[
        ("LEMONSQUEEZY_WEBHOOK_SECRET", "w"),
        ("LEMONSQUEEZY_ENVIRONMENT", "test"),
        ("USD_TRY_RATE", "44.5"),
        ("TRANSGATE_SPECIAL_OFFER_EMAILS", "One@Example.test, two@example.test"),
        ("TRANSGATE_SPECIAL_CLIENTS", sub_text.as_str()),
        ("TRANSGATE_SPECIAL_CLIENT_OFFER", "CODE:33"),
        ("TRANSGATE_SPECIAL_CLIENT_MESSAGE", "text as supplied"),
        ("TRANSGATE_ADMIN_EMAILS", "Boss@Example.test"),
    ])))
    .unwrap()
    .unwrap();
    assert_eq!(configured.rate.to_string(), "44.5");
    assert_eq!(configured.eligibility.special_offer_emails, BTreeSet::from(["one@example.test".to_string(), "two@example.test".to_string()]));
    assert!(configured.eligibility.special_clients.contains(&AccountId(sub)));
    assert_eq!(
        configured.eligibility.special_client_offer.map(|o| (o.code, o.percent, o.message)),
        Some(("CODE".to_string(), 33, "text as supplied".to_string()))
    );
    assert!(configured.identity.admin_emails.contains("boss@example.test"));
}

fn complete_lemon() -> Vec<(&'static str, &'static str)> {
    with(&[("LEMONSQUEEZY_WEBHOOK_SECRET", "w"), ("LEMONSQUEEZY_ENVIRONMENT", "test")])
}

fn without(pairs: Vec<(&'static str, &'static str)>, drop: &[&str]) -> Vec<(&'static str, &'static str)> {
    pairs.into_iter().filter(|(k, _)| !drop.contains(k)).collect()
}

#[test]
fn mail_delivery_is_required_to_start_and_a_partial_group_names_what_is_missing() {
    let mail = ["TRANSGATE_MAIL_URL", "TRANSGATE_MAIL_API_KEY", "TRANSGATE_MAIL_FROM", "TRANSGATE_SALES_EMAIL", "TRANSGATE_SUPPORT_EMAIL"];
    let none = settings_from(&env(&without(complete_lemon(), &mail))).unwrap_err();
    assert!(none.contains("mail delivery is required") && none.contains("TRANSGATE_SUPPORT_EMAIL"), "{none}");
    let partial = settings_from(&env(&without(complete_lemon(), &["TRANSGATE_SUPPORT_EMAIL"]))).unwrap_err();
    assert!(partial.contains("Mail delivery") && partial.contains("TRANSGATE_SUPPORT_EMAIL"), "{partial}");
    let ok = settings_from(&env(&complete_lemon())).unwrap().unwrap();
    assert_eq!((ok.mail.sales_email.as_str(), ok.mail.support_email.as_str()), ("sales@example.test", "support@example.test"));
}

#[test]
fn a_trusted_signup_source_is_required_the_service_token_route_is_not_one_and_development_tokens_may_go_without() {
    let no_source = without(complete_lemon(), &["SUPABASE_URL", "SUPABASE_SERVICE_ROLE_KEY"]);
    let error = settings_from(&env(&no_source)).unwrap_err();
    assert!(error.contains("no trusted signup source") && error.contains("SUPABASE_SERVICE_ROLE_KEY"), "{error}");
    // A signup token alone is only an extra reporting route, never the producer.
    let mut token_only = no_source.clone();
    token_only.push(("TRANSGATE_SIGNUP_TOKEN", "t"));
    assert!(settings_from(&env(&token_only)).unwrap_err().contains("no trusted signup source"));
    // The auth directory is the source; the token then adds a reporting route.
    let ok = settings_from(&env(&complete_lemon())).unwrap().unwrap();
    let auth = ok.auth_directory.unwrap();
    assert_eq!((auth.url.as_str(), auth.service_key.as_str()), ("https://project.supabase.example", "service"));
    assert!(ok.signup_token.is_none());
    let mut both = complete_lemon();
    both.push(("TRANSGATE_SIGNUP_TOKEN", "t"));
    assert_eq!(settings_from(&env(&both)).unwrap().unwrap().signup_token.as_deref(), Some("t"));
    // Half of it is an error that names the other half.
    let mut half = no_source.clone();
    half.push(("SUPABASE_URL", "https://project.supabase.example"));
    assert!(settings_from(&env(&half)).unwrap_err().contains("SUPABASE_SERVICE_ROLE_KEY"));
    // Deliberate local development does not need it.
    let mut dev = no_source;
    dev.push(("TRANSGATE_DEV_AUTH", "1"));
    assert!(settings_from(&env(&dev)).unwrap().is_some());
}

#[test]
fn analytics_is_optional_but_all_or_none() {
    let off = settings_from(&env(&complete_lemon())).unwrap().unwrap();
    assert!(off.analytics.is_none());
    let mut on = complete_lemon();
    on.extend([("POSTHOG_HOST", "https://eu.i.posthog.test"), ("POSTHOG_API_KEY", "phc_k")]);
    let configured = settings_from(&env(&on)).unwrap().unwrap().analytics.unwrap();
    assert_eq!((configured.host.as_str(), configured.api_key.as_str()), ("https://eu.i.posthog.test", "phc_k"));
    let mut half = complete_lemon();
    half.push(("POSTHOG_API_KEY", "phc_k"));
    assert!(settings_from(&env(&half)).unwrap_err().contains("POSTHOG_HOST"));
}
