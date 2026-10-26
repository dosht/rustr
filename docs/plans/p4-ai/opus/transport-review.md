# P4 shared transport review (Opus, read-only)

Reviewer `w23:p1V`, original saved session, 2026-10-25. Scope: `crates/llm-core` and `crates/llm-http` implementation and their substantive tests, read against `decisions.md` D1-D8, `implementation-contract.md` and `implementation/llm.md`. Host and shell code was read only to decide whether a reducer/codec counterexample is reachable. No product edits, builds, tests or provider calls. The lead's 27-test pass is not re-run or re-proved here. Product quota, consent and PostgreSQL audit are out of scope.

Every schedule below is **[INFERENCE: source schedule, not executed]**. p21 (owner) confirmed all three as unintended defects on 2026-10-25. Fixes await lead go-ahead; p21 makes no change on advisory alone.

## Finding 1 — P2: a resumed attachment does not apply control `Interrupted` and masks its error

`llm-core/src/lib.rs:205` returns `ResnapshotRequired` for any frame other than Snapshot/Final/Revoked while `awaiting_snapshot`. `Final` and `Revoked` pass this gate; `Interrupted` does not. The frame is neither applied nor retained.

Reachable through published hosts:

1. `taskr-client/src/audio_summary.rs:352` calls `stream.resume()` before every watch, so each attachment starts with `awaiting_snapshot = true`.
2. On the first body pull, `taskr-work/src/summary/http.rs:103-112` authenticates before reading the generation. `AuthError::Unavailable` yields `Interrupted(identity_unavailable)` at `sequence + 1` as the **first** frame. A first-pull read failure similarly yields `Interrupted(unavailable)` (`:127-130`). Transgate's shared watch has the same auth/read ordering (`transgate/http/src/ai.rs:226-238`).
3. The reducer returns `ResnapshotRequired`. `audio_summary.rs:414-415` returns before the transient branch at `:419` and reports `code: "resnapshot"`, "A fresh snapshot is required", instead of `identity_unavailable`/`unavailable`. The view keeps its pre-resume status and has no error.

Effect: a transient sign-in or store outage appears as a protocol resnapshot condition. The real reason is discarded. Any consumer that resumes on `resnapshot` retries against the same outage. Taskr's resume-on-error behaviour was not traced.

Owner-confirmed correction: let `Interrupted` pass the awaiting gate like Final/Revoked and apply status plus `PublicError` without replacing text. p21 also identified a second drop at `lib.rs:208`: if the client's `last_seq` from raw deltas exceeds the durable sequence, a control frame at `durable + 1` is still ignored. The stale check for this path must compare against `durable_seq`, as the Snapshot path does at `:211`, not `last_seq`.

Named regressions:
- `llm-core/tests/stream.rs::resumed_attachment_applies_control_interruption_without_text_change`: start, apply delta, `resume()`, apply `Interrupted{identity_unavailable}` at `last+1`. Expect status Interrupted, preserved error code and unchanged text. Repeat with raw `last_seq` greater than the durable seq and the control frame at `durable + 1`.
- A taskr-client watch test whose first frame is `identity_unavailable`, expecting that error code rather than `resnapshot`.

## Finding 2 — P3: encoder can produce application frames the decoder rejects (owner-confirmed)

`application.rs:52` allows an `Interrupted` message up to 4096 bytes and a code up to 256 bytes. Encoding (`:62-72`) never checks the encoded size. `decode_application_json` rejects frames larger than `6 * output_bytes + 4096` (`:47,75`). JSON escapes each control byte as six bytes, so a 4096-byte control-character message encodes to about 24.6 KiB.

With `Limits { output_bytes: 1024, .. }`, encoding succeeds and decoding the result returns `Limit`. This is **not reachable with D4 defaults**: every reviewed host uses `Limits::default()`. It is a smaller-limit configuration trap, not a current runtime defect.

Owner-confirmed correction: `wire_limit = 6 * max(output_bytes, 256 + 4096) + 4096`. It covers escaped code and message. Identity strings reject control characters, so they escape at most 2x; together with the SSE `id`/`event` lines, they fit the 4096-byte allowance. D4 defaults are unchanged.

Named regression: `llm-http/tests/codecs.rs::every_encodable_frame_decodes_under_the_same_limits`. Use small `output_bytes`, a maximum control-character `Interrupted` message and maximum-length quoted identity strings, then round-trip.

## Finding 3 — P2: the deadline discards an already-received terminal answer (owner-confirmed)

`provider.rs:222` checks the deadline before parsing bytes already in `self.chunk`. Suppose a provider tail containing the last delta, finish and `[DONE]` arrives in one chunk before 60 seconds. If the consumer spends time between `next()` calls — Transgate runs a checkpoint transaction there (`transgate/core/src/ai/app.rs:906-915`) — it receives `Timeout`/`Unknown`. A real provider completion is lost, though no false success is produced. p21 confirmed that D4 bounds provider waiting and new network reads, not bytes already received.

Owner-confirmed correction: check the deadline only before fetching more network bytes. Bytes still in the reqwest buffer and not yet pulled still time out. Keep the explicit pre-fetch check: tokio's `timeout_at` polls the inner future first, so a continuously streaming provider would otherwise never time out.

Named regressions:
- `llm-http/tests/transport.rs::buffered_terminal_tail_is_not_discarded_by_consumer_latency`: deliver delta, finish and `[DONE]` in one chunk; sleep past the deadline after the first event. Expect the remaining delta and `Finished`.
- `continuous_stream_still_times_out`: send keepalives or deltas faster than the deadline and expect `Timeout`.

## Checked without counterexample

These are source-review results only, not runtime proof.

- **Terminal/EOF/partial:** `Finished` requires a finish reason plus `[DONE]`; EOF, truncated or malformed records, invalid UTF-8, multiple choices, tool/refusal/reasoning content and content after finish are typed errors. Delivered partials remain with the consumer.
- **Permits:** acquired before dispatch with no queue; released on HTTP error, protocol/timeout failure, `[DONE]`, cancel or drop. `next()` awaits only `response.chunk()`, so dropping it on a cancellation branch loses no consumed record. Quiet upstreams and comment-only keepalives hit `timeout_at` or the loop-top deadline.
- **No retries or credential leaks:** provider and watch clients disable redirects, retries, proxy inheritance and Referer. URL credentials, fragments and provider queries are rejected; watch queries allow only numeric `after`/`after_seq`. Errors carry no body or prompt. API keys with control characters are rejected.
- **Queue and resnapshot:** delta accounting includes envelope storage; there is one replaceable snapshot; replacement discards raw deltas and switches one-way. Hosts emit durable-only HTTP frames, so a sanitized prefix cannot gain a raw suffix over HTTP.
- **Durable cursor:** taskr-client resumes from `durable_seq`, which control frames do not advance. Both hosts reject a cursor ahead of the saved sequence. Snapshot/checkpoint/final sequences increase monotonically (`generation.rs:137,162,177,194`).
- **Identity:** the reducer requires an exact generation/revision/epoch/execution match. Execution is assigned only by claim/checkpoint for the same token, so legitimate recovery does not strand an attachment on a changed identity.

Out of scope, noted for the lead: Transgate's `llm.open(request).await` (`app.rs:836`) is not raced with fence probes. User cancellation or revocation while awaiting response headers is observed only after headers or the 60-second deadline. This belongs to the policy owner, not the transport crates.
