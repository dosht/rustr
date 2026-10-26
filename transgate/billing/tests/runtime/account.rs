//! Real JWT, GoTrue fixture, HTTP account routes and owned PostgreSQL data.
//! Included under erasure so its already isolated product fixture is reused.

use super::*;
use transgate_core::account::{Account, AccountServices, VerifiedProfile};
use transgate_core::flags::FlagConfig;
use transgate_http::account::{AccountHttp, AccountIdentity};

struct Profiles(Arc<transgate_billing::session::SessionIdentity>);
impl AccountIdentity for Profiles {
    fn profile<'a>(&'a self, headers: &'a axum::http::HeaderMap) -> BoxFuture<'a, Result<VerifiedProfile, billing_http::AuthError>> {
        Box::pin(async move {
            let profile = self.0.profile(headers).await?;
            Ok(VerifiedProfile { account: profile.account, email: profile.email, full_name: profile.full_name, email_confirmed_at: profile.email_confirmed_at })
        })
    }
    fn receipt_account<'a>(&'a self, headers: &'a axum::http::HeaderMap) -> BoxFuture<'a, Result<Option<AccountId>, billing_http::AuthError>> {
        Box::pin(self.0.receipt_account(headers))
    }
}

pub(super) fn router(app: &App, privacy: Arc<Privacy>, store: Arc<dyn JobStore>, clock: Arc<dyn Clock>) -> Router {
    transgate_http::account::router(Arc::new(AccountHttp {
        account: Arc::new(Account::new(AccountServices { store, clock: clock.clone() })),
        privacy, identity: app.transgate.identity.clone(), profiles: Arc::new(Profiles(app.transgate.session.clone())),
        clock, flags: FlagConfig::default(), ai_chat: false,
    }))
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn unbarred_receipt_read_verifies_credentials_without_contact_enrollment_or_provisioning() {
    let app = app_with_privacy(Arc::new(OffsetClock::default()), Arc::new(LegacyInventory::default()), Arc::new(IdentityDeletion::default())).await;
    let person = user();
    register_auth(&app, &person);
    let lookups = app.auth.0.lock().unwrap().requests.len();
    for credential in [None, Some("not-a-jwt")] {
        let (status, body) = app.call("GET", "/api/account/erasure/status", credential, None).await;
        assert_eq!((status, body["code"].clone()), (401, json!("unauthorized")));
    }
    let (status, body) = app.call("GET", "/api/account/erasure/status", Some(&person.jwt), None).await;
    assert_eq!((status, body["code"].clone()), (404, json!("not_found")));
    assert_eq!(contact_count(&app, &person).await, 0);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.accounts WHERE account_id = $1")
        .bind(person.id).fetch_one(&app.owner).await.unwrap();
    assert_eq!(accounts, 0, "a receipt probe is not account provisioning");
    assert_eq!(app.auth.0.lock().unwrap().requests.len(), lookups, "receipt verification never fetches directory PII");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn session_uses_trusted_confirmation_and_profile_not_creation_or_claimed_metadata_and_fails_on_directory_outage() {
    let app = app_with_privacy(Arc::new(OffsetClock::default()), Arc::new(LegacyInventory::default()), Arc::new(IdentityDeletion::default())).await;
    let person = user();
    let mut profile = auth_user(&person, &person.email, "2020-01-01T00:00:00Z", "2025-01-02T00:00:00Z", true);
    profile["email_confirmed_at"] = json!("2025-01-01T00:00:00Z");
    profile["user_metadata"]["full_name"] = json!("Directory Ada");
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), profile);
    let response = app.http.get(format!("{}/api/session?locale=tr", app.base)).bearer_auth(&person.jwt).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["account"], person.id.to_string());
    assert_eq!(body["full_name"], "Directory Ada");
    assert_eq!(body["display_name"], "Directory Ada");
    assert_eq!(body["joined_on"], 1_735_689_600_000_000_i64);
    assert!(body["capabilities"].as_object().unwrap().values().all(Value::is_boolean), "no private flag rules or allow-lists escape");
    app.auth.0.lock().unwrap().failing.insert(person.id.to_string());
    let (status, body) = app.call("GET", "/api/session", Some(&person.jwt), None).await;
    assert_eq!((status, body["code"].clone()), (503, json!("identity_unavailable")), "no fallback to stale token facts: {body}");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn barred_admin_can_only_read_an_unchanged_own_receipt_without_advancing_erasure_or_regaining_authority() {
    let legacy = Arc::new(LegacyInventory::default());
    legacy.unavailable.store(true, Ordering::SeqCst);
    let identity = Arc::new(IdentityDeletion::default());
    let app = app_with_privacy(Arc::new(OffsetClock::default()), legacy.clone(), identity.clone()).await;
    let id = Uuid::new_v4();
    let admin = User { id, email: ADMIN.into(), jwt: token(&id, ADMIN, 3600) };
    let other = user();
    register_auth(&app, &admin);
    register_auth(&app, &other);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&admin.jwt), None).await.0, 200);
    let (status, initial) = app.call("DELETE", "/api/delete-account", Some(&admin.jwt), None).await;
    assert_eq!((status, initial["status"].clone()), (202, json!("local_data_erased")));
    let listings = legacy.owners.lock().unwrap().len();
    let lookups = app.auth.0.lock().unwrap().requests.len();
    let (status, receipt) = app.call("GET", "/api/account/erasure/status", Some(&admin.jwt), None).await;
    assert_eq!((status, receipt), (200, initial));
    assert_eq!(legacy.owners.lock().unwrap().len(), listings, "GET cannot retry inventory");
    assert!(identity.0.lock().unwrap().is_empty(), "GET cannot advance identity deletion");
    assert_eq!(app.auth.0.lock().unwrap().requests.len(), lookups);
    assert_eq!(contact_count(&app, &admin).await, 0);
    assert_eq!(app.call("GET", "/api/account/erasure/status", Some(&other.jwt), None).await.0, 404);
    for path in ["/api/session", "/api/jobs"] {
        let (status, body) = app.call("GET", path, Some(&admin.jwt), None).await;
        assert_eq!((status, body["code"].clone()), (410, json!("account_deletion_in_progress")), "{path}: {body}");
    }
    assert_eq!(app.call("POST", "/api/billing/admin/customer-invoices", Some(&admin.jwt), Some(json!({"email":other.email}))).await.0, 410);
    let expired = token(&id, ADMIN, -3600);
    assert_eq!(app.call("GET", "/api/account/erasure/status", Some(&expired), None).await.0, 401);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn feedback_is_owned_and_exactly_persisted_while_support_acceptance_is_durable_not_delivery() {
    let app = app_with_privacy(Arc::new(OffsetClock::default()), Arc::new(LegacyInventory::default()), Arc::new(IdentityDeletion::default())).await;
    let (owner, stranger) = (user(), user());
    register_auth(&app, &owner);
    register_auth(&app, &stranger);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&owner.jwt), None).await.0, 200);
    let (status, draft) = app.call("POST", "/api/jobs", Some(&owner.jwt), Some(json!({"name":"meeting.mp3","speakers":false,"duration_minutes":1,"size_bytes":10}))).await;
    assert_eq!(status, 201, "{draft}");
    let job = draft["id"].as_i64().unwrap();
    let path = format!("/api/jobs/{job}/feedback");
    let body = json!({"rating":0,"feedback":"  Original comment.  "});
    let rejected = app.call("POST", &path, Some(&stranger.jwt), Some(body.clone())).await;
    let missing = app.call("POST", "/api/jobs/9223372036854775807/feedback", Some(&stranger.jwt), Some(body.clone())).await;
    assert_eq!((rejected.0, rejected.1["code"].clone()), (missing.0, missing.1["code"].clone()));
    assert_eq!(rejected.0, 404);
    let (status, saved) = app.call("POST", &path, Some(&owner.jwt), Some(body)).await;
    assert_eq!(status, 201, "{saved}");
    let feedback: (Uuid, i16, String) = sqlx::query_as("SELECT account_id,rating,comment FROM transgate_jobs.job_feedback WHERE id = $1")
        .bind(Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap()).fetch_one(&app.owner).await.unwrap();
    assert_eq!(feedback, (owner.id, 0, "  Original comment.  ".into()));
    let (status, accepted) = app.call("POST", "/api/support", Some(&owner.jwt), Some(json!({"message":"  Help with Unicode: ç ☕  ","jobId":job}))).await;
    assert_eq!((status, accepted["status"].clone()), (202, json!("queued")));
    let request = Uuid::parse_str(accepted["request_id"].as_str().unwrap()).unwrap();
    let persisted: (Uuid, Option<i64>, String) = sqlx::query_as("SELECT account_id,job_id,message FROM transgate_jobs.support_requests WHERE id = $1")
        .bind(request).fetch_one(&app.owner).await.unwrap();
    assert_eq!(persisted, (owner.id, Some(job), "  Help with Unicode: ç ☕  ".into()));
    assert!(app.mail.0.lock().unwrap().sent.is_empty(), "202 means queued; no fictional synchronous mail claim");
}
