-- Synthetic completed audio work for P4 browser/wasm/UniFFI evidence, NEVER production.
-- First enroll the signed-in owner via POST /api/audio-work/enroll. The account UUID must be
-- that verified session's account. This fixture issues no upload URL or provider request.
-- psql "$OWNED_TEST_DATABASE_URL" -v ON_ERROR_STOP=1 \
--   -v account_id=<enrolled-session-uuid> -v work_id=<fresh-work-uuid> \
--   -f scripts/p4-fixtures/taskr-completed.sql
BEGIN;
SELECT pg_advisory_xact_lock(hashtextextended('taskr_work:' || :'account_id', 0));
INSERT INTO taskr_work.works
    (id, account_id, object, name, language, requested, status, revision, transcript, error, created_us, completed_us)
SELECT :'work_id'::uuid, e.account_id,
       jsonb_build_object('store', 'gcs', 'bucket', 'p4-fixture-audio',
                          'key', 'work/' || e.account_id || '/' || :'work_id' || '/planning.txt',
                          'version', 'synthetic-fixture-v1'),
       'Planning meeting', 'en', NULL, 'completed', 1,
       'We reviewed the launch plan. Ada will finish accessibility review on Monday. Sam will prepare the release notes. The team will meet again on Friday to decide whether to ship.',
       NULL, 1792886400000000, 1792886460000000
FROM taskr_work.enrollments e
WHERE e.account_id = :'account_id'::uuid
  AND NOT EXISTS (SELECT 1 FROM taskr_work.erasures x WHERE x.account_id = e.account_id)
ON CONFLICT (id) DO NOTHING;
SELECT id, account_id, status, revision, transcript FROM taskr_work.works
WHERE id = :'work_id'::uuid AND account_id = :'account_id'::uuid;
COMMIT;
