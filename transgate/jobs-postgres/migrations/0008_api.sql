-- P7 (docs/plans/p7-api/decisions.md D1-D5, D10, D11): the public API's rows in
-- their own schema, written by the same restricted runtime group and in the same
-- transactions as every other account write. Keys, their audit rows,
-- idempotency records, callback deliveries and attempts, signing secrets and the
-- per-key rate windows all go with account erasure (`erase_account_rows`).
--
-- Times are timestamptz written through transgate_jobs.us_to_ts and read through
-- transgate_jobs.ts_to_us, so a session time zone never changes a value.

CREATE SCHEMA transgate_api;

-- API-002/019, D2: one row per key. A migrated source key keeps only its bcrypt
-- hash until its first use writes the SHA-256 digest and nulls the hash.
CREATE TABLE transgate_api.api_keys (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  name text NOT NULL,
  created_at timestamptz NOT NULL,
  expires_at timestamptz,
  last_used_at timestamptz,
  active boolean NOT NULL,
  scopes text[] NOT NULL,
  expiration_policy text NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}',
  digest text UNIQUE,
  legacy_bcrypt text,
  hint text,
  CONSTRAINT api_keys_identified CHECK (digest IS NOT NULL OR legacy_bcrypt IS NOT NULL)
);

CREATE INDEX api_keys_account ON transgate_api.api_keys (account_id, created_at DESC, id);
-- The D2 scan set: active keys not yet upgraded.
CREATE INDEX api_keys_legacy ON transgate_api.api_keys (id) WHERE digest IS NULL AND active;

-- D11: one audit row per authenticated request (source parity, mu decides later).
CREATE TABLE transgate_api.key_uses (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  key_id uuid NOT NULL REFERENCES transgate_api.api_keys (id) ON DELETE CASCADE,
  account_id uuid NOT NULL,
  at timestamptz NOT NULL,
  endpoint text NOT NULL,
  method text NOT NULL,
  status integer NOT NULL,
  ip text,
  user_agent text,
  duration_ms integer NOT NULL
);

CREATE INDEX key_uses_key ON transgate_api.key_uses (key_id, at);
CREATE INDEX key_uses_account ON transgate_api.key_uses (account_id);

-- D3: calendar-aligned minute and hour windows per key, consumed together.
CREATE TABLE transgate_api.rate_windows (
  key_id uuid NOT NULL REFERENCES transgate_api.api_keys (id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('minute', 'hour')),
  window_start timestamptz NOT NULL,
  count integer NOT NULL CHECK (count >= 0),
  PRIMARY KEY (key_id, kind, window_start)
);

-- D4: an optional Idempotency-Key per account, remembered for 24 hours.
CREATE TABLE transgate_api.idempotency (
  account_id uuid NOT NULL,
  key text NOT NULL,
  request_digest text NOT NULL,
  request_type text NOT NULL,
  jobs bigint[] NOT NULL,
  answer jsonb,
  created_at timestamptz NOT NULL,
  PRIMARY KEY (account_id, key)
);

-- D5/D10: one delivery per completed event with a callback URL. The job is
-- referenced without a foreign key: a hard-deleted job abandons its delivery.
CREATE TABLE transgate_api.callback_deliveries (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  job_id bigint NOT NULL,
  event_id bigint NOT NULL UNIQUE,
  url text NOT NULL,
  state text NOT NULL CHECK (state IN ('pending', 'delivered', 'failed', 'abandoned')),
  attempts integer NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL,
  completed_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL,
  delivered_at timestamptz,
  payload jsonb,
  claim_token uuid,
  claimed_until timestamptz,
  -- Why a delivery ended without an attempt (its job is gone).
  note text
);

CREATE INDEX callback_deliveries_due ON transgate_api.callback_deliveries (next_attempt_at, id) WHERE state = 'pending';
CREATE INDEX callback_deliveries_account ON transgate_api.callback_deliveries (account_id);
CREATE INDEX callback_deliveries_job ON transgate_api.callback_deliveries (job_id);

-- API-017/D5: an attempt row is written before each POST and completed after.
CREATE TABLE transgate_api.callback_attempts (
  delivery_id uuid NOT NULL REFERENCES transgate_api.callback_deliveries (id) ON DELETE CASCADE,
  n integer NOT NULL CHECK (n >= 1),
  started_at timestamptz NOT NULL,
  outcome jsonb,
  finished_at timestamptz,
  PRIMARY KEY (delivery_id, n)
);

-- D5: the per-account HMAC secret, created on the first delivery.
CREATE TABLE transgate_api.signing_secrets (
  account_id uuid PRIMARY KEY,
  secret text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

REVOKE ALL ON SCHEMA transgate_api FROM PUBLIC;
GRANT USAGE ON SCHEMA transgate_api TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.api_keys TO billing_runtime;
GRANT UPDATE (name, active, last_used_at, digest, legacy_bcrypt, hint) ON transgate_api.api_keys TO billing_runtime;

-- Audit rows are written once and never edited.
GRANT SELECT, INSERT, DELETE ON transgate_api.key_uses TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.rate_windows TO billing_runtime;
GRANT UPDATE (count) ON transgate_api.rate_windows TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.idempotency TO billing_runtime;
GRANT UPDATE (request_digest, request_type, jobs, answer, created_at) ON transgate_api.idempotency TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.callback_deliveries TO billing_runtime;
GRANT UPDATE (state, attempts, next_attempt_at, delivered_at, payload, claim_token, claimed_until, note)
  ON transgate_api.callback_deliveries TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.callback_attempts TO billing_runtime;
GRANT UPDATE (outcome, finished_at) ON transgate_api.callback_attempts TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_api.signing_secrets TO billing_runtime;
