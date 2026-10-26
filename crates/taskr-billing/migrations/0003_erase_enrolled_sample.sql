-- Erasing an account's enrolled sample resources (the directory row and its
-- metered sample work) when the billing module erases the account.
--
-- Two guards, both enforced by the database:
--
--  * The runtime may delete these rows only while the billing erasure barrier
--    exists for the account (row security, as billing.accounts does).
--  * Once the barrier exists, nothing may write these rows again, and the
--    check happens at write time inside the writing transaction: every write
--    takes the account's shared sample lock first and only then reads the
--    barrier. The erasing transaction takes the same lock exclusively before
--    it deletes, so a write that began before the barrier is either committed
--    and then deleted, or waits behind the erasure and is refused by the
--    barrier. A refresh that was already in flight cannot recreate a row.

CREATE FUNCTION taskr_billing.sample_lock_key(account uuid) RETURNS bigint
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT hashtextextended('taskr_billing.sample:' || account::text, 0)
$$;

CREATE FUNCTION taskr_billing.refuse_erased_write() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_advisory_xact_lock_shared(taskr_billing.sample_lock_key(NEW.account_id));
  IF EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = NEW.account_id) THEN
    RAISE EXCEPTION 'account % is being erased', NEW.account_id USING ERRCODE = '55006';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER customers_refuse_erased BEFORE INSERT OR UPDATE ON taskr_billing.customers
  FOR EACH ROW EXECUTE FUNCTION taskr_billing.refuse_erased_write();
CREATE TRIGGER sample_work_refuse_erased BEFORE INSERT OR UPDATE ON taskr_billing.sample_work
  FOR EACH ROW EXECUTE FUNCTION taskr_billing.refuse_erased_write();

GRANT DELETE ON taskr_billing.customers, taskr_billing.sample_work TO billing_runtime;

ALTER TABLE taskr_billing.customers ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON taskr_billing.customers FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON taskr_billing.customers FOR INSERT TO billing_runtime WITH CHECK (true);
CREATE POLICY runtime_update ON taskr_billing.customers FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON taskr_billing.customers FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = customers.account_id));

ALTER TABLE taskr_billing.sample_work ENABLE ROW LEVEL SECURITY;
CREATE POLICY runtime_read ON taskr_billing.sample_work FOR SELECT TO billing_runtime USING (true);
CREATE POLICY runtime_insert ON taskr_billing.sample_work FOR INSERT TO billing_runtime WITH CHECK (true);
CREATE POLICY runtime_update ON taskr_billing.sample_work FOR UPDATE TO billing_runtime USING (true) WITH CHECK (true);
CREATE POLICY runtime_erase ON taskr_billing.sample_work FOR DELETE TO billing_runtime
  USING (EXISTS (SELECT 1 FROM billing.erasure_barriers b WHERE b.account_id = sample_work.account_id));
