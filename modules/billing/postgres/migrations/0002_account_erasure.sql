-- Account erasure (P3: decisions D5, D9 and D10).
--
-- The barrier outlives the erased account: it holds only the account id, two
-- instants and, per subscription, identifiers and outcomes. The runtime can
-- write a barrier and stamp its completion but never remove it.

CREATE TABLE billing.erasure_barriers (
  account_id uuid PRIMARY KEY,
  started_at timestamptz NOT NULL,
  billing_erased_at timestamptz
);

CREATE TABLE billing.erasure_terminations (
  account_id uuid NOT NULL REFERENCES billing.erasure_barriers (account_id),
  payment_gateway text NOT NULL,
  environment text NOT NULL,
  subscription_ref text NOT NULL,
  -- Dropped once the local cascade has run: a target detail no longer needed.
  customer_ref text,
  status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'confirmed')),
  attempts integer NOT NULL DEFAULT 0,
  last_outcome text,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (payment_gateway, environment, subscription_ref)
);

CREATE INDEX erasure_terminations_account ON billing.erasure_terminations (account_id);

GRANT SELECT, INSERT ON billing.erasure_barriers, billing.erasure_terminations TO billing_runtime;
GRANT UPDATE (billing_erased_at) ON billing.erasure_barriers TO billing_runtime;
GRANT UPDATE (customer_ref, status, attempts, last_outcome, updated_at) ON billing.erasure_terminations TO billing_runtime;
GRANT SELECT, INSERT, UPDATE ON billing.erasure_barriers, billing.erasure_terminations TO billing_import;

-- The completion stamp is written once: a runtime cannot clear it and so cannot
-- reopen an erased account for recreation.
ALTER TABLE billing.erasure_barriers ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON billing.erasure_barriers FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON billing.erasure_barriers FOR INSERT TO billing_runtime WITH CHECK (billing_erased_at IS NULL);
CREATE POLICY runtime_stamp ON billing.erasure_barriers FOR UPDATE TO billing_runtime
  USING (billing_erased_at IS NULL) WITH CHECK (billing_erased_at IS NOT NULL);
CREATE POLICY import_all ON billing.erasure_barriers FOR ALL TO billing_import USING (true) WITH CHECK (true);

-- The local cascade deletes the account row; the existing foreign keys then
-- remove its payments, packages, subscriptions and usage and keep billing_info
-- with its owner link removed (D5). The runtime may delete an account row, and
-- the account's outbox rows, only while an erasure barrier exists for it, and
-- the account row only once every subscription termination is confirmed. Row
-- security is the guard; the referential actions themselves run as the owner.
GRANT DELETE ON billing.accounts, billing.outbox, billing.outbox_deliveries TO billing_runtime;

ALTER TABLE billing.accounts ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON billing.accounts FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON billing.accounts FOR INSERT TO billing_runtime
  WITH CHECK (NOT EXISTS (SELECT 1 FROM billing.erasure_barriers b
                          WHERE b.account_id = accounts.account_id AND b.billing_erased_at IS NOT NULL));
CREATE POLICY runtime_update ON billing.accounts FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON billing.accounts FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = accounts.account_id)
         AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                         WHERE t.account_id = accounts.account_id AND t.status <> 'confirmed'));
CREATE POLICY import_all ON billing.accounts FOR ALL TO billing_import USING (true) WITH CHECK (true);

ALTER TABLE billing.outbox ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON billing.outbox FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON billing.outbox FOR INSERT TO billing_runtime WITH CHECK (true);
CREATE POLICY runtime_update ON billing.outbox FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON billing.outbox FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = outbox.account_id));
CREATE POLICY import_all ON billing.outbox FOR ALL TO billing_import USING (true) WITH CHECK (true);

ALTER TABLE billing.outbox_deliveries ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON billing.outbox_deliveries FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON billing.outbox_deliveries FOR INSERT TO billing_runtime WITH CHECK (true);
CREATE POLICY runtime_update ON billing.outbox_deliveries FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON billing.outbox_deliveries FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.outbox o JOIN billing.erasure_barriers b ON b.account_id = o.account_id
                 WHERE o.event_id = outbox_deliveries.event_id));
CREATE POLICY import_all ON billing.outbox_deliveries FOR ALL TO billing_import USING (true) WITH CHECK (true);
