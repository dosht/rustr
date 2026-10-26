-- One bounded current snapshot/staged final per request, not a token log. All runtime operations
-- take the existing taskr account lock and barrier before touching these rows. Foreign keys make
-- the existing work/account cascade erase every summary child in the same transaction.
CREATE TABLE taskr_work.summary_generations (
    id uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES taskr_work.enrollments(account_id) ON DELETE CASCADE,
    work_id uuid NOT NULL REFERENCES taskr_work.works(id) ON DELETE CASCADE,
    request_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    source_revision bigint NOT NULL,
    payload jsonb NOT NULL,
    created_us bigint NOT NULL,
    lease_until_us bigint NOT NULL,
    UNIQUE(account_id, request_id),
    CHECK (payload->>'status' IN ('accepted', 'running', 'completed', 'interrupted', 'cancelled'))
);
CREATE INDEX summary_generations_latest ON taskr_work.summary_generations(work_id, created_us DESC, id);
CREATE UNIQUE INDEX summary_one_active_per_work ON taskr_work.summary_generations(work_id)
    WHERE payload->>'status' IN ('accepted', 'running');
CREATE INDEX summary_generations_recovery ON taskr_work.summary_generations(lease_until_us)
    WHERE payload->>'status' IN ('accepted', 'running');

CREATE TABLE taskr_work.summaries (
    work_id uuid PRIMARY KEY REFERENCES taskr_work.works(id) ON DELETE CASCADE,
    generation_id uuid NOT NULL REFERENCES taskr_work.summary_generations(id) ON DELETE CASCADE,
    account_id uuid NOT NULL REFERENCES taskr_work.enrollments(account_id) ON DELETE CASCADE,
    payload jsonb NOT NULL
);

-- LLM dispatch may retain data even after a successful completion. Taskr has no provider-delete
-- proof or operator exception API: retain only pseudonymous request facts, explicitly unresolved.
ALTER TABLE taskr_work.erasure_resources DROP CONSTRAINT erasure_resources_kind_check;
ALTER TABLE taskr_work.erasure_resources ADD CONSTRAINT erasure_resources_kind_check
    CHECK (kind IN ('object', 'remote', 'unidentified', 'llm'));
