-- P5 D7: job feedback and the support request outbox. Both live in the account
-- lock domain and go with account erasure. Feedback references its job and goes
-- with a hard job deletion (source PRV-005 cascade); a support request keeps its
-- optional job id only as context, without a foreign key, and is deleted once
-- both of its mails were sent.

CREATE TABLE transgate_jobs.job_feedback (
  id uuid PRIMARY KEY,
  job_id bigint NOT NULL REFERENCES transgate_jobs.jobs (id) ON DELETE CASCADE,
  account_id uuid NOT NULL,
  rating smallint NOT NULL CHECK (rating IN (0, 5, 10)),
  comment text NOT NULL,
  created_at timestamptz NOT NULL
);

CREATE INDEX job_feedback_job ON transgate_jobs.job_feedback (job_id);
CREATE INDEX job_feedback_account ON transgate_jobs.job_feedback (account_id);

CREATE TABLE transgate_jobs.support_requests (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL,
  job_id bigint,
  message text NOT NULL,
  created_at timestamptz NOT NULL,
  attempts integer NOT NULL DEFAULT 0,
  claimed_until timestamptz,
  last_error text
);

CREATE INDEX support_requests_due ON transgate_jobs.support_requests (created_at, id);
CREATE INDEX support_requests_account ON transgate_jobs.support_requests (account_id);

-- Feedback rows are written once and never edited.
GRANT SELECT, INSERT, DELETE ON transgate_jobs.job_feedback TO billing_runtime;

GRANT SELECT, INSERT, DELETE ON transgate_jobs.support_requests TO billing_runtime;
GRANT UPDATE (attempts, claimed_until, last_error) ON transgate_jobs.support_requests TO billing_runtime;
