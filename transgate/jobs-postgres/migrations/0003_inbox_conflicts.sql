-- F6: a delivery whose body differs from the stored entry for the same attempt
-- and remote id is kept as its own row in state `conflict`, pointing at that
-- entry, with its body and fingerprint. It is never pending and never applied.
-- The delivery key stays unique among primary entries only.

ALTER TABLE transgate_jobs.inbox ADD COLUMN conflict_of bigint REFERENCES transgate_jobs.inbox (id) ON DELETE CASCADE;
ALTER TABLE transgate_jobs.inbox DROP CONSTRAINT inbox_attempt_id_remote_id_key;
CREATE UNIQUE INDEX inbox_delivery ON transgate_jobs.inbox (attempt_id, remote_id) WHERE conflict_of IS NULL;
CREATE UNIQUE INDEX inbox_conflict ON transgate_jobs.inbox (conflict_of, fingerprint) WHERE conflict_of IS NOT NULL;
