//! Ordered support delivery uses the same durable leases and stable mail keys as job mail.

use billing_core::ports::{CustomerDirectory, MailMessage};
use transgate_core::account::{SupportMail, acknowledgement_name};
use transgate_core::account::ports::{ClaimedSupport, SupportOutbox};
use transgate_core::jobs::ports::JobQueries;

use super::{EVENT_BATCH, EVENT_LEASE_SECONDS, Micros, Workers, retry_delay_seconds};
use crate::mail::Owner;

impl Workers {
    pub async fn deliver_support(&self) -> Result<usize, String> {
        let claims = self.store.claim_support(EVENT_BATCH as u32, EVENT_LEASE_SECONDS, self.clock.now()).await.map_err(|e| e.to_string())?;
        let count = claims.len();
        for claim in claims {
            let changed = match self.deliver_support_request(&claim).await {
                Ok(()) => self.store.ack_support(&claim).await.map_err(|e| e.to_string())?,
                Err(error) => {
                    let retry_at = Micros(self.clock.now().0 + retry_delay_seconds(claim.attempts) * 1_000_000);
                    self.store.release_support(&claim, &error, retry_at).await.map_err(|e| e.to_string())?
                }
            };
            if !changed {
                eprintln!("transgate: support request {}: delivery claim is no longer current", claim.request.id.0);
            }
        }
        Ok(count)
    }

    async fn deliver_support_request(&self, claim: &ClaimedSupport) -> Result<(), String> {
        let request = &claim.request;
        if self.store.barrier(request.account).await.map_err(|e| e.to_string())? {
            return Ok(());
        }
        let owner = match self.directory.by_account(request.account).await {
            Ok(Some(customer)) => Owner { email: Some(customer.email), full_name: customer.full_name },
            Ok(None) => Owner::default(),
            Err(error) => return Err(format!("the customer directory could not answer: {error}")),
        };
        for mail in SupportMail::ORDER {
            // A claimed request can outlive the account cascade while directory or mail I/O
            // is in flight. Never dispatch another mail after observing the barrier.
            if self.store.barrier(request.account).await.map_err(|e| e.to_string())? {
                return Ok(());
            }
            let Some(to) = mail.recipient(owner.email.as_deref()) else { continue };
            let text = match mail {
                SupportMail::ToSupport => request.message.clone(),
                SupportMail::Acknowledgement => acknowledgement(owner.full_name.as_deref()),
            };
            let message = MailMessage { to, subject: mail.subject(request, owner.email.as_deref()), text };
            self.mailer.send(&message, &mail.idempotency_key(request.id)).await.map_err(|error| error.0)?;
        }
        Ok(())
    }
}

/// Plain-text rendering of the source SupportAutoReplyEmail, as for existing job mails.
fn acknowledgement(full_name: Option<&str>) -> String {
    let name = acknowledgement_name(full_name);
    format!(
        "Hello {name},\n\n\
         Thank you for reaching out to us at Transgate!\n\n\
         This is an automatic reply to let you know that we've received your message. \
         One of our team members will review it and get back to you within the next 24 hours.\n\n\
         In the meantime, you can check out our FAQ section for helpful information that might answer your question:\n\
         https://transgate.ai/frequently-asked-questions\n"
    )
}
