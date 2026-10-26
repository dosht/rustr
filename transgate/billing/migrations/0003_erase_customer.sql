-- The customer directory row is erased with the account (P3 account erasure: the job host's
-- `erase_account_rows` deletes it inside the erasure transaction).
--
-- Two guards, both enforced by the database:
--
--  * The runtime may delete a customer row only while the billing erasure barrier exists for the
--    account (row security, as billing.accounts does).
--  * Once the barrier exists nothing may write the row again, and the check happens at write time
--    inside the writing transaction: every write takes the account's shared customer lock first and
--    only then reads the barrier. The erasing transaction must take the same lock exclusively before
--    it deletes (`pg_advisory_xact_lock(transgate_billing.customer_lock_key(account))`), so a contact
--    refresh or signup replay that began before the barrier is either committed and then deleted, or
--    waits behind the erasure and is refused by the barrier. It cannot recreate the row.

CREATE FUNCTION transgate_billing.customer_lock_key(account uuid) RETURNS bigint
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT hashtextextended('transgate_billing.customer:' || account::text, 0)
$$;

CREATE FUNCTION transgate_billing.refuse_erased_customer() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_advisory_xact_lock_shared(transgate_billing.customer_lock_key(NEW.account_id));
  IF EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = NEW.account_id) THEN
    RAISE EXCEPTION 'account % is being erased', NEW.account_id USING ERRCODE = '55006';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER customers_refuse_erased BEFORE INSERT OR UPDATE ON transgate_billing.customers
  FOR EACH ROW EXECUTE FUNCTION transgate_billing.refuse_erased_customer();

GRANT DELETE ON transgate_billing.customers TO billing_runtime;

ALTER TABLE transgate_billing.customers ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON transgate_billing.customers FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON transgate_billing.customers FOR INSERT TO billing_runtime WITH CHECK (true);
CREATE POLICY runtime_update ON transgate_billing.customers FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON transgate_billing.customers FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = customers.account_id));
