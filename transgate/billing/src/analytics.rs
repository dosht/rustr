//! Server-side product analytics through PostHog's documented capture endpoint
//! (`POST <host>/i/v0/e/`, JSON with `api_key`, `event`, `distinct_id`,
//! `properties`, `timestamp`). It carries what the source's `server.ts` carried:
//!
//! - the event id is `uuidv5(JSON.stringify([event, kind, id]), namespace)` with the
//!   source's namespace, so a retry, a second process or a restart names the same
//!   event and PostHog counts it once;
//! - geo-IP enrichment is suppressed (`$geoip_disable`);
//! - a payment event carries the transition's commit time as its timestamp.
//!
//! Delivery is best effort from the outbox: a non-2xx answer or a transport failure
//! is an error the relay retries with the same identity. Client-side GA purchase
//! tracking stays with the browser (P5).

use std::time::Duration;

use billing_core::calendar::format_rfc3339;
use billing_core::notify::AnalyticsEvent;
use billing_core::ports::{AnalyticsError, AnalyticsSink};
use billing_core::types::BoxFuture;
use serde_json::{Value, json};
use uuid::Uuid;

/// The source's namespace for analytics event ids.
pub const NAMESPACE: Uuid = Uuid::from_u128(0x4d40_7b2d_2266_45ca_87fc_b13c_44f7_0f26);

pub struct PostHog {
    http: reqwest::Client,
    url: String,
    api_key: String,
}

impl PostHog {
    /// `host` is the PostHog ingestion host (for example `https://eu.i.posthog.com`).
    pub fn new(host: &str, api_key: &str, timeout: Duration) -> Result<PostHog, String> {
        let host = host.trim().trim_end_matches('/');
        if host.is_empty() || api_key.trim().is_empty() {
            return Err("PostHog needs a host and an API key".into());
        }
        let http = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
        Ok(PostHog { http, url: format!("{host}/i/v0/e/"), api_key: api_key.trim().to_string() })
    }
}

/// The event body exactly as it is sent.
pub fn capture_body(api_key: &str, event: &AnalyticsEvent) -> Value {
    let mut properties = event.properties.clone();
    properties.insert("$geoip_disable".into(), json!(true));
    let mut body = json!({
        "api_key": api_key,
        "event": event.name,
        "distinct_id": event.distinct_id,
        "properties": properties,
        "uuid": Uuid::new_v5(&NAMESPACE, event.dedup_name.as_bytes()).to_string(),
    });
    if let Some(at) = event.timestamp {
        body["timestamp"] = json!(format_rfc3339(at));
    }
    body
}

impl AnalyticsSink for PostHog {
    fn send<'a>(&'a self, event: &'a AnalyticsEvent, _dedup_key: &'a str) -> BoxFuture<'a, Result<(), AnalyticsError>> {
        Box::pin(async move {
            let response = self
                .http
                .post(&self.url)
                .header("content-type", "application/json")
                .body(capture_body(&self.api_key, event).to_string())
                .send()
                .await
                // The request URL and body hold the key: never include them in the error.
                .map_err(|e| AnalyticsError(format!("analytics sending failed: {}", e.without_url())))?;
            let status = response.status();
            if status.is_success() {
                Ok(())
            } else {
                Err(AnalyticsError(format!("the analytics service answered HTTP {}", status.as_u16())))
            }
        })
    }
}
