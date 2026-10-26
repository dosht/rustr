-- Verified contact facts and when they were last confirmed against the trusted
-- auth directory. `created_at` stays what the identity system said at signup and
-- is never updated: a refresh can change an address, never grant another trial.
--
-- `contact_version` orders contact facts (the auth directory's own update time).
-- NULL means "as old as created_at", so rows written by the host's own signup
-- system need no extra column. A report whose version is not newer never replaces
-- stored contact facts, so a stale signup replay cannot undo a verified email change.

ALTER TABLE transgate_billing.customers
  ADD COLUMN contact_version timestamptz,
  ADD COLUMN contact_checked_at timestamptz;

GRANT UPDATE (email, full_name, contact_version, contact_checked_at) ON transgate_billing.customers TO billing_runtime;
