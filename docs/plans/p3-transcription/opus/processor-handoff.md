# P3 free-tools processor: handoff

Owner: processor slice (`w23:p1W`). Opus authored the original implementation/evidence; Sol resumed the same saved session and completed the bounded backend acceptance expansion and approved cancellation/reaping repair. **Lead verification on 2026-10-25: `cargo test --locked -p transgate-processor` passed 53 tests across four suites, artifact 562.** The original 40-pass run (artifact 400) is historical; **artifact 544** retains the invalidation failing-before capture. I ran no build, test or formatter. Source is on hold for targeted lead-reported lint/runtime errors only. Product container proof and `transgate/processor/deploy/` remain owned by P3 integration owner `w23:p1Y`, now authorized for the final repaired processor image/runtime; no final-image result is inferred from the test run. P4 implementation remains with separate owners.

## Packages and registration

| Package | Path | Kind | Notes |
|---|---|---|---|
| `transgate-processor` | `transgate/processor` | lib `transgate_processor` + bin `transgate-processor` | New; **needs a root workspace member** |
| `transgate-core` | `transgate/core` | lib | Core owner's package; I own `src/tools.rs` only (std-only, no new deps). `pub mod tools;` is already declared |

Processor dependencies: axum 0.8, base64 0.22, bytes, crc32fast, flate2 (`default-features = false`, `zlib-rs`, the same pure-Rust backend transgate-export uses), futures-core, http-body-util, serde, serde_json, tokio (+process, fs, time, io-util), tokio-util (`io`) and uuid. The dev-dependency is reqwest 0.13 with no default features. flate2, crc32fast and zlib-rs need the lead's lockfile update; the rest are already locked. No multipart or zip crate is used (see the ZIP assessment).

No Next route proxies the processor: the browser calls `NEXT_PUBLIC_FREE_TOOLS_PROCESSOR_URL` directly, and `frontend/src/pages/api/free-tools` does not exist. So there is no `transgate/tools-http` facade, and the host needs no change. The host composition API, if one is ever wanted, is `Config::from_env()`, `Processor::new(Config) -> Arc<Processor>`, `processor.router()` (all 40 routes plus CORS; serve it with connect info) and `processor.spawn_cleanup()`.

The existing `transgate/Dockerfile` and `deploy/dev/transgate.compose.yaml` are untouched.

## Coverage

- Parity rows API-F45..API-F84: `GET /health` plus the 39 tool endpoints listed in `transgate/processor/README.md`.
- Rules API-022..API-030, RTL-006..RTL-008, CRN-008, CRN-009 and FLG-008 (the processor side of server-first).
- Deployment constraints OPS-D06 and API-D10, kept as captured: one instance, process-local sessions and allowances.

Files and what they cover:

| File | Covers |
|---|---|
| `transgate/core/src/tools.rs` | Per family: mounts, prefixes, TTLs, size limits, allowances and kill ceilings. Upload acceptance in each family's check order. Safe names (UTF-16 replacement, Node `extname`/`basename`). UUID v4 ids. The fixed-window allowance. Refusal messages. Every option validator and preset (convert/reduce/denoise/extract/compress/convert-video). Trim copy-versus-re-encode and the B-frame seam. Progress math. JS `Math.round` |
| `transgate/processor/src/families/*.rs` | The seven routers with the source's exact status codes, messages and JSON shapes, including the four progress shapes; detached jobs; 202/409/410/425 |
| `src/upload.rs` | Streaming formidable-equivalent multipart: the boundary must open the body; `CRLF--boundary` ends a part only when CRLF or `--` follows, otherwise the bytes are payload; the last `file` part wins; every received file byte counts against the limit; a part with no `Content-Type` is a field |
| `src/zip.rs` | Stored two-entry ZIP: crc32fast CRCs first, then streamed from disk with an exact length |
| `src/http.rs` | CORS (`cors` 2.8 semantics); `express.json()` as body-parser 1.20 (identity/gzip/deflate, 100 kb limit on decoded bytes, `utf-` charsets with BOM stripped); qs query parsing; the Express 404; `getClientIp`. Known-length bodies fire completion with their final byte |
| `src/media.rs` | FFmpeg/ffprobe children: no shell, kill and reap on timeout, overflow or cancellation, SIGKILL ceilings. `output_pair` kills and reaps the sibling as soon as one part fails |
| `src/lib.rs` | Dispatch (case-insensitive, one trailing slash, HEAD answered by GET), the ten-minute purge, and `Config`: an unset PORT means 8080, an invalid one is a `ConfigError` and the binary exits 2 |
| `deploy/` | Dockerfile (Debian ffmpeg, UID 65532, curl healthcheck), dockerignore, compose (Caddy edge plus one processor, 1.5 CPU / 2 GiB, `/tmp` volume, 30 s stop grace), Caddyfile, env.example |

## Tests

Commands:

```sh
cargo test -p transgate-core tools::
cargo test --locked -p transgate-processor
```

The second needs `ffmpeg`, `ffprobe` and `unzip` on PATH and loopback networking only. The media is generated per test with lavfi sine and testsrc; no binary fixture is checked in and nothing leaves the machine.

- **Core (`transgate/core/src/tools.rs` tests):** family table; exact-limit size boundary; extension-versus-MIME precedence per family; format-versus-size order; safe names; session ids; fixed allowance window; trim shape and range (including NaN duration); keyframe copy and seam (FT-QA-04/05 ported); trim progress; convert-audio order and opus 48k; reduce presets; stream progress; denoise; extraction; compression scale and maxrate; convert-video validation order and OOM message; `js_round`.
- **Processor unit tests:** JS number parsing and ordered JSON; civil time; `-progress` parsing; a real FFmpeg child killed at its ceiling; `a_failing_sibling_kills_and_reaps_the_other_command_promptly` (a real endless FFmpeg is gone when the pair returns); spawn versus exit failure; multipart over 1/7/64-byte chunks (last `file` wins, nothing left on disk); the limit plus one byte; truncated and unknown bodies; `delimiter_lookalikes_inside_file_bytes_are_payload_wherever_the_chunks_split` (and a preamble is refused); disposition parsing; `unzip -t` of a stored ZIP; `completion_fires_with_the_final_byte_not_a_later_end_of_stream_poll` (also for an empty body); client IP from the right; repeated and bracketed query keys; PORT refusals; `probed_metadata_decides_the_copy_boundary_and_its_seam_correction` (ffprobe packet and seam rows to the cut: usable B-frame metadata backs part 1 off; swapped, unreadable or missing metadata copies uncorrected; off-keyframe re-encodes); `waveform_peaks_are_absolute_maxima_of_equal_blocks_capped_at_one_however_the_bytes_arrive`; v2a, compress and convert-video ffprobe parsing.
- **Processor unit tests added in the p1S round:** `a_child_that_closes_its_pipes_cannot_outlive_the_deadline_or_a_failed_sibling`, `a_file_shorter_than_announced_fails_the_body_without_completing_it`, `a_compressed_frame_cannot_expand_past_the_decoded_limit` (a 50 MiB single gzip frame; exactly 100 kb accepted) and `json_charsets_decode_as_the_installed_iconv_lite_does` (RFC 2152/3501 vectors, UTF-16 BOM and heuristic, odd byte, `:NNNN`, no UTF-32). The waveform test feeds 1- and 3-byte pieces, which exercises the carry fix.
- **`transgate/processor/tests/processor.rs`:**
  - `all_forty_routes_are_mounted_and_anything_else_is_the_express_404`
  - `json_bodies_are_read_through_gzip_deflate_and_utf16_like_express` (gzip and deflate inflated; UTF-16LE read; a 150 kb gzip body under 2 kb refused; br and latin1 refused; non-JSON left empty)
  - `cors_reflects_only_listed_origins_and_answers_every_preflight`
  - `trim_audio_cuts_two_parts_serves_them_singly_and_as_a_zip_then_cleans_up`
  - `trim_video_reencodes_off_a_keyframe_and_copies_on_one_without_duplicate_frames`
  - `convert_audio_answers_when_done_forces_opus_to_48k_and_cleans_up_after_download`
  - `reduce_denoise_and_extract_run_detached_and_poll_to_their_outputs`
  - `compress_and_convert_video_report_results_and_refuse_a_second_concurrent_job`
  - `the_processing_allowance_is_per_family_and_address_and_charges_refused_options` (RTL-008)
  - `session_ids_that_are_not_server_minted_never_reach_the_filesystem`
  - `cleanup_purges_sessions_past_their_ttl_and_orphans_by_age_and_prefix` (CRN-009, driven through `Processor::purge(now)`)
  - `the_deployable_binary_loses_sessions_on_restart_keeps_their_files_and_stops_on_sigterm` (actual binary, OPS-D06)
  - `the_binary_refuses_to_start_on_an_invalid_port_instead_of_moving_to_8080` (exit code 2 with the message)

The HTTP/1 download-then-cleanup checks (`wait_until_no_files`) in the trim, convert-audio, audio and video scenarios are the real-transport regression for final-byte completion.

### Verified backend expansion (lead artifact 562)

The 13 additional private `acceptance::` tests in `transgate/processor/tests/processor/acceptance.rs` passed with the integrated suite. Exact test selectors, rule/parity mappings and limits are in [processor-rule-evidence.md §6](processor-rule-evidence.md:146).

- All seven families: valid-session invalidation removes owned uploads/completed outputs and expires subsequent lookups without deleting another session's files.
- Active invalidation: concurrent DELETEs return only after owned children are cancelled/reaped, no late output is published, and another active session remains alive and completes normally. Convert-video's background thumbnail has its own cancellation regression.
- All-family 20/10 quotas: uploads uncharged, invalid options and absent-session checks charged. Six asynchronous families additionally prove busy checks charge without dispatching another worker; synchronous convert-audio has no captured busy-state contract.
- All-family session/orphan TTL behavior, exact orphan-mtime boundary and deterministic ten-minute callback using dev-only Tokio `test-util`; no wall ten-minute sleep or production clock hook.
- Representative real codec/container/options for API-024..028, exact terminal success/100, and real metadata/output assertions. No redundant Cartesian matrix or seven 2 GiB uploads.
- Corrected ffprobe packet counting now uses `-count_packets` and requires parsing; the previous zero fallback could make the old no-duplicate-frame check tautological. Artifact 562 covers the corrected H05 assertion and copied-video hashes; artifact 400 must not be credited for that packet-count proof.

The approved source repair registers per-session processing/thumbnail workers, fences dispatch/publication on close, cancels/reaps before cleanup completion, and removes known plus future session-owned paths. Cleanup survives a dropped request and concurrent callers share its completion. Shared cleanup also removes superseded same-id outputs; the earlier MP3 orphan expectation after Opus download is intentionally gone. HTTP options/status/quota semantics otherwise remain unchanged. The lead's saved multipart/waveform/UTF16/error-representation lint changes were preserved.

The lead's artifact-562 run passed the corrected trim-video packet-count/seam assertions and concurrent-processing scenarios. FFmpeg-version-specific or final-image runtime differences, if reported, remain targeted follow-up work rather than grounds for a new audit.

## Environment

`PORT`, `ALLOWED_ORIGINS`, `REQUEST_TIMEOUT_MS`, `FREE_TOOLS_TRUSTED_PROXY_COUNT`, `FFMPEG_THREADS`, `FFMPEG_PATH`, `FFPROBE_PATH`, `TMPDIR`.

The only one the source lacks is `TMPDIR`, the standard temp directory. The source hard-coded `/tmp`; the default is unchanged.

## Differences from source

The lead has accepted these:

1. **Octet-stream uploads.** formidable's whole-body octet-stream upload (`X-File-Name`) had no size limit; it now gets the family limit. Formidable's json and urlencoded upload bodies answer 400 `No file provided`.
2. **Temporary files.** Temporary upload and waveform files carry the family prefix, so a crash mid-upload is purged. formidable's random `/tmp` names never were.
3. **Download failures.** A stat or open failure after the existence check answers 500 JSON. In the source the async rejection left the request hanging.
4. **ZIP marking.** The ZIP is marked downloaded when its final byte is read, where the source marked it as soon as the response was sent. An archive over 4 GiB answers the source's ZIP 500; JSZip/Buffer could not produce one either.

Repairs from this round:

5. **Trim failure.** When either FFmpeg part fails, the other is killed and reaped at once. The source left it running to its end.
6. **Invalid PORT.** A set but invalid PORT (empty, non-digit, 0, over 65535) exits 2 with a clear error. The source silently used 8080; an unset PORT still defaults to 8080.
7. **Waveform memory.** The trim waveform streams the raw decode with O(chunk) memory. The source held the whole raw Buffer, about 1.15 GB for 20 h.
8. **Multipart delimiters.** A `CRLF--boundary` lookalike inside the payload is data, as formidable reads it. The first cut rejected such bodies.
9. **Download completion.** Completion no longer depends on hyper polling a known-length body past its final byte. A file shorter (or longer) than its announced length is a body error and never counts as downloaded, so nothing is cleaned up.

p1S review fixes:

10. **Waveform carry.** A partial f32 sample carried across pieces is kept until it completes.
11. **Child deadline and cancellation.** Both cover the reap as well as the pipe reads, so a child that closes its pipes and stays alive is still stopped.
12. **Bounded decode.** The decoded-size bound is enforced inside the decoder's output sink, including `finish`, so no compressed frame expands past 100 kb first.

**JSON charsets.** These follow the installed source: processor package-lock `iconv-lite` 0.4.24, read in `frontend/node_modules/iconv-lite` 0.4.24 at `encodings/{index,internal,utf16,utf7}.js`. body-parser passes only `utf-` charsets; iconv-lite canonicalizes the name (lower-case, strip a `:NNNN` suffix and non-alphanumerics). Within that, 0.4.24 decodes utf-8, utf-16le, utf-16be, utf-16 (BOM, else the first-64-byte ASCII heuristic, else LE), utf-7 and utf-7-imap, all ported with BOM stripping. It has no `utf32.js`, so utf-32 answers the source's 415, which is a 500 here. The one remaining difference: a lone UTF-16 surrogate, which a JS string keeps, becomes U+FFFD.

**Framework metadata, not ported (lead decision).** Express's `X-Powered-By` and its default weak ETags on `res.json`/`res.send` are omitted. Range behavior is preserved as found (see below).

Expired allowance windows are pruned on cleanup, which is equivalent to a reset.

## ZIP: maintained streaming writer versus exact length

zip 9.1's `ZipWriter::new_stream` writes to a non-seekable `Write`. Its stored entries therefore get data descriptors: bit 3 set, CRC and sizes zero in the local header and written after the data. Using it here would mean:

- **Content-Length.** The source sends an exact `Content-Length`, and the archive is a JSZip STORE archive with sizes and CRCs in the local headers and no descriptors. Knowing the exact length up front would mean re-deriving the crate's internal layout (headers, extra fields, 16- or 24-byte descriptors). A crate update that changes it would abort responses mid-body. The clean option is chunked transfer with no Content-Length.
- **Readers.** Stored entries with descriptors are not readable by some streaming unzippers. java.util.zip.ZipInputStream refuses them, for example; desktop tools that read the central directory are fine.
- **Sync bridge.** The writer is a sync `Write`, so it needs a spawn_blocking writer and a bounded channel into the async body. That keeps bounded memory and no temporary archive, but it is more machinery than the layout it replaces.
- **No temporary copy.** A seekable `ZipWriter` would restore sizes in the headers, but it needs a temporary archive file: the extra copy the brief excludes.

The browser client never reads Content-Length (`response.blob()` in `trimAudioServer.ts:134`), so dropping it is client-compatible. It does change the HTTP contract and the archive format.

**Decision (lead):** keep the bounded stored two-entry layout, with crc32fast for the CRC. It keeps the exact Content-Length and the local-header compatibility without a temporary archive. `unzip -t` covers it in a unit test and over HTTP.

## Range and conditional requests (source checked)

- **File downloads and thumbnails.** Every source download and thumbnail handler sets Content-Type, Content-Length and (for downloads) Content-Disposition by hand, then `fs.createReadStream(path).pipe(res)`. None uses `res.sendFile`, so there is no Accept-Ranges, no Range handling (a Range header gets the full 200), no ETag, no Last-Modified and no 304. The port does the same.
- **ZIP and JSON.** Only `res.send(zipBuffer)` (the ZIP) and `res.json` (all JSON) get Express's default weak ETag, and with a matching `If-None-Match` on GET/HEAD they answer 304. For the ZIP this is unreachable in practice: the session is cleaned up when the ZIP is sent, so a revalidation gets 410. Hashing the archive would also cost a full extra read. For JSON polling, a 304 is transparent to `fetch()`.
- **Decision (lead):** the ETags are framework metadata and are omitted; Range behavior (none) is preserved.

## Open / unverified

- **Accepted as captured (lead):** a WAV trim's part 1 is stream-copied and ends on the first input-packet boundary after the cut, about 93 ms at most. PCM re-encoding is out of scope.
- **Other phases / explicit limits:** live deployment and the P5/P6 rendered tool pages; shared-limiter and multi-replica/session-affinity behavior (API-D10/OPS-D06); full permission/client-abort/resource-ceiling matrices and actual allowance reset through binary restart. Y's earlier Docker/non-root/read-only/FFmpeg-health PASS is packaging evidence for its recorded snapshot; p1Y owns the authorized final repaired processor image/runtime proof. No new result is claimed here.
