-- D3/D9 LLM dispatch ledger: one payload-free row per generation that may have
-- reached a provider, written in the claim transaction before any provider
-- I/O. It has no foreign key to jobs or generations, so a consent DELETE purge,
-- a hard job deletion or a content deletion cannot lose it; only account erasure
-- removes it, after the erasure manifest copied it. No prompt, question or
-- transcript is stored, only identifiers and the disposition.

CREATE TABLE transgate_jobs.llm_dispatches (
  generation_id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  provider text NOT NULL,
  model text NOT NULL,
  disposition text NOT NULL CHECK (disposition IN ('possible', 'not_dispatched')),
  -- Distinct observed provider ids in first-seen order; the bound is
  -- ai::ports::MAX_DISPATCH_REMOTE_IDS.
  remote_ids text[] NOT NULL DEFAULT '{}' CHECK (cardinality(remote_ids) <= 8),
  created_at timestamptz NOT NULL,
  -- Evidence is never cleared: a row with an observed id is always possible.
  CHECK (disposition = 'possible' OR cardinality(remote_ids) = 0)
);

CREATE INDEX llm_dispatches_account ON transgate_jobs.llm_dispatches (account_id);

GRANT SELECT, INSERT, DELETE ON transgate_jobs.llm_dispatches TO billing_runtime;
GRANT UPDATE (disposition, remote_ids) ON transgate_jobs.llm_dispatches TO billing_runtime;

-- Late LLM dispatch reconciliation after the local cascade rewrites a manifest
-- item's target; the adapter restricts it to llm_dispatch items on both sides.
GRANT UPDATE (target) ON transgate_jobs.erasure_items TO billing_runtime;
