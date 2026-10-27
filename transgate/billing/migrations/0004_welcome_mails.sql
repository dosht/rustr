-- EML-002: one welcome mail per account creation. The queue row is written by the database in the
-- same statement that creates the customer record, so every creation path (the signup route and the
-- first authenticated access) queues exactly one, a replay or refresh of an existing customer queues
-- none, and a record without an address (a malformed creation) queues none. The row goes with the
-- customer record on account erasure.

CREATE TABLE transgate_billing.welcome_mails (
  account_id uuid PRIMARY KEY REFERENCES transgate_billing.customers (account_id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL DEFAULT now(),
  attempts integer NOT NULL DEFAULT 0,
  claimed_until timestamptz,
  sent_at timestamptz
);

CREATE FUNCTION transgate_billing.queue_welcome() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  INSERT INTO transgate_billing.welcome_mails (account_id) VALUES (NEW.account_id) ON CONFLICT DO NOTHING;
  RETURN NEW;
END
$$;

CREATE TRIGGER customers_queue_welcome AFTER INSERT ON transgate_billing.customers
  FOR EACH ROW WHEN (btrim(NEW.email) <> '')
  EXECUTE FUNCTION transgate_billing.queue_welcome();

REVOKE ALL ON transgate_billing.welcome_mails FROM PUBLIC;
GRANT SELECT, INSERT ON transgate_billing.welcome_mails TO billing_runtime;
GRANT UPDATE (attempts, claimed_until, sent_at) ON transgate_billing.welcome_mails TO billing_runtime;
