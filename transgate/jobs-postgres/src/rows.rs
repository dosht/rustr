//! Row mapping. Enums are stored as their serde names, structured values as
//! JSON, instants as `timestamptz` read back as UTC microseconds.

use billing_core::ports::StoreError;
use media_ports::SecretString;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::Row;
use transgate_core::ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
use transgate_core::jobs::domain::{Attempt, Job, JobStatus, Provider};
use transgate_core::jobs::ports::{InboxEntry, NewInboxEntry};
use transgate_core::languages::Language;
use transgate_core::privacy::erasure::{ErasureItem, ErasureRecord};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::Line;

macro_rules! job_cols {
    () => {
        "id, account_id, revision, status, job_name, source, language, translate_to, speakers, fix_grammer,
         duration_minutes, size_bytes, provider, model, callback_url, whisper_params, error, retry_count,
         recovery_attempts, soniox_transcription_id,
         transgate_jobs.ts_to_us(started_at) AS started_us, transgate_jobs.ts_to_us(created_at) AS created_us,
         transgate_jobs.ts_to_us(updated_at) AS updated_us, current_attempt, attempts_made, deferred_charge,
         transgate_jobs.ts_to_us(next_dispatch_at) AS next_dispatch_us"
    };
}

macro_rules! attempt_cols {
    () => {
        "id, job_id, account_id, number, provider, project, model, state,
         transgate_jobs.ts_to_us(claimed_at) AS claimed_us, transgate_jobs.ts_to_us(lease_until) AS lease_us,
         request, callback_secret, remote_id, transgate_jobs.ts_to_us(accepted_at) AS accepted_us, last_error,
         remote_cleanup"
    };
}

macro_rules! line_cols {
    () => {
        "id, job_id, account_id, start_time, end_time, line, speaker, corrected_text, split_on_speaker_change,
         language, translation_status, transgate_jobs.ts_to_us(created_at) AS created_us"
    };
}

macro_rules! inbox_cols {
    () => {
        "id, attempt_id, job_id, account_id, remote_id, origin, observation, fingerprint,
         transgate_jobs.ts_to_us(received_at) AS received_us, state,
         transgate_jobs.ts_to_us(settled_at) AS settled_us, note, failures"
    };
}

macro_rules! erasure_cols {
    () => {
        "id, account_id, transgate_jobs.ts_to_us(requested_at) AS requested_us, state,
         transgate_jobs.ts_to_us(local_erased_at) AS local_erased_us,
         transgate_jobs.ts_to_us(identity_deleted_at) AS identity_deleted_us,
         transgate_jobs.ts_to_us(closed_at) AS closed_us"
    };
}

pub(crate) use {attempt_cols, erasure_cols, inbox_cols, job_cols, line_cols};

pub(crate) fn corrupt(what: &str, e: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(format!("stored {what} is unreadable: {e}"))
}

pub(crate) fn get<'r, T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>>(row: &'r PgRow, column: &str) -> Result<T, StoreError> {
    row.try_get(column).map_err(|e| corrupt(column, e))
}

/// The serde name of a unit enum value, as stored.
pub(crate) fn to_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => Ok(s),
        Ok(other) => Err(StoreError::Unavailable(format!("{other} is not a storable name"))),
        Err(e) => Err(StoreError::Unavailable(e.to_string())),
    }
}

pub(crate) fn from_text<T: DeserializeOwned>(text: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_value(Value::String(text.to_string())).map_err(|e| corrupt(what, e))
}

fn language(name: Option<String>) -> Result<Option<Language>, StoreError> {
    name.map(|n| Language::from_name(&n).ok_or_else(|| corrupt("language", format!("unknown language {n:?}"))))
        .transpose()
}

fn micros(us: Option<i64>) -> Option<Micros> {
    us.map(Micros)
}

pub(crate) fn job_from(row: &PgRow) -> Result<Job, StoreError> {
    let status: String = get(row, "status")?;
    let provider: Option<String> = get(row, "provider")?;
    let targets: Vec<String> = get(row, "translate_to")?;
    let deferred: String = get(row, "deferred_charge")?;
    Ok(Job {
        id: JobId(get(row, "id")?),
        account: AccountId(get(row, "account_id")?),
        revision: get(row, "revision")?,
        status: JobStatus::parse(&status).ok_or_else(|| corrupt("job status", &status))?,
        name: get(row, "job_name")?,
        source: get::<Json<_>>(row, "source")?.0,
        language: language(get(row, "language")?)?,
        translate_to: targets
            .into_iter()
            .map(|t| Language::from_name(&t).ok_or_else(|| corrupt("translation target", format!("unknown language {t:?}"))))
            .collect::<Result<_, _>>()?,
        speakers: get(row, "speakers")?,
        fix_grammer: get(row, "fix_grammer")?,
        duration_minutes: get(row, "duration_minutes")?,
        size_bytes: get(row, "size_bytes")?,
        provider: provider.map(|p| Provider::parse(&p).ok_or_else(|| corrupt("provider", &p))).transpose()?,
        model: get(row, "model")?,
        callback_url: get(row, "callback_url")?,
        whisper_params: get(row, "whisper_params")?,
        error: get(row, "error")?,
        retry_count: get(row, "retry_count")?,
        recovery_attempts: get(row, "recovery_attempts")?,
        soniox_transcription_id: get(row, "soniox_transcription_id")?,
        started_at: micros(get(row, "started_us")?),
        created_at: Micros(get(row, "created_us")?),
        updated_at: Micros(get(row, "updated_us")?),
        current_attempt: get::<Option<uuid::Uuid>>(row, "current_attempt")?.map(AttemptId),
        attempts_made: get(row, "attempts_made")?,
        deferred_charge: from_text(&deferred, "deferred charge")?,
        next_dispatch_at: micros(get(row, "next_dispatch_us")?),
    })
}

pub(crate) fn attempt_from(row: &PgRow) -> Result<Attempt, StoreError> {
    let provider: String = get(row, "provider")?;
    let state: String = get(row, "state")?;
    let secret: String = get(row, "callback_secret")?;
    Ok(Attempt {
        id: AttemptId(get(row, "id")?),
        job: JobId(get(row, "job_id")?),
        account: AccountId(get(row, "account_id")?),
        number: get(row, "number")?,
        provider: Provider::parse(&provider).ok_or_else(|| corrupt("provider", &provider))?,
        project: get(row, "project")?,
        model: get(row, "model")?,
        state: from_text(&state, "attempt state")?,
        claimed_at: Micros(get(row, "claimed_us")?),
        lease_until: Micros(get(row, "lease_us")?),
        request: get::<Json<_>>(row, "request")?.0,
        callback_secret: SecretString::new(secret),
        remote_id: get(row, "remote_id")?,
        accepted_at: micros(get(row, "accepted_us")?),
        last_error: get(row, "last_error")?,
        remote_cleanup: get::<Json<_>>(row, "remote_cleanup")?.0,
    })
}

pub(crate) fn line_from(row: &PgRow) -> Result<Line, StoreError> {
    let status: Option<String> = get(row, "translation_status")?;
    Ok(Line {
        id: LineId(get(row, "id")?),
        job: JobId(get(row, "job_id")?),
        account: AccountId(get(row, "account_id")?),
        start_time: get(row, "start_time")?,
        end_time: get(row, "end_time")?,
        line: get(row, "line")?,
        speaker: get(row, "speaker")?,
        corrected_text: get(row, "corrected_text")?,
        split_on_speaker_change: get(row, "split_on_speaker_change")?,
        language: language(get(row, "language")?)?,
        translation_status: status.map(|s| from_text(&s, "translation status")).transpose()?,
        created_at: Micros(get(row, "created_us")?),
    })
}

pub(crate) fn inbox_from(row: &PgRow) -> Result<InboxEntry, StoreError> {
    let origin: String = get(row, "origin")?;
    let state: String = get(row, "state")?;
    Ok(InboxEntry {
        id: InboxId(get(row, "id")?),
        entry: NewInboxEntry {
            attempt: AttemptId(get(row, "attempt_id")?),
            job: JobId(get(row, "job_id")?),
            account: AccountId(get(row, "account_id")?),
            remote_id: get(row, "remote_id")?,
            origin: from_text(&origin, "observation origin")?,
            observation: get::<Json<_>>(row, "observation")?.0,
            fingerprint: get(row, "fingerprint")?,
            received_at: Micros(get(row, "received_us")?),
        },
        state: from_text(&state, "inbox state")?,
        settled_at: micros(get(row, "settled_us")?),
        note: get(row, "note")?,
        failures: get(row, "failures")?,
    })
}

pub(crate) fn settings_from(row: &PgRow) -> Result<UserSettings, StoreError> {
    Ok(UserSettings {
        account: AccountId(get(row, "account_id")?),
        auto_delete_transcripts: get(row, "auto_delete_transcripts")?,
        unlimited_history: get(row, "unlimited_history")?,
        blocked: get(row, "blocked")?,
        deleted_monthly: get(row, "deleted_monthly")?,
    })
}

pub(crate) fn erasure_from(row: &PgRow) -> Result<ErasureRecord, StoreError> {
    let state: String = get(row, "state")?;
    Ok(ErasureRecord {
        id: ErasureId(get(row, "id")?),
        account: AccountId(get(row, "account_id")?),
        requested_at: Micros(get(row, "requested_us")?),
        state: from_text(&state, "erasure state")?,
        local_erased_at: micros(get(row, "local_erased_us")?),
        identity_deleted_at: micros(get(row, "identity_deleted_us")?),
        closed_at: micros(get(row, "closed_us")?),
    })
}

pub(crate) fn item_from(row: &PgRow) -> Result<ErasureItem, StoreError> {
    Ok(ErasureItem {
        id: get(row, "id")?,
        erasure: ErasureId(get(row, "erasure_id")?),
        target: get::<Json<_>>(row, "target")?.0,
        state: get::<Json<_>>(row, "state")?.0,
        attempts: get(row, "attempts")?,
    })
}
