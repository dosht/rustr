-- Transgate's own customer records: the host-owned directory billing consults
-- for recipients, eligibility and, above all, the trusted account creation
-- instant that anchors the free trial. History lives in
-- transgate_billing._sqlx_migrations. Run after the billing migrations: the
-- billing_runtime role must already exist.

CREATE SCHEMA IF NOT EXISTS transgate_billing;

CREATE TABLE transgate_billing.customers (
  account_id uuid PRIMARY KEY,
  email text NOT NULL,
  full_name text,
  -- When the identity system created the account. Never a token timestamp,
  -- never the first billing request.
  created_at timestamptz NOT NULL
);

CREATE UNIQUE INDEX customers_email ON transgate_billing.customers (lower(email));

REVOKE ALL ON SCHEMA transgate_billing FROM PUBLIC;
GRANT USAGE ON SCHEMA transgate_billing TO billing_runtime;
GRANT SELECT, INSERT ON transgate_billing.customers TO billing_runtime;
