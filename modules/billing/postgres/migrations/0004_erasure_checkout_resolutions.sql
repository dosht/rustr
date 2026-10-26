-- Verified closure of an unresolved plan checkout during account erasure (P3, D10, D13).
--
-- An exposed checkout form may still create a subscription after a lost or
-- uncertain confirmation, and no local status proves it never will. Erasure
-- therefore waits on every plan checkout no termination target of the same
-- account covers, until an authenticated provider result adopts its
-- subscription as a target, or an operator closes it. A closure is never a risk
-- waiver: it attests, from provider evidence kept at `evidence_ref`, both that
-- the checkout can no longer complete and that no subscription from it renews.
-- The row keeps identifiers, the operator, the attestations and the statement
-- only (D9), outlives the cascade, and cannot be changed or removed by the runtime.

CREATE TABLE billing.erasure_checkout_resolutions (
  account_id uuid NOT NULL REFERENCES billing.erasure_barriers (account_id),
  -- No foreign key: the payment row goes with the cascade, the audit stays.
  payment_id bigint PRIMARY KEY,
  payment_gateway text NOT NULL,
  environment text NOT NULL,
  token text,
  resolved_by text NOT NULL CHECK (btrim(resolved_by) <> ''),
  checkout_cannot_complete boolean NOT NULL CHECK (checkout_cannot_complete),
  no_renewing_subscription boolean NOT NULL CHECK (no_renewing_subscription),
  evidence_ref text NOT NULL CHECK (btrim(evidence_ref) <> ''),
  rationale text NOT NULL CHECK (btrim(rationale) <> ''),
  resolved_at timestamptz NOT NULL
);

CREATE INDEX erasure_checkout_resolutions_account ON billing.erasure_checkout_resolutions (account_id);

GRANT SELECT, INSERT ON billing.erasure_checkout_resolutions TO billing_runtime;
GRANT SELECT, INSERT, UPDATE ON billing.erasure_checkout_resolutions TO billing_import;

-- The account row may be deleted only when every bound subscription has a
-- confirmed termination of the same account and every plan checkout is covered
-- by a confirmed termination of the same account or a verified closure. A
-- target recorded for another account never covers this one's references.
DROP POLICY runtime_erase ON billing.accounts;
CREATE POLICY runtime_erase ON billing.accounts FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = accounts.account_id)
         AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                         WHERE t.account_id = accounts.account_id AND t.status <> 'confirmed')
         AND NOT EXISTS (SELECT 1 FROM billing.subscriptions s
                         WHERE s.account_id = accounts.account_id
                           AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                           WHERE t.account_id = s.account_id
                                             AND t.payment_gateway = s.payment_gateway AND t.environment = s.environment
                                             AND t.subscription_ref = s.subscription_ref AND t.status = 'confirmed'))
         AND NOT EXISTS (SELECT 1 FROM billing.payments p
                         WHERE p.account_id = accounts.account_id AND p.pricing_plan_id IS NOT NULL
                           AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                           WHERE t.account_id = p.account_id
                                             AND t.payment_gateway = p.payment_gateway AND t.environment = p.environment
                                             AND t.subscription_ref = p.subscription_ref AND t.status = 'confirmed')
                           AND NOT EXISTS (SELECT 1 FROM billing.erasure_checkout_resolutions r
                                           WHERE r.payment_id = p.id AND r.account_id = p.account_id)));
