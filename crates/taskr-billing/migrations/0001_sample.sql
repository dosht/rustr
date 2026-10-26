-- taskr's billing sample. Own schema, own migration history
-- (taskr_billing._sqlx_migrations). Run after the billing module's migrations:
-- the billing_runtime role must already exist.

CREATE SCHEMA IF NOT EXISTS taskr_billing;

-- The host directory: who an account is, and when it was enrolled in the
-- sample. Enrollment is explicit; billing never invents it from a token.
CREATE TABLE taskr_billing.customers (
  account_id uuid PRIMARY KEY,
  email text NOT NULL,
  full_name text,
  created_at timestamptz NOT NULL
);

CREATE UNIQUE INDEX customers_email ON taskr_billing.customers (lower(email));

-- One metered sample operation. The work identity is scoped to the account;
-- the requested units and operation are immutable once stored, and the
-- successful result is stored so a retry replays it instead of charging again.
CREATE TABLE taskr_billing.sample_work (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  account_id uuid NOT NULL,
  work_key text NOT NULL,
  units integer NOT NULL CHECK (units > 0),
  operation text NOT NULL,
  status text NOT NULL CHECK (status IN ('pending', 'done')),
  result jsonb,
  created_at timestamptz NOT NULL,
  UNIQUE (account_id, work_key)
);

REVOKE ALL ON SCHEMA taskr_billing FROM PUBLIC;
GRANT USAGE ON SCHEMA taskr_billing TO billing_runtime;
GRANT SELECT, INSERT ON taskr_billing.customers TO billing_runtime;
GRANT SELECT, INSERT ON taskr_billing.sample_work TO billing_runtime;
GRANT UPDATE (status, result) ON taskr_billing.sample_work TO billing_runtime;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA taskr_billing TO billing_runtime;
