-- P4 AI persistence (D1..D5): consent with write epochs and a DELETE purge
-- cutoff, per-job source revisions, generations with one bounded snapshot and
-- a staged final (no token or event log), chat pairs, completion summaries and
-- their durable producer queue, and content-free quota counters.
--
-- Every row lives in the account-first lock domain of jobs and billing. Rows
-- owned by a job go with a hard job deletion; account erasure deletes all of
-- them, counters and consent included. A consent epoch of NULL is a legacy
-- import and is covered by every DELETE purge.

CREATE TABLE transgate_jobs.ai_consent (
  account_id uuid PRIMARY KEY,
  has_consented boolean NOT NULL DEFAULT false,
  version text NOT NULL,
  provider text NOT NULL,
  policy_url text NOT NULL,
  consent_date timestamptz,
  revoked_date timestamptz,
  epoch bigint NOT NULL,
  purge_cutoff bigint,
  purge_pending boolean NOT NULL DEFAULT false,
  updated_at timestamptz NOT NULL
);

CREATE INDEX ai_consent_purge_pending ON transgate_jobs.ai_consent (updated_at) WHERE purge_pending;

-- The job's content revision: advanced by edits, renames, retention and soft deletion.
CREATE TABLE transgate_jobs.source_revisions (
  job_id bigint PRIMARY KEY REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  revision bigint NOT NULL CHECK (revision > 0)
);

CREATE TABLE transgate_jobs.generations (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  job_id bigint NOT NULL REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('analysis', 'chat', 'completion_summary')),
  request_key uuid NOT NULL,
  request_digest text NOT NULL,
  source_revision bigint NOT NULL,
  consent_epoch bigint,
  input jsonb NOT NULL,
  status text NOT NULL CHECK (status IN ('accepted', 'running', 'completed', 'interrupted', 'cancelled')),
  sequence bigint NOT NULL,
  draft jsonb NOT NULL,
  staged jsonb,
  execution_token uuid,
  lease_until timestamptz,
  quota jsonb,
  interruption jsonb,
  remote_outcome_unknown boolean NOT NULL DEFAULT false,
  remote_id text,
  tokens_used bigint,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  finished_at timestamptz,
  UNIQUE (account_id, request_key),
  CHECK ((execution_token IS NULL) = (lease_until IS NULL))
);

CREATE INDEX generations_job ON transgate_jobs.generations (job_id, kind, created_at);
CREATE INDEX generations_account ON transgate_jobs.generations (account_id);
CREATE INDEX generations_runnable ON transgate_jobs.generations (created_at) WHERE status IN ('accepted', 'running');
-- D2: at most one locally active metered generation per account.
CREATE UNIQUE INDEX generations_one_active_metered ON transgate_jobs.generations (account_id)
  WHERE quota IS NOT NULL AND status IN ('accepted', 'running');

CREATE TABLE transgate_jobs.chat_messages (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  job_id bigint NOT NULL REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  generation_id uuid NOT NULL REFERENCES transgate_jobs.generations (id) ON DELETE CASCADE,
  message_order bigint NOT NULL,
  role text NOT NULL CHECK (role IN ('user', 'assistant')),
  content text NOT NULL,
  consent_epoch bigint,
  created_at timestamptz NOT NULL,
  UNIQUE (job_id, message_order),
  UNIQUE (generation_id, role)
);

CREATE INDEX chat_messages_account ON transgate_jobs.chat_messages (account_id);

CREATE TABLE transgate_jobs.completion_summaries (
  job_id bigint NOT NULL REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  source_revision bigint NOT NULL,
  account_id uuid NOT NULL,
  generation_id uuid NOT NULL,
  text text NOT NULL,
  consent_epoch bigint,
  created_at timestamptz NOT NULL,
  PRIMARY KEY (job_id, source_revision)
);

CREATE INDEX completion_summaries_account ON transgate_jobs.completion_summaries (account_id);

-- Content-free: survives consent refusal, revocation and DELETE purge.
CREATE TABLE transgate_jobs.summary_requests (
  job_id bigint NOT NULL REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  source_revision bigint NOT NULL,
  account_id uuid NOT NULL,
  requested_at timestamptz NOT NULL,
  generation_id uuid,
  outcome jsonb,
  decided_at timestamptz,
  PRIMARY KEY (job_id, source_revision)
);

CREATE INDEX summary_requests_pending ON transgate_jobs.summary_requests (requested_at) WHERE outcome IS NULL;
CREATE INDEX summary_requests_account ON transgate_jobs.summary_requests (account_id);

-- Content-free counters: survive refusal, revocation and purge, not account
-- erasure. Transcript-scoped rows go with their job, as source chat_limits did
-- (job_id ON DELETE CASCADE); account-wide rows have no job and stay.
CREATE TABLE transgate_jobs.quota_counters (
  account_id uuid NOT NULL,
  scope text NOT NULL CHECK (scope IN ('transcript', 'user')),
  job_id bigint REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('questions', 'tokens')),
  period text NOT NULL CHECK (period IN ('daily', 'weekly', 'monthly', 'transcript')),
  count bigint NOT NULL,
  max_limit bigint,
  reset_at timestamptz,
  CONSTRAINT quota_counters_key UNIQUE NULLS NOT DISTINCT (account_id, scope, job_id, kind, period),
  CHECK ((scope = 'transcript') = (job_id IS NOT NULL)),
  CHECK ((period = 'transcript') = (scope = 'transcript'))
);

GRANT SELECT, INSERT, DELETE ON transgate_jobs.ai_consent TO billing_runtime;
GRANT UPDATE (has_consented, version, provider, policy_url, consent_date, revoked_date, epoch, purge_cutoff,
              purge_pending, updated_at)
  ON transgate_jobs.ai_consent TO billing_runtime;

GRANT SELECT, INSERT ON transgate_jobs.source_revisions TO billing_runtime;
GRANT UPDATE (revision) ON transgate_jobs.source_revisions TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_jobs.generations TO billing_runtime;
GRANT UPDATE (status, sequence, draft, staged, execution_token, lease_until, quota, interruption,
              remote_outcome_unknown, remote_id, tokens_used, updated_at, finished_at)
  ON transgate_jobs.generations TO billing_runtime;

-- Chat pairs and completion summaries are written once and never edited.
GRANT SELECT, INSERT, DELETE ON transgate_jobs.chat_messages, transgate_jobs.completion_summaries TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_jobs.summary_requests TO billing_runtime;
GRANT UPDATE (generation_id, outcome, decided_at) ON transgate_jobs.summary_requests TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_jobs.quota_counters TO billing_runtime;
GRANT UPDATE (count, max_limit, reset_at) ON transgate_jobs.quota_counters TO billing_runtime;
