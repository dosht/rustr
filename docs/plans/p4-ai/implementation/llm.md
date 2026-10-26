# P4 shared LLM slice: source-ready handoff

Owner `w23:p21`, resumed 2026-10-25. **Lead execution:** initial `cargo test -p llm-core -p llm-http` passed 27 tests in 8 suites (artifact 599). The final shared run after the F1-F3 repairs passed **33 tests in 8 suites (artifact 666)**. Lead rebuilt the real `e4_capture` and reran E4: **81 offline loopback cases passed** (`target/p4-e4/run-ApXNGk`). This worker ran no build, test, formatter, live provider request or runtime smoke. The wasm target check and real shell/runtime acceptance are not covered here. Live provider/model/flag/retention acceptance remains a prerequisite; offline E4 fixtures do not prove it. Root Cargo manifests/lockfile, canonical ledgers, book and other owners' files were not edited. D4 bounds the 1 MiB decoded snapshot separately from its outer JSON escaping; provider and document limits are unchanged.

**Review repairs (p1V, `opus/transport-review.md`).** Each finding failed first under lead execution before its repair.
- **F1** (red artifact 635, green artifact 651: llm-core 12 passed): after `resume()`, a host control `Interrupted` frame passes the awaiting-snapshot gate like Final/Revoked. It is ordered against `durable_seq`: anything at or below the durable checkpoint is stale, and raw provisional sequence numbers are ignored. It keeps its true `PublicError`, text and durable cursor, and never moves `last_seq` backwards.
- **F2** (red artifact 636, green artifact 651: codecs 7 passed): the decoder ceiling is now `6*max(output_bytes, 256+4096)+4096`. Encoders refuse any JSON/SSE record the matching decoder would reject. Default D4 decoded/provider limits are unchanged. Native consumer: 7 passed (artifact 650).
- **F3** (red artifact 656, green artifact 666): `HttpCompletion::read_next` parses the remaining already-received chunk before checking the deadline. The explicit deadline check now sits immediately before each network fetch, and `timeout_at` still wraps the read. No retry or new read happens after expiry. Post-deadline work is bounded by the single buffered chunk. Deterministic private regressions are in `llm-http/src/provider/tests.rs` (one controlled chunk, paused Tokio time, native dev `tokio` `test-util`); there is no production clock/config abstraction. The real-HTTP guard `continuous_stream_still_times_out_and_reads_nothing_after_expiry` stays in `tests/transport.rs`.

## Files and dependency boundaries

- `crates/llm-core/{Cargo.toml,src/lib.rs,tests/stream.rs}`: std-only ports/types, text reducer and bounded attachment queue. No dependencies, including no host/billing/taskr-core dependency.
- `crates/llm-http/Cargo.toml`, `src/{lib,sse,application,provider,client}.rs`: portable codecs plus native provider/client implementations.
- `crates/llm-http/tests/{codecs,transport,client}.rs`: substantive parser/reducer, real loopback provider and native host-stream regression source.
- `crates/llm-http/examples/e4_capture.rs`: real-adapter loopback-only E4 executable coordinated with p24. E4 scripts/fixtures remain p24-owned.
- Both manifests and real lib roots are coherent; the earlier missing-manifest metadata hold is released. The parked pre-pause manifest is historical, not a workspace member.

`llm-http` uses existing workspace serde/serde_json. Native-only dependencies: reqwest 0.13 with rustls/json/stream, Tokio 1 sync/time/rt, bytes 1. Native-only dev dependencies: Axum 0.8, Tokio macros/rt-multi-thread/net/io-util/sync/time, futures-util 0.3. No new SDK, gateway or plugin framework. Lead alone resolves/updates root lockfile. wasm builds only the portable codecs; neither provider keys nor native Tokio/reqwest code is compiled there.

## Published real interfaces

All core types are in `crates/llm-core/src/lib.rs`, exported at crate root:

```text
Llm::open(CompletionRequest)
  -> BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>>
CompletionStream::next(&mut self)
  -> BoxFuture<'_, Result<Option<CompletionEvent>, LlmError>>
CompletionStream::cancel(&mut self) // synchronous, local transport only
```

`CompletionRequest::new(model, Vec<Message>)` initializes temperature 0.3, max_output_tokens 1000, timeout 60 seconds and D4 byte/depth defaults; `response_format` is explicitly `None` or `Some(ResponseFormat::JsonObject)`. Roles are typed System/User/Assistant. The product caller still chooses model/messages/response format.

Events are `Metadata { remote_id, usage }`, exact `Delta(String)`, and `Finished(CompletionFinish { reason, usage, remote_id })`. Only `FinishReason::Stop.is_success()` is true. Length/filter/tool/other reasons remain observable unsuccessful finishes. A Finished event requires finish metadata **and** `[DONE]`; plain EOF, malformed/partial records, invalid UTF-8 or unsupported structured/tool content is a typed error, not success. No provider auto-retry or fallback.

`LlmError { kind, remote, remote_id }` distinguishes configuration/input/capacity, HTTP status/retry-after, transport/deadline/cancel/protocol/limit. `RemoteOutcome` distinguishes not dispatched, HTTP rejected and unknown; local cancellation/timeout is never external completion/deletion evidence. Errors do not carry provider body, prompt, auth header or content.

Provider constructors, native only, root exports from `llm-http/src/provider.rs`:

```text
ProviderConfig::new(base_url, api_key)
DispatchCapacity::default() // ONE process-wide shared eight-slot semaphore
DispatchCapacity::new(maximum) -> Result<Self, LlmError> // explicit pool; clone to share
HttpLlm::deepseek(config, capacity) -> Result<Self, LlmError>
HttpLlm::openai(config, capacity) -> Result<Self, LlmError>
```

Pass DeepSeek `https://api.deepseek.com` or OpenAI `https://api.openai.com/v1` explicitly; constructors append `chat/completions`. HTTPS is required except literal loopback HTTP fixtures; credentials/query/fragment in provider URL are rejected. Redirects, proxy inheritance and reqwest retry policy are disabled. Dispatch refuses capacity immediately, without a hidden queue, and holds a permit until terminal/error/drop/cancel. Deadlines cover response headers and pull-stream consumption. DeepSeek sends store=false/ephemeral=true; OpenAI omits those flags. Model is never substituted.

## Application codec and presentation seam

`ApplicationFrame { identity: StreamIdentity { generation:String, content_revision:u64, consent_epoch:u64, execution:String }, seq:u64, event:TextEvent }`.

Text events: `Delta(String)`, `Snapshot { text,status,persistence }`, `Final { text,persistence }`, `Interrupted(PublicError)` and `Revoked`. No pending original question, transcript or prompt field exists in the public envelope. Host/product types and analysis sections stay outside this neutral codec.

Portable `llm-http` exports:

```text
encode_application_json(&ApplicationFrame, Limits) -> Result<String, CodecError>
decode_application_json(&[u8], Limits) -> Result<ApplicationFrame, CodecError>
encode_frame(&ApplicationFrame, Limits) -> Result<String, CodecError>
ApplicationDecoder::new(Limits)
ApplicationDecoder::push(&[u8]) -> Result<(consumed_bytes, Option<ApplicationFrame>), CodecError>
ApplicationDecoder::finish() -> Result<(), CodecError>
```

SSE is `event: text`, `id: generation:seq`, JSON data `{version:1,identity,seq,event:{type:...}}`. Loop over each chunk's unused suffix: push consumes at most one dispatched record; it does not allocate an event vector for a whole chunk. UTF-8 byte splits, LF/CRLF, comments and multiline data are handled. Invalid identity/event/version/extra question fields and false completed-unsaved snapshots are rejected.

Provider SSE record bound remains 256 KiB. Application cumulative snapshots may contain up to 1 MiB **decoded text**; their JSON envelope has a separate bounded worst-case escape allowance (`6*output_bytes+4096`) so escaped 1 MiB snapshots remain representable. This does not raise the provider/document or decoded-output limit. Host and shell were told this distinction.

`StreamState::new(output_bytes)/default`, `start(identity)`, `update(frame) -> Result<StreamUpdate, StreamError>`, borrowed `view() -> StreamView<'_>`, `resume/detach/cancel/clear`. `view` does not clone the entire accumulated text per token. Release consumer locks before listeners.

Any sanitized Snapshot switches that attachment one-way to replacement mode. Later raw Delta requests resnapshot rather than corrupting the prefix. `resume()` allows a durable checkpoint to replace an uncommitted suffix; final replaces either mode. Identity/epoch/revision/execution mismatches and duplicate/old frames are ignored; delta gaps request resnapshot. Unsaved final remains visible but not Completed; later saved final can reconcile it. Cancel does not erase an already completed saved answer. Interrupted streams do not accept late raw deltas without explicit resume.

`AttachmentQueue` is one bounded attachment, not a dispatcher/log: bounded delta memory (including envelope/string allocation accounting), one replaceable cumulative snapshot, raw queue discarded on replacement, slow-consumer detach. Host supplies account/consent/erasure authorization before each publication; these crates do not pretend to enforce product permissions.

Native root exports `ApplicationClient::new(Limits, Duration)`, async `open(full_watch_url, Option<&str> bearer) -> ApplicationStream`, async `next() -> Result<Option<ApplicationFrame>, ClientError>`, sync `cancel()`. Only numeric after/after_seq query parameters are accepted; bearer goes in headers, redirects/retries disabled. This attaches only, never starts generation or calls host cancellation. wasm shell supplies fetch ReadableStream/bytes_stream bytes to the same portable decoder; no full-body text buffering. Shell p22 confirmed use of both paths.

## E4 executable contract

Lead command, **not executed here**:

```sh
cargo run -p llm-http --example e4_capture -- input.json output.json
```

Input exactly: `{provider:"deepseek"|"openai",base_url,model,messages:[{role,content}],response_format:"json_object"|null,api_key}`. Example accepts literal loopback HTTP only and a synthetic `e4-` credential, never a live provider. Source temperature/output cap flow through the real adapter.

Output: `{content,terminal:bool,finish_reason:string|null,error:null|{kind,remote,status,retry_after}}`. Delivered partial content survives errors. Non-stop finish can have terminal=true/error=null; consumers inspect finish_reason. Unexplained EOF/malformed SSE has terminal=false and error. Exit 0 means report saved, not provider success; invalid input or failed report write exits nonzero. No prompt/provider diagnostics are printed.

P24 was notified of this real CLI. Exact old request with json_object versus omitted-mode analysis correction remains p24's separate experiment; this example neither changes prompts nor invents an SDK comparison.

## Regression source and lead commands

```sh
cargo test -p llm-core --test stream
cargo test -p llm-http --test codecs
cargo test -p llm-http --test transport
cargo test -p llm-http --test client
cargo check -p llm-core -p llm-http --target wasm32-unknown-unknown --lib
```

Use this machine's configured wasm toolchain/build-std path if required; native examples/tests are intentionally native. These are proposed lead commands, not passed results.

- Core: real state/queue transitions for raw Hello-space -> sanitized Hello -> future world, cumulative redaction replacement, gaps/duplicates, lost suffix, stale epochs/workers, revoke/late frames, unsaved->saved reconciliation, output pressure, queue coalescing/detach, interrupted late deltas and completed cancellation.
- Codecs: one-byte UTF-8/CRLF/multiline records, invalid/truncated/flood inputs, actual encode/decode plus reconnect reducer, envelope privacy/identity/false-success rejection, escaped large snapshot versus decoded bound, depth attack.
- Real loopback upstream: explicit response mode/provider flags, partial before terminal, capacity refusal without dispatch, cancel releases slot, partial retained across EOF/malformed/unknown shape, non-stop finishes, usage, HTTP 429/no retry, total deadline, record/output/depth bounds and input refusal before dispatch. Tests use explicit isolated capacity pools rather than contending on the process default.
- Native host client: same-generation watch/reconnect through real HTTP, authoritative replacement, local cancel, EOF-not-success, header-only credentials and no redirected credential forwarding.

These support AI-003/005/020 transport and finalization, D1/D4/D7, streaming scenario 10 and the E4 transport boundary. Product parser/count/quota/consent/PG behavior and real browser/UniFFI acceptance remain their owners' independent evidence.

## Review limits and next owner

Source review covered typed errors, partial/EOF/deadline/cancel behavior, no raw body diagnostics, platform dependency separation, no provider retry, explicit output/queue bounds and no hot-path full-history copy. No runtime verification is claimed. Lead must run the coherent targets and E4 harness, then report concrete diagnostics here for repair before acceptance. No worker-owned services/processes/containers are running.

Signatures were sent directly to lead p1E, host p1T, policy p1X, shell p22 and E4 p24 before dependent wiring. No external API shape/model retention guarantee was invented. Provider/model/flags/quality/cost/region/retention, true remote cancellation and actual shell/runtime acceptance remain explicit live/integration prerequisites. `deepseek-chat` is preserved despite current documentation's different advertised model identifiers.
