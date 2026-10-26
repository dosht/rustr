-- Resource safety F2: an archived payload records whether its PUT is still in
-- flight, landed, or definitely did not land. The intent is recorded as
-- in_flight inside the apply transaction, before the PUT, and settled after
-- it. Rows recorded before this migration were written after a successful PUT.

ALTER TABLE transgate_jobs.archived_payloads
  ADD COLUMN write text NOT NULL DEFAULT 'written' CHECK (write IN ('in_flight', 'written', 'not_written'));
ALTER TABLE transgate_jobs.archived_payloads ALTER COLUMN write DROP DEFAULT;

GRANT UPDATE (write) ON transgate_jobs.archived_payloads TO billing_runtime;
