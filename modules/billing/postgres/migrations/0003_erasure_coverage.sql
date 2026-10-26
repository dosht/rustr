-- Account erasure completes on coverage, not presence (P3, D10).
--
-- The account row may be deleted only when every subscription bound to it and
-- every plan checkout it started, whatever the checkout's local status, is
-- covered by a confirmed termination. A binding or checkout that no target
-- covers keeps the row, so a falsely complete status cannot erase the last
-- correlation to a subscription the provider still renews.

DROP POLICY runtime_erase ON billing.accounts;
CREATE POLICY runtime_erase ON billing.accounts FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = accounts.account_id)
         AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                         WHERE t.account_id = accounts.account_id AND t.status <> 'confirmed')
         AND NOT EXISTS (SELECT 1 FROM billing.subscriptions s
                         WHERE s.account_id = accounts.account_id
                           AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                           WHERE t.payment_gateway = s.payment_gateway AND t.environment = s.environment
                                             AND t.subscription_ref = s.subscription_ref AND t.status = 'confirmed'))
         AND NOT EXISTS (SELECT 1 FROM billing.payments p
                         WHERE p.account_id = accounts.account_id AND p.pricing_plan_id IS NOT NULL
                           AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                           WHERE t.payment_gateway = p.payment_gateway AND t.environment = p.environment
                                             AND t.subscription_ref = p.subscription_ref AND t.status = 'confirmed')));
