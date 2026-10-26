-- Purging old scheduler run history (CRN-007, PRV-025/031 adapted). A runtime
-- role granted DELETE by `grant_runtime` may remove only finished runs: a
-- pending or running claim is invisible to its DELETE. The owner is unaffected.

ALTER TABLE media.schedule_slots ENABLE ROW LEVEL SECURITY;

CREATE POLICY runtime_read ON media.schedule_slots FOR SELECT USING (true);
CREATE POLICY runtime_claim ON media.schedule_slots FOR INSERT WITH CHECK (true);
CREATE POLICY runtime_update ON media.schedule_slots FOR UPDATE USING (true) WITH CHECK (true);
CREATE POLICY runtime_purge ON media.schedule_slots FOR DELETE USING (finished_at IS NOT NULL);
