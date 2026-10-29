//! Contract tests for the hand-built host bodies the client reads by key or through
//! a view type of its own (REPAIR-P5-10 sweep). Every body below is produced by the
//! real host function, so drift between host and client fails here.
//!
//! Not covered because the host builds the JSON inline in its route handler with
//! no reusable producer (a copy of the literal would only test itself):
//! `DELETE /api/jobs` -> `{"deleted": n}` and `PATCH /api/jobs/lines` ->
//! `{"updated": n}` (`transgate/http/src/lib.rs`), the billing catalog envelope
//! (`{plans:[{package_id, card}], pay_as_you_go, min_hours, max_hours}`) and the
//! coupon reply (`modules/billing/http/src/routes.rs`). For the catalog only its
//! cards come from real producers (`plan_card`, `payg_card`), asserted below.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::{Json, Router};
use billing_core::decimal::Decimal;
use billing_core::grants::PackageSpec;
use billing_core::presentation::purchase::{CatalogPlan, CatalogView, PaygCardView, PlanCardView};
use billing_core::pricing::{Catalog, PaygEntry, PlanEntry, PlanInterval, payg_card, plan_card};
use serde_json::{Value, json};
use transgate_client::ai::{consent_view, limits_view};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::Micros;
use transgate_core::account::SupportId;
use transgate_core::ai::ports::Period;
use transgate_core::ai::{ConsentView, LimitUsage, LimitsView};
use uuid::Uuid;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);
impl AuthStorage for Memory {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().unwrap().get(key).cloned() }
    fn set(&self, key: &str, value: &str) { self.0.lock().unwrap().insert(key.into(), value.into()); }
    fn remove(&self, key: &str) { self.0.lock().unwrap().remove(key); }
}
struct NoContention;
impl SessionLock for NoContention {
    fn acquire(&self) -> BoxFuture<'_, Result<LockGuard, String>> { Box::pin(async { Ok(LockGuard::new(|| {})) }) }
}
struct Fixed(AtomicI64);
impl Clock for Fixed {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}

const USER: Uuid = Uuid::from_u128(1);
/// Whole milliseconds: the wire is `toISOString()` text.
const SOME_TIME: Micros = Micros(1_800_000_123_000);
const LATER: Micros = Micros(1_800_086_523_000);

async fn serve(routes: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    base
}

fn api(base: &str) -> HttpApi {
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &json!({"access_token": "t", "refresh_token": "r", "expires_at": 4_000_000_000i64,
                                    "user_id": USER, "email": "ada@example.test"}).to_string());
    let ports = AuthPorts { storage, random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    HttpApi::new(base, Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports)))
}

#[test]
fn consent_view_reads_what_consent_response_writes() {
    for view in [
        ConsentView {
            has_consented: true, consent_version: "1".into(), consent_date: Some(SOME_TIME), revoked_date: None,
            privacy_policy_url: "/legal/privacy-policy-en".into(), provider_name: "DeepSeek".into(), can_revoke: true, cleanup_pending: false,
        },
        ConsentView {
            has_consented: false, consent_version: "2".into(), consent_date: Some(SOME_TIME), revoked_date: Some(LATER),
            privacy_policy_url: "/legal/privacy-policy-en".into(), provider_name: "DeepSeek".into(), can_revoke: false, cleanup_pending: true,
        },
    ] {
        assert_eq!(consent_view(&transgate_api::ai::consent_response(&view)).unwrap(), view);
    }
}

#[test]
fn limits_view_reads_what_limits_response_writes() {
    let transcript = LimitUsage { current: 1, max: 2, exceeded: false, reset_at: None, reset_period: None };
    let user = LimitUsage { current: 10, max: 10, exceeded: true, reset_at: Some(LATER), reset_period: Some(Period::Weekly) };
    for view in [
        LimitsView { enabled: true, has_limits: true, questions_per_transcript: Some(transcript.clone()), questions_per_user: Some(user.clone()), tokens_per_transcript: None, tokens_per_user: Some(user.clone()) },
        LimitsView { enabled: false, has_limits: false, ..LimitsView::default() },
    ] {
        assert_eq!(limits_view(&transgate_api::ai::limits_response(&view)).unwrap(), view);
    }
}

#[tokio::test]
async fn request_support_reads_the_hosts_accepted_body() {
    let id = SupportId(Uuid::from_u128(77));
    let routes = Router::new().route("/api/support", post(move || async move { Json(transgate_api::account::support_accepted(id)) }));
    let api = api(&serve(routes).await);
    assert_eq!(api.request_support(USER, "hello", None).await.unwrap(), id.0);
}

#[tokio::test]
async fn catalog_decodes_cards_built_by_the_real_pricing_functions() {
    let d = |text: &str| Decimal::parse(text).unwrap();
    let spec = |name: &str| PackageSpec { name: name.into(), minutes: 1200, count: 1, interval: "month".into(), frequency: 1, expiry: 1 };
    let catalog = Catalog {
        plans: vec![
            PlanEntry { name: "PREMIUM".into(), interval: PlanInterval::Monthly, spec: spec("PREMIUM"), usd_price: d("10.00"), struck_price: d("12.00"), plan_code_try: None, plan_code_usd: None },
            PlanEntry { name: "BUSINESS".into(), interval: PlanInterval::Yearly, spec: spec("BUSINESS"), usd_price: d("120.00"), struck_price: d("240.00"), plan_code_try: None, plan_code_usd: None },
        ],
        payg: Some(PaygEntry {
            stored_name: "Pay-as-you-Go".into(), catalog_label: "PAY AS YOU GO".into(), usd_per_hour: d("1.49"), struck_per_hour: d("2.00"),
            min_hours: 5, max_hours: 9999, minutes_per_hour: 60,
        }),
        locale_discount_locales: Default::default(),
        special_offer_try_per_hour: None,
    };
    let served = catalog.clone();
    let routes = Router::new().route("/api/billing/catalog", get(move || {
        let catalog = served.clone();
        async move {
            Json(json!({
                "plans": catalog.plans.iter().enumerate().map(|(index, p)| json!({"package_id": index, "card": plan_card(p)})).collect::<Vec<Value>>(),
                "pay_as_you_go": payg_card(&catalog),
                "min_hours": catalog.payg.as_ref().map(|p| p.min_hours),
                "max_hours": catalog.payg.as_ref().map(|p| p.max_hours),
            }))
        }
    }));
    let view = api(&serve(routes).await).catalog(USER).await.unwrap();
    assert_eq!(
        view,
        CatalogView {
            plans: vec![
                CatalogPlan { package_id: 0, card: PlanCardView { name: "PREMIUM".into(), interval: PlanInterval::Monthly, usd_per_month: d("10.00"), struck_per_month: d("12.00"), badge: view.plans[0].card.badge.clone() } },
                CatalogPlan { package_id: 1, card: PlanCardView { name: "BUSINESS".into(), interval: PlanInterval::Yearly, usd_per_month: d("10.00"), struck_per_month: d("240.00"), badge: view.plans[1].card.badge.clone() } },
            ],
            pay_as_you_go: Some(PaygCardView { price_text: "$1.49 per 1 hour".into(), struck_per_hour: d("2.00") }),
        }
    );
}
