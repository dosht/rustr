-- Shared scheduling state for media work. History lives in
-- media._sqlx_migrations, created by this crate's migrator.
--
-- No business rule lives here: product code decides which schedules exist,
-- which slot is due and what a run does. This table only makes one run per
-- (schedule, UTC slot) claimable once, recoverable after a crash, and final.

CREATE SCHEMA IF NOT EXISTS media;

CREATE FUNCTION media.us_to_ts(us bigint) RETURNS timestamptz
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT timestamptz 'epoch'
       + (us / 1000000) * interval '1 second'
       + (us % 1000000) * interval '1 microsecond'
$$;

CREATE FUNCTION media.ts_to_us(ts timestamptz) RETURNS bigint
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT (extract(epoch FROM ts) * 1000000)::bigint
$$;

CREATE TABLE media.schedule_slots (
  schedule text NOT NULL CHECK (schedule <> ''),
  slot_at timestamptz NOT NULL,
  attempts integer NOT NULL DEFAULT 0,
  claim_token uuid,
  claimed_until timestamptz,
  started_at timestamptz,
  finished_at timestamptz,
  outcome text,
  PRIMARY KEY (schedule, slot_at),
  CHECK ((finished_at IS NULL) = (outcome IS NULL))
);

REVOKE ALL ON SCHEMA media FROM PUBLIC;
