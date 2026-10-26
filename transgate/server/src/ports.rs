//! The ports core declares and the host implements with real adapters: the GoTrue admin deletion
//! behind account erasure, the sleeper between Deepgram submissions and the download analytics
//! event. Nothing here decides anything; each forwards to a real service.

use std::sync::Arc;
use std::time::Duration;

use billing_core::notify::AnalyticsEvent;
use billing_core::ports::AnalyticsSink;
use billing_core::types::{BoxFuture, Clock};
use serde_json::{Map, json};
use transgate_billing::directory::{AuthDeletion, SupabaseAdmin};
use transgate_core::AccountId;
use transgate_core::export::DownloadEvent;
use transgate_core::jobs::ports::{IdentityAdmin, IdentityOutcome, Sleeper};
use transgate_http::DownloadEvents;

/// `DELETE /auth/v1/admin/users/{id}` with the service key. Only a documented success or a 404 is an
/// answer; anything else is an error the erasure keeps retrying, never a deletion.
pub struct GoTrueAdmin(pub Arc<SupabaseAdmin>);

impl IdentityAdmin for GoTrueAdmin {
    fn delete_user(&self, account: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>> {
        Box::pin(async move {
            match self.0.delete_user(account).await {
                Ok(AuthDeletion::Deleted) => Ok(IdentityOutcome::Deleted),
                Ok(AuthDeletion::Absent) => Ok(IdentityOutcome::Absent),
                Err(e) => Err(e.0),
            }
        })
    }
}

pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, millis: u64) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(Duration::from_millis(millis)))
    }
}

/// `transcript_downloaded` (EXP-016) through the configured analytics destination. Best effort: a
/// failed delivery is dropped, because a download never waits on or fails with its analytics.
pub struct DownloadAnalytics {
    pub sink: Arc<dyn AnalyticsSink>,
    pub clock: Arc<dyn Clock>,
}

impl DownloadEvents for DownloadAnalytics {
    fn downloaded<'a>(&'a self, account: AccountId, event: &'a DownloadEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let now = self.clock.now();
            let mut properties = Map::new();
            properties.insert("job_id".into(), json!(event.job_id));
            properties.insert("download_format".into(), json!(event.download_format));
            properties.insert("word_count".into(), json!(event.word_count));
            properties.insert("media_type".into(), json!(event.media_type));
            properties.insert("timestamps_enabled".into(), json!(event.timestamps_enabled));
            properties.insert("speakers_enabled".into(), json!(event.speakers_enabled));
            let analytics = AnalyticsEvent {
                name: "transcript_downloaded",
                distinct_id: account.to_string(),
                timestamp: Some(now),
                dedup_name: format!("transcript_downloaded:{}:{}:{}", event.job_id, event.download_format, now.0),
                properties,
            };
            let _ = self.sink.send(&analytics, &analytics.target()).await;
        })
    }
}
