//! Stored media and payload objects of jobs: the player URL and the raw
//! payload archive (UPL-016, JOB-044). A child of `jobs::app`, so it reaches
//! `Jobs`' private services and helpers directly.
//!
//! Resource safety (C10): no capability is handed out across the erasure
//! barrier (F7). Every archive object is durable intent before its first byte
//! is sent, and the PUT's known outcome is acknowledged durably, also after the
//! local cascade removed the intent (F2). An unknown outcome stays in flight.

use media_ports::{AccessMethod, ObjectRef, TemporaryAccess, TransportError};
use serde_json::Value;

use super::{Jobs, JobsError, store_kind};
use crate::ids::{AccountId, JobId, to_media};
use crate::jobs::domain::{Job, JobSource};
use crate::jobs::ingest;
use crate::jobs::ports::{ArchiveWrite, ArchivedPayload};
use crate::privacy::erasure::{self, ErasureState};
use crate::sources;

impl Jobs {
    /// UPL-016: a 900-second player URL for the owner's job. The barrier is read
    /// again after signing: a URL signed while erasure began is never returned (F7).
    pub async fn streaming_url(&self, account: AccountId, id: JobId, file_name: &str) -> Result<TemporaryAccess, JobsError> {
        self.guard(account).await?;
        let job = self.s.store.owned_job(account, id).await?.ok_or(JobsError::NotFound)?;
        let object = match &job.source {
            JobSource::Upload { object: Some(o), .. } => o.clone(),
            JobSource::Upload { storage, .. } => ObjectRef {
                store: store_kind(*storage).ok_or(JobsError::AudioUnavailable)?,
                bucket: self.config.upload_bucket.clone(),
                key: sources::upload_key(account, id, file_name),
                version: None,
            },
            JobSource::ExternalUrl { staged: Some(o), .. } => o.clone(),
            _ => return Err(JobsError::AudioUnavailable),
        };
        let store = self.object_store_for(&object).ok_or(JobsError::AudioUnavailable)?;
        let access = store
            .sign(&object, AccessMethod::Get, to_media(self.now()), sources::PLAYER_URL_TTL_SECONDS)
            .await
            .map_err(|_| JobsError::AudioUnavailable)?;
        self.guard(account).await?;
        Ok(access)
    }

    /// JOB-044, F2: the archive object of a result, named before anything is
    /// written. The caller records it with `record_archived_payload` in the result
    /// transaction, so the object stays known to erasure whatever later happens
    /// to the job row, the database or the PUT. The key holds a fresh id: no
    /// other write ever targets it, so finding the object proves this one PUT
    /// landed (the adapters never retry a PUT).
    pub(super) fn archive_intent(&self, job: &Job, file: &str) -> ArchivedPayload {
        ArchivedPayload {
            account: job.account,
            job: job.id,
            object: ObjectRef {
                store: self.config.archive_store,
                bucket: self.config.archive_bucket.clone(),
                key: ingest::archive_key(job, uuid::Uuid::new_v4(), file),
                version: None,
            },
            write: ArchiveWrite::InFlight,
        }
    }

    /// JOB-044: after the result commit, best effort; a failure is only logged by
    /// the host. The PUT begins only with permission granted under the barrier
    /// lock (`permit_archive`); refused, nothing is sent. A completed PUT is
    /// acknowledged `Written`, a refused or unsent one `NotWritten`; any other
    /// failure leaves the outcome unknown, so the intent stays in flight.
    pub(super) async fn archive_payload(&self, intent: &ArchivedPayload, body: Value) {
        let Some(store) = self.object_store_for(&intent.object) else {
            return self.acknowledge_archive(intent, ArchiveWrite::NotWritten).await;
        };
        if !self.permit_archive(intent).await {
            return self.acknowledge_archive(intent, ArchiveWrite::NotWritten).await;
        }
        let bytes = serde_json::to_vec_pretty(&body).unwrap_or_default();
        let len = bytes.len() as u64;
        let stream: media_ports::ByteStream = Box::pin(OneChunk(Some(bytes)));
        let put = store
            .put(media_ports::PutObject { object: intent.object.clone(), body: stream, content_length: len, content_type: "application/json".into() })
            .await;
        match put {
            Ok(_) => self.acknowledge_archive(intent, ArchiveWrite::Written).await,
            Err(TransportError::Rejected { .. }) => self.acknowledge_archive(intent, ArchiveWrite::NotWritten).await,
            Err(_) => {}
        }
    }

    /// F2: permission to begin the PUT, read under the barrier lock in its own
    /// transaction (no network inside). Once granted, an erasure opening later
    /// finds the intent still in flight, so nothing it does can miss the write.
    async fn permit_archive(&self, intent: &ArchivedPayload) -> bool {
        let Ok(mut uow) = self.s.store.begin().await else { return false };
        let barrier = uow.lock_barrier(intent.account).await;
        match barrier {
            Ok(None) => uow.commit().await.is_ok(),
            _ => {
                let _ = uow.rollback().await;
                false
            }
        }
    }

    /// F2: one transaction under the barrier lock. Before the manifest, the
    /// intent row carries the outcome into it; after the local cascade, the
    /// outcome goes to the erasure's items, reopening cleanup for a written
    /// object. A failed acknowledgement leaves the intent in flight.
    async fn acknowledge_archive(&self, intent: &ArchivedPayload, write: ArchiveWrite) {
        let Ok(mut uow) = self.s.store.begin().await else { return };
        let result: Result<(), JobsError> = async {
            match uow.lock_barrier(intent.account).await? {
                Some(mut record) if record.state != ErasureState::Requested => {
                    let items = uow.erasure_items(record.id).await?;
                    let ack = erasure::acknowledge_archive(&record, &items, &intent.object, write, self.now());
                    for item in &ack.save {
                        uow.save_erasure_item(item).await?;
                    }
                    if !ack.insert.is_empty() {
                        uow.insert_erasure_items(&ack.insert).await?;
                    }
                    if ack.reopen {
                        record.state = ErasureState::LocalErased;
                        record.closed_at = None;
                        uow.save_erasure(&record).await?;
                    }
                }
                _ => uow.settle_archived_payload(intent.account, &intent.object, write).await?,
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                let _ = uow.commit().await;
            }
            Err(_) => {
                let _ = uow.rollback().await;
            }
        }
    }
}

/// A single-chunk byte stream for archive uploads.
struct OneChunk(Option<Vec<u8>>);

impl futures_core::Stream for OneChunk {
    type Item = Result<bytes::Bytes, media_ports::TransportError>;

    fn poll_next(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.0.take().map(|b| Ok(bytes::Bytes::from(b))))
    }
}

#[cfg(test)]
mod tests;
