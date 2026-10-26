# P0 research answers

Research date: 2026-10-24. This document answers PRD R1 through R5 from the sourced topic inventories and the prior lead's read-only production probe. Source declarations, production observations and unverified external behavior are distinguished below. These answers do not claim that the rewrite or provider adapters already exist.

## R1: provider languages and dropping RunPod

The current application offers 61 transcription languages. Its capability declarations give all 61 to Deepgram and RunPod. Soniox and the explicitly selected Soniox-v5 path support 53, excluding Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek and Luxembourgish. Translation uses Soniox one-way translation and the same 53-language set.

**No offered language is RunPod-only according to the application's routing contract.** Removing RunPod leaves the eight Soniox exclusions dependent on Deepgram. This is not a vendor guarantee or a live transcription smoke test. Keep all offered languages and verify the eight-language route with recorded fixtures and authorized provider smoke before cutover. Removing RunPod also removes fallback capacity and provider-specific options; language-set equality alone is not complete feature parity.

Models in the current source: Soniox `stt-async-v3`, explicit Soniox-v5 `stt-async-v5`, Deepgram `whisper-large` with one hardcoded user's `nova-2` override, RunPod `large-v3`. Production reports a Soniox duration threshold of 200 minutes; the code default is 250. Preserve configuration distinctions rather than turning defaults into production facts.

Sources: [language coverage and fixtures](discovery/languages.md#research), [job routing and gaps](discovery/jobs-transcription.md#research), `frontend/src/services/unified-transcription/providers/capabilities.ts:13-104`, [production configuration](production-facts.md#config-that-rules-read).

The language fixture comparison was exercised against source: 16 UI locale codes, 61 transcription names/codes, 53 translation targets and provider membership all match. The four extra enum values exist in production despite absent migrations; they must not be lost during migration.

## R2: AI providers, prompts and limits

Speech translation is not an LLM call. It sends Soniox `translation.type=one_way` and the first selected target's ISO code, then stores original and translated lines. Do not introduce an LLM translation gateway while claiming a like-for-like port.

The active analysis, chat and general completion-summary constructors explicitly select DeepSeek. They do not switch providers merely because `AI_PROVIDER` changes. The DeepSeek adapter calls `deepseek-chat` with temperature 0.3 and output cap 1000 tokens. An OpenAI adapter exists, with `OPENAI_MODEL` defaulting to `gpt-3.5-turbo`. Both use direct HTTP fetch streaming today.

- Analysis detects generic, lecture, meeting, sales, support or interview and requests relevant structured sections plus a summary under 300 words. Prompt: `frontend/src/lib/ai/prompt-templates.ts:93-146`; schemas: `frontend/src/lib/ai/schemas.ts:24-301`.
- Chat embeds the transcript and sanitized caller history, asks for transcript-related answers and forwards the job/source language. Prompt: `frontend/src/lib/ai/ai-service-manager.ts:74-91,245-263`.
- General completion summary is separate from the interactive structured analysis: concise or detailed prompt, under 300 words, collected from a stream. Source: `frontend/src/lib/ai/ai-service-manager.ts:99-149`.
- Production chat caps are 50 questions/20000 tokens per transcript and 500 questions/50000 tokens per user per day; paid users bypass these checks and increments. Checks and settlement are separate today, so concurrent calls and answer length can exceed the apparent remaining allowance.
- Nominal timeout/retry/rate settings do not prove enforcement. The streaming provider path lacks the corresponding abort/retry loop. Analysis computes a target-aware language option but drops it in `generateWithTemplateStream`; it does not guarantee that output language.

Sources and precise behavior: [AI inventory, ledger and R2](discovery/ai.md), [production settings](production-facts.md#config-that-rules-read), [language reconciliation](discovery/languages.md#rules).

**E4 implication:** compare identical models, prompts, schemas and streams. The current baseline is already plain HTTP; do not claim an SDK-removal benefit that does not exist. Fixing prompt framing or honoring previously ignored options is a separately recorded behavior decision, not provider-output parity.

## R3: active third-party keys and callbacks

The previous lead's read-only production probe found 46 keys, 40 active and 39 active/unexpired, belonging to 31 owners. Two keys from two owners were used in the preceding 90 days, with latest use on 2026-10-24. Callbacks had two successful deliveries in that period, latest 2026-10-16. Endpoint audit fields were empty, so the used API version cannot be established from those counts.

**The API and callbacks are live.** We cannot assume no customers depend on them, and we cannot infer specifically `v1-beta` usage. Inventory the existing request/response shapes and migrate key hashes without plaintext recovery. Before cutover choose either a compatibility boundary for the required shapes or confirmed client migration. The PRD explicitly makes a shim a decision rather than a default. Building the new versioned contract and fake callback endpoints does not wait for this decision; deploying a breaking replacement does.

Sources: [production API observations](production-facts.md#r3-public-api-use), [API contracts and research](discovery/api.md).

## R4: paid sources, free tools and voice demo

The distinctions below are behavior, not just page categories:

| Surface | Current behavior | Rewrite obligation |
|---|---|---|
| Uploaded media | Signed-in job, upload/progress, duration and billing rules | Preserve paid job flow |
| Direct URL transcription API | Creates a job with deferred duration/billing handling | Preserve URL validation, job and billing semantics |
| Signed-in YouTube | Creates a job from captions and skips the billed not_started transition | Preserve this current bypass; do not silently charge it |
| Anonymous YouTube transcript tool | Fetches/copies/downloads captions without a job; missing captions lead to product CTA | Keep free caption flow distinct from paid transcription |
| Browser recording and tab capture | Browser media capture and local export; subsequent upload is a separate action | Interactive browser shell, not an automatic transcription job |
| Public audio/video conversion, trimming, compression and noise removal | Server-first session/file workflows; `CLIENT_WASM_ENABLED=false` | Preserve processor runtime obligations, not static HTML alone |
| Voice demo | Separate voice conversation/DeepSeek fallback runtime, no transcript billing link | Named exclusion under the PRD's presumption; no new voice implementation |

The media processor is a separate service with seven route families and 39 family endpoints plus `/health`. The families are trim-audio, convert-audio, reduce-audio, remove-noise, video-to-audio, compress-video and convert-video. Each has upload, processing, progress, download and invalidation; selected families add ZIP or thumbnail retrieval. These paths are on the processor host, not Next.js `/api`.

Sources: [complete R4 processor routes](discovery/static-pages.md#research), `frontend/src/utils/free-tools/clientWasmEnabled.ts:5`, `frontend/services/free-tools-processor/src/index.ts:22-50`, [job source boundaries](discovery/jobs-transcription.md#research), [AI voice demo](discovery/ai.md#research).

**Packaging consequence:** the PRD's app-image/static-image design must account for a runtime media processor. Static generation cannot erase it. Choose its adapter/deployment placement during foundation design without dropping the public tools or adding a third product capability.

## R5: sitemap and intentional pruning

The checked-in sitemap contains **4527 unique URLs across 16 locales**. The surfaces worker derived URL candidates from page files, locale config, blog metadata, dynamic generators and the existing sitemap filters, then compared normalized sets: zero source-only and zero snapshot-only paths. This is source/config evaluation, not a successful Next.js build or a production crawl.

There are 159 dated article directories; the current JavaScript publication-date rule includes 157. April 31 normalizes to May 1, while April 32 and 33 are excluded. Preserve the current URLs/content identities deliberately rather than applying a different date parser and silently dropping content.

Pruning is intentional:

- 138 root keyword templates have 2208 locale candidates; 2170 pairs are excluded.
- Language matrix configuration has 3616 locale candidates; 112 traffic-earning URLs are listed.
- Translation matrices are largely pruned; traffic exceptions and curated keyword pages remain.
- Pruned pages can still render for direct links, with noindex/canonical consolidation. Sitemap exclusion is not permission to delete every direct route.

The snapshot also includes questionable entries such as errors, app routes and prototypes. P6's baseline remains the observed manifest unless a recorded decision changes it. Counting every generated language combination would undo intentional SEO pruning.

Per-locale counts, URL-family totals, exact source ranges and known canonical/hreflang mismatches: [static inventory and R5](discovery/static-pages.md).

## Decisions versus missing evidence

- Keep GoTrue: discovery found no concrete blocker requiring E2. Existing password/Google identities and account ids remain migration obligations.
- E1 still compares static generators; it must use the actual URL manifest and interactive/runtime boundary, not a few representative pages as the whole product.
- E3 palette selection and feature-flagged UI experiments do not authorize changing the main upload/transcript flow.
- Production schema dump, auth settings, cron/grants, provider acceptance and full migration balance parity remain unverified. Seeded schemas, fake ports and recorded fixtures let implementation proceed without claiming cutover readiness.
- Defect proposals are not approvals. Preserve current business outcomes unless the PRD already authorizes a clear defect correction, and record current/intended behavior and the decision before implementation.
