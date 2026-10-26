//! A configured delivery adapter: one JSON POST per message to a transactional
//! email API (the shape Resend and similar services use), with a stable
//! `Idempotency-Key` so a retry after an unknown outcome is not a second email.
//! Delivery is at least once, never claimed exactly once.

use std::time::Duration;

use billing_core::ports::{MailError, MailMessage, Mailer};
use billing_core::types::BoxFuture;
use serde_json::json;

pub struct HttpMailer {
    http: reqwest::Client,
    url: String,
    api_key: String,
    from: String,
}

impl HttpMailer {
    pub fn new(url: &str, api_key: &str, from: &str, timeout: Duration) -> Result<HttpMailer, String> {
        if url.is_empty() || api_key.is_empty() || from.is_empty() {
            return Err("mail delivery needs a URL, an API key and a sender".into());
        }
        let http = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
        Ok(HttpMailer { http, url: url.into(), api_key: api_key.into(), from: from.into() })
    }
}

impl Mailer for HttpMailer {
    fn send<'a>(&'a self, message: &'a MailMessage, idempotency_key: &'a str) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            let body = json!({
                "from": self.from,
                "to": [message.to],
                "subject": message.subject,
                "text": message.text,
            });
            let response = self
                .http
                .post(&self.url)
                .header("authorization", format!("Bearer {}", self.api_key))
                .header("content-type", "application/json")
                .header("idempotency-key", idempotency_key)
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| MailError(format!("sending failed: {}", e.without_url())))?;
            let status = response.status();
            if status.is_success() {
                Ok(())
            } else {
                Err(MailError(format!("the mail service answered HTTP {}", status.as_u16())))
            }
        })
    }
}
