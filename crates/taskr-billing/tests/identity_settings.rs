//! taskr's billing identity mapping and configuration rules. No database.

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, header};
use billing_core::types::AccountId;
use billing_http::{AuthError, BillingIdentity};
use taskr_billing::identity::{SUBJECTS, TaskrIdentity};
use taskr_billing::settings_from;
use taskr_core::shared::access::{Email, Identity, IdentityError, Principal, UserId};
use taskr_core::shared::future::BoxFuture;
use uuid::Uuid;

enum Answer {
    User(String, String),
    Down,
}

/// Answers for exactly the tokens it was given; everything else is invalid.
struct Known(HashMap<String, Answer>);

impl Identity for Known {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>> {
        Box::pin(async move {
            match self.0.get(token) {
                Some(Answer::User(user, email)) => Ok(Principal { user_id: UserId(user.clone()), email: Email::parse(email).unwrap() }),
                Some(Answer::Down) => Err(IdentityError::Unavailable("verifier down".into())),
                None => Err(IdentityError::Invalid("unknown token".into())),
            }
        })
    }
}

fn bearer(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    headers
}

fn adapter(real_subject: Uuid) -> TaskrIdentity {
    let user = |sub: &str, email: &str| Answer::User(sub.to_string(), email.to_string());
    TaskrIdentity::new(Arc::new(Known(HashMap::from([
        ("real".to_string(), user(&real_subject.to_string(), "ada@example.test")),
        ("dev-ada".to_string(), user("dev-ada@example.test", "ada@example.test")),
        ("dev-bob".to_string(), user("dev-bob@example.test", "bob@example.test")),
        ("down".to_string(), Answer::Down),
    ]))))
}

#[tokio::test]
async fn a_supabase_uuid_subject_is_the_account_and_a_dev_subject_hashes_deterministically() {
    let subject = Uuid::new_v4();
    let identity = adapter(subject);
    let real = identity.authenticate(&bearer("real")).await.unwrap();
    assert_eq!(real.account, AccountId(subject));
    assert_eq!(real.email, "ada@example.test");
    assert!(!real.is_admin, "taskr has no billing administrators");

    let first = identity.authenticate(&bearer("dev-ada")).await.unwrap();
    let again = identity.authenticate(&bearer("dev-ada")).await.unwrap();
    let bob = identity.authenticate(&bearer("dev-bob")).await.unwrap();
    assert_eq!(first.account, again.account);
    assert_ne!(first.account, bob.account);
    assert_eq!(first.account, AccountId::from_subject("dev-ada@example.test", &SUBJECTS));
}

#[tokio::test]
async fn nothing_is_authenticated_without_a_valid_bearer_and_an_outage_is_not_a_rejection() {
    let identity = adapter(Uuid::new_v4());
    assert!(matches!(identity.authenticate(&HeaderMap::new()).await, Err(AuthError::Unauthenticated(_))));
    assert!(matches!(identity.authenticate(&bearer("nobody")).await, Err(AuthError::Unauthenticated(_))));
    assert!(matches!(identity.authenticate(&bearer("")).await, Err(AuthError::Unauthenticated(_))));
    let mut basic = HeaderMap::new();
    basic.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic dXNlcjpwdw=="));
    assert!(matches!(identity.authenticate(&basic).await, Err(AuthError::Unauthenticated(_))));
    assert!(matches!(identity.authenticate(&bearer("down")).await, Err(AuthError::Unavailable(_))));
}

fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |key| map.get(key).cloned()
}

const COMPLETE: [(&str, &str); 7] = [
    ("TASKR_BILLING_DATABASE_URL", "postgres://billing_runtime@db/taskr"),
    ("TASKR_PUBLIC_URL", "https://taskr.example.test"),
    ("LEMONSQUEEZY_API_KEY", "key"),
    ("LEMONSQUEEZY_STORE_ID", "1"),
    ("LEMONSQUEEZY_VARIANT_ID", "7"),
    ("LEMONSQUEEZY_WEBHOOK_SECRET", "secret"),
    ("LEMONSQUEEZY_ENVIRONMENT", "test"),
];

#[test]
fn without_a_billing_database_taskr_runs_unchanged_and_with_one_the_store_group_is_complete_or_refused() {
    let none = settings_from(&lookup(&[("LEMONSQUEEZY_API_KEY", "key")]));
    assert!(none.unwrap().is_none(), "no database, no billing, no error");

    let settings = settings_from(&lookup(&COMPLETE)).unwrap().unwrap();
    assert_eq!(settings.lemon_base_url, "https://api.lemonsqueezy.com");
    assert_eq!(settings.migration_database_url, None);

    for missing in COMPLETE.iter().skip(1).map(|(k, _)| *k) {
        let partial: Vec<(&str, &str)> = COMPLETE.iter().copied().filter(|(k, _)| *k != missing).collect();
        let error = settings_from(&lookup(&partial)).unwrap_err();
        assert!(error.contains(missing), "{missing} named in {error}");
    }

    let mut wrong_environment: Vec<(&str, &str)> = COMPLETE.iter().copied().filter(|(k, _)| *k != "LEMONSQUEEZY_ENVIRONMENT").collect();
    wrong_environment.push(("LEMONSQUEEZY_ENVIRONMENT", "sandbox"));
    assert!(settings_from(&lookup(&wrong_environment)).unwrap_err().contains("test or live"));
}
