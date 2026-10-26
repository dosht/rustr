-- Provider request identities become a bounded, distinct, first-seen-first list (M1): the response
-- header id and the body completion id are different identities and neither may overwrite the
-- other. 0002-era rows stored one scalar `remote_id`; convert it so no known identity is lost. An
-- already-known identity is kept as-is even if it is longer than the new per-id bound for new ids.
UPDATE taskr_work.summary_generations
SET payload = (payload - 'remote_id') || jsonb_build_object('remote_ids',
    CASE WHEN jsonb_typeof(payload->'remote_id') = 'string' AND payload->>'remote_id' <> ''
         THEN jsonb_build_array(payload->'remote_id') ELSE '[]'::jsonb END)
WHERE NOT payload ? 'remote_ids';

UPDATE taskr_work.erasure_resources
SET target = (target - 'remote_id') || jsonb_build_object('remote_ids',
    CASE WHEN jsonb_typeof(target->'remote_id') = 'string' AND target->>'remote_id' <> ''
         THEN jsonb_build_array(target->'remote_id') ELSE '[]'::jsonb END)
WHERE kind = 'llm' AND NOT target ? 'remote_ids';
