-- taskr's audio-work sample: its own schema in its own database, with its own
-- migration history (taskr_work._sqlx_migrations). Instants are UTC
-- microseconds (bigint), as in the shared media ports.

CREATE SCHEMA IF NOT EXISTS taskr_work;

-- Explicit enrollment: nothing exists for an account that did not ask for it.
CREATE TABLE taskr_work.enrollments (
  account_id uuid PRIMARY KEY,
  email text NOT NULL,
  enrolled_us bigint NOT NULL,
  -- Transcripts of completed work older than the retention window are removed
  -- only while this is true (the source's opt-out, true by default).
  auto_delete boolean NOT NULL DEFAULT true
);

CREATE TABLE taskr_work.works (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES taskr_work.enrollments (account_id),
  object jsonb NOT NULL,
  name text NOT NULL,
  language text,
  requested text,
  status text NOT NULL CHECK (status IN ('awaiting_upload', 'uploaded', 'pending', 'running', 'completed', 'failed')),
  revision bigint NOT NULL,
  transcript text,
  error text,
  created_us bigint NOT NULL,
  completed_us bigint
);

CREATE INDEX works_by_account ON taskr_work.works (account_id, created_us DESC);
CREATE INDEX works_open ON taskr_work.works (created_us) WHERE status IN ('uploaded', 'pending', 'running');

-- One row per provider attempt, kept for the life of the work: a superseded
-- attempt's remote identity is an erasure resource.
CREATE TABLE taskr_work.attempts (
  id uuid PRIMARY KEY,
  work_id uuid NOT NULL REFERENCES taskr_work.works (id) ON DELETE CASCADE,
  number integer NOT NULL,
  provider text NOT NULL CHECK (provider IN ('soniox', 'deepgram')),
  project text NOT NULL,
  phase text NOT NULL CHECK (phase IN ('claimed', 'accepted', 'uncertain', 'rejected', 'superseded', 'failed', 'done')),
  lease_until_us bigint NOT NULL,
  claimed_us bigint NOT NULL,
  accepted_us bigint,
  remote_id text,
  reason text,
  -- Authenticates this attempt's callback: stored before the request leaves.
  callback_secret text NOT NULL,
  -- Digest of the callback payload whose result was applied: the same attempt and identity with
  -- another payload is a conflict, held and never applied.
  result_digest text,
  UNIQUE (work_id, number)
);

-- A provider's remote identity belongs to exactly one attempt.
CREATE UNIQUE INDEX attempts_remote_identity ON taskr_work.attempts (provider, project, remote_id) WHERE remote_id IS NOT NULL;
CREATE INDEX attempts_live ON taskr_work.attempts (lease_until_us) WHERE phase IN ('claimed', 'uncertain');

-- The erasure barrier. It outlives the account's data and the completed
-- cleanup: it holds only the identifier, instants and the outcome.
CREATE TABLE taskr_work.erasures (
  account_id uuid PRIMARY KEY,
  requested_us bigint NOT NULL,
  state text NOT NULL CHECK (state IN ('pending', 'complete')),
  finished_us bigint,
  -- Set once, in the one transaction that erases billing, the enrolled sample resources and this
  -- schema's rows together.
  data_erased_us bigint,
  -- Why the data erasure has not run yet (for example subscriptions not confirmed stopped).
  note text,
  -- When a pass last tried this erasure: passes take the least recently tried first, so a few
  -- erasures that can never finish (a provider with no delete API) cannot starve the others.
  last_advanced_us bigint
);

-- What still has to be removed outside the database. Rows of a finished
-- erasure are deleted: the details are no longer needed.
CREATE TABLE taskr_work.erasure_resources (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES taskr_work.erasures (account_id),
  kind text NOT NULL CHECK (kind IN ('object', 'remote', 'unidentified')),
  target jsonb NOT NULL,
  state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'done', 'absent', 'blocked', 'unresolved')),
  blocked_until_us bigint,
  last_error text
);

CREATE INDEX erasure_resources_open ON taskr_work.erasure_resources (account_id) WHERE state IN ('pending', 'blocked', 'unresolved');

REVOKE ALL ON SCHEMA taskr_work FROM PUBLIC;
