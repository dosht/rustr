-- D9: a nonempty provider id that was observed but not kept (over 256 bytes, or
-- past the bound of 8) is still proof the request reached the provider. This
-- flag remembers it so a later report without an id cannot turn the row back
-- into not_dispatched; it only ever goes from false to true. 0005 is executed
-- and stays as it was.

ALTER TABLE transgate_jobs.llm_dispatches ADD COLUMN unkept_id_observed boolean NOT NULL DEFAULT false;
ALTER TABLE transgate_jobs.llm_dispatches
  ADD CONSTRAINT llm_dispatches_unkept_evidence CHECK (disposition = 'possible' OR NOT unkept_id_observed);

GRANT UPDATE (unkept_id_observed) ON transgate_jobs.llm_dispatches TO billing_runtime;

-- Manifest items written before this field existed record it explicitly as
-- false: nothing durable could have noted unkept evidence before now.
UPDATE transgate_jobs.erasure_items
SET target = target || '{"unkept_id_observed": false}'::jsonb
WHERE target ->> 'kind' = 'llm_dispatch' AND NOT (target ? 'unkept_id_observed');
