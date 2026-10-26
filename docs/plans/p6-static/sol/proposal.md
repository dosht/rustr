# P6 SOL proposal: one content compiler, static public HTML, real tool islands

Independent design only. Read the P6 brief, PRD, required discovery and the source references below. No Opus P6 proposal was read and no design was exchanged with the other advisor. One factual question was sent to the billing owner about an existing public catalog projection. No implementation, canonical/book edits, builds, tests, browser/provider calls or nested agents. Recommendations below require lead decisions and independent execution; none are acceptance evidence.

## 1. Recommendation and non-negotiable boundaries

Build a small reusable `modules/cms` content/URL compiler, with Markdown in git, typed metadata and one source-to-output manifest. My preferred E1 candidate is native Rust Markdown + Askama HTML, using the existing Svelte 5/Vite toolchain only for interactive public islands. Askama 0.16.1 already appears in this workspace lock; this is a dependency fact, not a measured E1 victory. Compare Astro, SvelteKit prerender and Rust against the same complete inventory before choosing. Do not select a framework by rendering three attractive examples and ignoring the rest of the URLs. [R1,R2,R19]

Static pages and interactive tools are separate responsibilities, not alternative deliveries. Reuse the real P3 processor and public-caption use case. Add shared pure presentation for public tools and thin browser adapters. Do not create another media backend, expose paid transcription anonymously, enable FFmpeg-in-browser by accident, or make `modules/cms` a workflow/plugin/admin platform.

Keep P5's signed-in app separate. `/api/session` verifies a session-presence hint; public HTML is not authentication. Preserve P7's ownership of keys, versioned public API and documentation truth. Preserve P4 D6's specifically named voice exclusion; it does not exclude recorder, captions, contact, tools or other live public flows. [R1,R3,R4]

## 2. Scope: count URLs, not directories or templates

The source-derived baseline is **4,527 distinct sitemap paths**, not a current production crawl or 4,527 static public success pages. It includes app/error/prototype URLs that require explicit dispositions. Discovery reconciled source generation/filtering to the checked-in XML with zero set differences. Preserve that reference set and its raw provenance before applying any correction. Counts below are the inspected discovery's source/snapshot result, not a new execution. [R2]

| Sitemap prefix family | Baseline URLs | Concrete obligation |
|---|---:|---|
| Blog | 2,508 | Localized articles/indexes plus the separate English editorial route; full metadata, first-image/social/Article/FAQ/TOC behavior. |
| Languages | 112 | Traffic-retained subset of 226 slugs × 16 locale candidates; directly reachable pruned pages still matter. |
| Translation children | 40 | Forward/reverse matrix and keyword paths, with per-locale redirects/indexability and named defects. |
| Tools | 186 | 138 de/tr/kk keyword landings plus three explicit tools × 16 locales. Keyword landings are signup content, not processor families. |
| Features | 705 | 45 English and 44 × 15 other locales, including FAQ/how-to/comparison content. |
| Free tools | 160 | Ten routes × 16 locales; index plus nine mounted routes over seven real processor families. |
| Legal | 5 | Index and English/Turkish privacy/terms documents; do not fabricate sixteen translated policies. |
| Certification | 2 | Two eager English sitemap entries; **three** source certificate identities and fallback locale reachability must also be reconciled. |
| Contact | 32 | Contact and success pages × 16 locales, including real submission. |
| Other routes | 777 | Root/editorial/persona/core pages and inventoried app/docs/errors/prototypes; classify each rather than turning this bucket into generic Markdown. |
| Total | **4,527** | Exact path-set reconciliation plus per-entry output/disposition. |

Locale sitemap totals are part of the baseline gate, not merely a sum: en 356; tr 282; pt 267; es 273; de 343; id 287; it 266; fr 263; el 264; ru 305; da 265; sv 264; no 263; fi 261; kk 300; az 268. English is unprefixed; the other fifteen are prefixed. SEO URL lists are not the product's 61 transcription languages or 53 translation targets. [R2,R5]

Additional source families to migrate, without implying disjoint sitemap counts: 138 root keyword files, eight editorial how-to files, 21 persona/business routes, fourteen named core-public routes, feature trees, FAQ, supported languages, homepage/navigation/footer, public pricing, legal, certificates, contact, blog content and assets. Root keyword candidates are 2,208; 2,170 exclusions leave 38 retained pairs. Only 112 of 3,616 language pairs are listed. The 359-entry traffic allowlist overrides broad pruning across several families; do not replace it with a blanket noindex or index-everything policy. [R2,R6]

Public proof families remain the union: SEO-001..016; SEO-P001..034 on desktop and phone; SEO-L01..16; applicable WEB-005/006/047/048; API-F17 and API-020/021; processor API-F45..84/API-022..030, RTL-004..008 and CRN-008/009; contact/newsletter API-F36/37 and EML-007/008. P5/P7/platform owners keep their joint clauses. This proposal neither closes them nor drops rows that appear in the baseline.

## 3. Import once; preserve evidence and publish an exact manifest

Use a read-only, deterministic importer against the saved legacy tree. No source-repository mutation or package installation is part of migration. Evaluate the actual page path generators, sitemap exclude/transform/additionalPaths, redirect data and blog metadata in an isolated capture with a frozen publication time. Hash every loaded source/config/content file. Static inspection is not enough to choose collision winners silently. The independent tester must compare this captured oracle to the new compiler. [R2,R6,R7,R8]

Produce these real artifacts during implementation:

- `baseline-sitemap.json`: every original XML loc, decoded comparison key, locale, family, priority/frequency and original XML/source hashes. Keep original escaping/encoding alongside the normalized comparison key.
- `source-to-output.json`: source-relative identity, locale filename and metadata Language, raw slug, source date, normalized publication date, source hashes, candidate paths, content identity, source indexability/canonical/alternates, asset references, output file or delegated route, status/redirect, and reason/decision id for every exclusion/collision/correction.
- `routes.json`: the deployed route table, including public HTML, permanent/temporary redirects, app handoffs, P7 docs handoffs, explicit errors and sitemap membership. One entry owns each decoded route; no ambiguous last-writer-wins output.
- `corrections.json`: per-URL old outcome → proposed/approved outcome with decision/provenance. The source baseline and corrected publication set remain independently inspectable.
- `assets.json`: source path/hash, public URL aliases, emitted hash/path, MIME, dimensions/alt where known, license/provenance and any external origin. Copy assets actually referenced by all retained/direct-link content, not just current sitemap winners.

Identity is independent of URL. For blogs use source directory + locale file as the durable import identity; retain metadata locale separately. Preserve all 159 directories, even excluded or colliding records. Source publication admits 157; April 31 normalizes into May 1, while April 32/33 do not pass the JavaScript date predicate. Missing locale/body is notFound, never an English article substituted beneath a localized slug. Metadata `Language` and duplicate slugs explain why Markdown file counts differ from unique URLs. A collision report must include every competing record and the captured source winner; lead approves redirect/merge/unpublished disposition before emission. Do not invent 159 published English pages or drop the losing source bytes. [R7,R8]

For ordinary article pages, preserve localized `1/2/3/4` title/description/long-description/slug, author/genre, published/modified dates and FAQ fields after `<+++>`. Preserve the full newest-first index without adding pagination. Cross-locale slug requests redirect to that requested locale's actual slug when its article exists; absent locale stays 404. Preserve all existing locale-exact malformed-slug redirects, including spaces, pre-encoded percent escapes and punctuation, with single encoding at the HTTP boundary. [R7,R8,R9]

A new article or scheduled publication is a content release: rebuild from git at an explicit as-of instant. Do not promise Next ISR or instant runtime publishing in a static-only image. Revalidation changes are observable release/routing changes, recorded as such; no new CMS scheduler or database is necessary.

## 4. Smallest concrete module and render architecture

### Reusable module

Proposed implementation units, **not existing signatures/stubs**:

| Unit | Responsibility | Must not contain |
|---|---|---|
| `modules/cms/core` | Pure typed document/locale/route/redirect/SEO/asset models; deterministic URL construction, metadata/sitemap serialization and consistency validation. Inputs include already resolved product indexing decisions. | Transgate/taskr dependencies, traffic-policy constants, billing prices, auth, media processing, network/filesystem access. |
| `modules/cms/build` | Native git/filesystem/Markdown adapter and chosen HTML renderer; produce the static bundle/manifests. The preferred adapter uses Askama. | Runtime content mutation, generic plugin registry, tenant/admin workflow, provider calls. |
| Transgate site build consumer | Import legacy content; own Transgate traffic/cluster/canonical policy and family data; consume the billing-owned public projection; provide branded content/templates and build `transgate/site/dist`. | A copied price catalog or business rules in templates. |
| Taskr site consumer | A real tiny two-locale page, blog index/article, locale navigation, metadata/sitemap and static package produced through the same module. | Dependency added to taskr-core or a compile-only fake second consumer. |
| Public browser bundle | Svelte desktop/phone tool views and browser mechanics, lazy-loaded only where required; common header/theme/consent/popup islands. | A second TypeScript business state machine or a mounted signed-in workspace on every public page. |

PRD/`modules/README.md` describe module crate groups including postgres/http. Recommend an explicit git-CMS exception: reserve the `cms` schema prefix but create **no empty postgres crate/table or write API** to satisfy a folder diagram. Add an HTTP adapter only if E1/routing actually requires one. Keep the existing module dependency gate and both real consumers; do not weaken `scripts/check-prd.py::module_graph`. The current gate checks module groups, forbidden transitive dependencies and consumer reachability, not an obligation to store git content in Postgres. [R1,R19]

The host-facing seams to publish before implementation consumers wire up are the concrete route/metadata bundle schema, content/asset loader, renderer input and output manifest. Product SEO policy belongs in Transgate's site adapter; reusable CMS accepts the resolved `robots`, canonical and alternate targets instead of learning traffic exceptions. Prefer normal typed data and renderer ports to arbitrary templates/code embedded in Markdown.

### E1 remains a measured decision

All three E1 candidates render the same pricing, SEO landing and blog article in two locales, then the **full** reconciled direct-route inventory, including pruned-but-reachable content. Compare Lighthouse SEO100, full build under two minutes, actual dependency/process count, output bytes and token integration. Include interactive-island packaging; comparing a Markdown-only Askama build to a working tools site is invalid. Freeze machine/toolchain/cache conditions, report clean dependency-cached and repeat builds, and do not omit the complete page emission from timing. PRD tie-break is no second JS framework. [R1]

Preferred Rust adapter dependencies: existing serde/serde_json, url 2.5.8, quick-xml 0.38.4 and a chosen SHA-256 version; Askama 0.16.1 is already locked. `pulldown-cmark` is a **proposed new Markdown parser**, to pin and prove against source markdown-it 14.1 before selection; it is not claimed installed or byte-equivalent. Verify headings, nested emphasis/links/images, lists, tables and TOC anchors against real content. Astro and SvelteKit/adapter-static versions are to be pinned by the E1 owner, not guessed in this advisory note. [R8,R19]

Browser dependencies reuse P5's Svelte 5.57.2, Vite 8.3.4, Tailwind 4.1.13, i18next 24.2.3, Playwright/Storybook and matching wasm-bindgen 0.2.128 pipeline as actually declared. Do not introduce React/Next/Redux/DAOs/FFmpeg-WASM into the new public bundle. Preserve a narrow media-Blob ZIP adapter if needed: source uses client-zip ^2.4.5. Its wrapper finishes with `.blob()`, so do not advertise the entire download as zero-buffering. [R3,R16,R20]

### Finite eager rendering versus blocking fallback

Separate **servable route inventory** from **sitemap membership**. Eagerly render the finite valid direct routes, not just the 4,527 sitemap entries: all root-keyword locale pairs, all 226 × 16 valid language pages, applicable translation pages/redirects, known blog locale/alias outcomes, all three certificate ids and valid locales, explicit tools, and ordinary families. Record the total from the importer; do not guess it from template count. Non-indexable pages still serve full content, noindex/follow and their approved cluster canonical. [R2,R6,R7,R10]

No source-style loading shell is emitted on a cold request. Source language paths have an empty eager list with blocking fallback; translation also blocks. The new image serves completed HTML immediately. Blog/tool/certificate fallback behavior is resolved by the imported route table. Unknown `/tools/{keyword}` locale/slug pairs keep the source permanent-home redirect; mismatched translation locale routes keep the source **temporary** redirect to that locale's `/translation`. Do not collapse 301/permanent and 307/temporary outcomes into client navigation. [R7,R9,R10]

Two source behaviors prevent pretending that every fallback is already finite: invalid language slugs are normalized to English, and reverse-translation fallback fabricates metadata for an unrecognized language. Recommend the explicit restrictions in §8 before static cutover. If lead declines those corrections, this static-only recommendation must be reconsidered with a real bounded runtime resolver using the same route/content semantics; silently replacing reachable fallback behavior with 404 would not be parity. No such runtime stub is proposed as implemented.

## 5. Page metadata, locale copy, links and assets

The CMS emits complete crawlable head/body without JavaScript. Do not port `KeywordsComponent`'s `isClient` early return, which suppresses even its Head before hydration. Every indexable route has one effective title/description/canonical/robots decision and the appropriate source-family schema; no generic global Product/FAQ on every page. [R6,R11,R12]

- Preserve Organization/WebSite/Product/Breadcrumb distinctions, per-feature FAQ/HowTo/comparison data, SoftwareApplication for translation, and Article/Breadcrumb/FAQ for blogs. Serialize structured data from typed records and visible facts, not concatenated JSON. Source hardcoded price offers must move to the billing projection. Audit unsupported SearchAction/review/capability claims against actual routes/content rather than fabricating features to make schemas validate. Lighthouse SEO100 is not rich-result eligibility or ranking proof. [R11,R12]
- Keep the traffic/cluster policy and English x-default rules where source actually uses them. Do not synthesize sixteen alternates for every keyword, policy or missing article. Language retained localized routes self-canonicalize; pruned language pages consolidate to English. Translation's English canonical and potentially unsupported hreflang are separately decided in SEO-D05. Alternate links must resolve to the intended content, not an app login, redirect loop or arbitrary fallback. [R6,R10]
- Import all public copy as real localized content. Reuse P5 common labels/primitives by their published contract; keep P6-only namespaces/content in its own owned files, not overlapping edits to P5's reachable-key catalogs. Recommend applying P5 D3's complete-reachable-key discipline to public UI: translate the real tools/editorial/navigation keys in all sixteen locales, verify interpolation, never fill catalogs with copied English. Preserve the source fallback/missing-key inventory as evidence. Blog missing-file 404 and the two actual legal document languages remain distinct from UI-label translation. [R3,R5]
- All sixteen UI locales are LTR. Transcript/caption content can be RTL independently; do not change the UI locale set to match speech or SEO language codes. Use document/block language attributes for the actual English/Turkish legal body and localized articles. No translated legal policy is invented. [R5]
- Render Markdown through a restricted typed tree with escaped text/attributes; raw HTML/MDX/script is not executed. Validate link/image schemes, disallow traversal and encoded separators from becoming output filesystem paths, and keep a deliberate slug encoding model with raw originals. Escape `<`, `>`, `&`, U+2028/U+2029 in script JSON. Imported content and public-caption/user messages do not get blanket `safe`/`{@html}` treatment. [R8,R12,R3]
- Keep first-content-image selection and fallback homepage thumbnail, author images, alt text, content image links and legacy asset URL aliases. Build an asset dependency report and test actual emitted files; no silent image omission after replacing Next Image. Local responsive derivatives may be generated at build time with the original/provenance retained. Remote image fetching/mirroring needs explicit rights and authorized acquisition, not a network surprise during build. [R8,R12]
- Root tokens, Paper Moss default, light/dark/system and licensed local fonts remain P5's authoritative assets. Do not copy font/logo blobs into a second public directory or introduce runtime CDN fonts. Analysis/invoice/transcript document exports remain `transgate/export`/billing contexts; CMS does not rebuild their writers or claim a certificate image is a new PDF product. [R3,R21]
- Public layout retains mounted step order and CTA meaning, with pure desktop/phone tool views; shared Svelte primitives provide keyboard/focus/status behavior. Use actual buttons for FAQ disclosure, labelled menus/inputs, keyboard-adjustable trim control, focus return, reduced motion and contrast in both themes. Long Kazakh/Turkish/German strings must fit. The source clickable-div FAQ is not an accessibility contract to copy. [R12]

## 6. Real public tools, no second media backend

### Seven processor families

Keep public `/free-tools` plus trim-audio/audio-trimmer/video-trimmer (one trim implementation), convert-audio, reduce-audio, remove-noise, video-to-audio, compress-video and convert-video. They create anonymous **processor sessions**, not jobs/accounts/package debits. P3's `transgate_core::tools::Family` and validators remain authoritative. `transgate-processor` owns HTTP/filesystem/FFmpeg, limits, invalidation/cancellation and cleanup. [R2,R13,R14]

| UI family | Existing real protocol to preserve | Presentation/result distinctions |
|---|---|---|
| Three trimming routes | Upload → waveform/duration → interior trim point → POST `/trim-audio/trim` 202 → progress → two outputs. | Two local part downloads and mounted client-side Download Both ZIP. The source already fetches both parts to Blobs before presenting success; do not fetch server ZIP after consumed outputs and call it equivalent. Server `/download-zip` remains a real API contract, tested separately. |
| Audio conversion | `/convert-audio/upload`, synchronous `/convert` 200, then `/download`. | No fake 202/poll requirement; preserve format/options/effective Opus rate behavior. Its progress endpoint exists independently. |
| Reduce audio | Upload, `/reduce` 202, progress, download. | low/medium/high, source progress/percent variants and expired-as-body behavior. |
| Denoise | Upload, `/denoise` 202, progress, download. | low/medium/high; status error versus failed and outputFilename must not be lost in a generic success decoder. |
| Extract video audio | Upload, thumbnail, `/extract` 202, progress, download. | Format/bitrate/rate/channels; no-audio 422; no bitrate for WAV; Opus effective rate. |
| Compress video | Upload, thumbnail, `/compress` 202, progress, download. | Quality presets and nested result filename/extension. |
| Convert video | Upload, thumbnail, `/convert` 202, progress, download. | Existing codec/resolution/fps/bitrate/rate/channels; outputFilename and source percentage mapping. |

Sources: actual seven `*Server.ts` adapters in R14, current P3 domain/runtime in R13, mounted trim component/download implementation in R16. Forty method/path endpoints (39 family routes plus health) are preserved, including DELETE invalidation and thumbnails; a UI route count is not an endpoint count.

Add one bounded **public-tool presentation leaf** in Transgate core, not CMS: family-tagged options, file-handle identity, upload/session/process/poll/download states, typed error and Retry-After notices, reset/disposal and stale-effect fencing. Proposed effects name upload/process/progress/download/invalidate; publish actual Rust/TS contracts before views depend on them. Reuse existing validators rather than reimplementing accepted codecs, limits or trim rules in JS. Mechanical ports own File/Blob/FormData, network transport/abort, waveform/media elements, ZIP/save and object URLs. Large file bytes never travel through JSON/base64 or repeated wasm copies. Shared state wasm is not FFmpeg wasm; keep source server-first media processing. [R13,R14]

Retain the source 1.5-second progress cadence, 10-second cheap-control request guard and **30-minute transfer/synchronous-processing budget**. Do not repeat the stale comment that every upload times out in ten seconds. Classify 429/503/Retry-After, transport timeout, expiry, invalid options and server failure without turning all outcomes into generic retry. Do not automatically resubmit uncertain processing. Disposing/resetting closes local work and invalidates an owned session where the current cancellation contract requires it; it does not prove a remote upload never arrived. P3 D12's reaping/invalidation semantics remain intact. Preserve one-hour/two-hour TTLs, 1/2 GiB caps and 20/10 per-family/IP processing allowances; P6 does not redesign them. [R13,R14]

### Microphone recording and tab capture

Use browser `getUserMedia`/`enumerateDevices`/MediaRecorder/Web Audio and `getDisplayMedia` as thin adapters around pure presentation transitions. No upload or billed job is automatic. Microphone flow includes device choice, record/pause/resume/stop, local preview and WAV download; source MIME probing tries webm/ogg/mp4/wav and records at 128000 bits/s. Tab capture requires video+audio display selection, audio-track validation, recorder controls and WAV download with the source original-format fallback on WAV failure. User-gesture permissions, unsupported browser, denied permission, stopped track, missing tab audio and conversion/save errors are visible states, especially on phones. [R15]

Retain the mounted recovery prompt/dismiss/recovery path and existing local recovery data compatibility; do not mistake all classes in the legacy recovery framework for mounted behavior. Storage failure must not destroy the active recording. Source recovery validity has a one-hour duration check; it is **not evidence of a general one-hour recording limit**. Preserve the actual source contract, not a new commercial cap. Release every stream track, AudioContext, timer and object URL on stop/error/disposal; propose recording permission acquisition on user action rather than leaking the eager source device-enumeration stream (§8). No fake tab-capture success is promised on unsupported phone browsers. [R15]

### Public YouTube captions

Reuse the existing `POST /api/youtube/public-transcript` and `PublicTranscript`/`PublicCaptions` port, not the paid-caption/transcription path. Request accepts URL, txt/timestamped/srt/vtt and optional valid lang. Preserve videoId/language/segments/text/format, invalid/no-caption/unavailable/rate/upstream outcomes and 10/IP/minute policy; current core also has six-hour successful-result caching. Use the same pure `format_transcript` for local format changes without another request, copy feedback, reading expansion/Escape, and TXT/SRT/VTT/download-all behavior actually mounted. Missing captions show the existing paid-product CTA, not a hidden new transcription job or a promise that an upsell fixes an upstream outage. [R17]

`/youtube-video-analysis` is editorial plus a clearly identified hardcoded example; `/subtitle-generator` is a sign-in-gated SEO landing. Preserve the real route/CTA content without making either an anonymous AI/transcription backend. The 138 `/tools/{keyword}` pages are also signup landings; they must not be counted as 138 conversion implementations. [R11,R18]

## 7. Exact shared seams and runtime routing

| Boundary | Existing authority / necessary work |
|---|---|
| P5 identity | `GET /api/session?locale=...` returns verified SessionProfile; 401 signed out, 410 barrier, 503 directory outage. `transgate.session` is only a presence hint. Use P5 `GoTrue`/PKCE/session locking/refresh rather than parsing JWT claims or creating a second auth SDK. |
| P5 app client | Published `loadTransgate(config, platform)`, synchronous dispatch/subscription/dispose, `TransgateConfig {apiUrl,authUrl,authKey?}`. Public site must not start the full AppPresenter on arbitrary public URLs. A narrow public-session/Google-auth facade, if needed for the homepage popup, is published by p22 over the same client; do not invent a provider query route. |
| P5 route split | Only `presentation::route::is_app_path` owns SPA fallback: home variants, signin/new-signin/new-password, billing, checkout/confirm with supported locale prefixes. `/`, bare locale roots, `/checkout`, `/dashboard/*`, assets/unknown and `/api/*` are **not** blanket SPA fallbacks. Preserve POST confirmation and host GET checkout behavior before static routing. |
| P5 job/AI | Session start is `/api/jobs/start`, not P7 public transcription intake. Session `/api/v1/ai/*` remains unchanged. Public tools do not call these implicitly. |
| Billing | Existing Transgate composition nests the catalog at `/api/billing/catalog`; its handler itself performs no identity call. Current `PlanCard`/`PaygCard` projection does not contain the complete public landing hours/expiry/locale-price view. R owns its narrow extension/shared projector, not CMS. |
| P3 runtime | Processor remains the separately deployed FFmpeg service; public captions remain an existing anonymous application endpoint. Do not forward either through a P7 API-key gate. |
| P7 | `/dashboard/api-keys` behavior and generated API/OpenAPI truth are P7-owned joint prerequisites. Preserve `/api-docs` and `/api-reference` URLs via explicit documentation handoffs. No P7 contract is invented here; P6 cannot ship plausible docs or key controls over absent routes. |
| Shared documents | P5 billing context + `transgate_export::documents` remain authoritative for invoice/analysis bytes/fonts. Caption text formatting uses public_transcript, and media ZIP is a scoped Blob transport adapter, not another generic JS document engine. |

These are read from actual source/accepted P5 notes, not a proposal exchange. [R3,R4,R13,R17,R19,R21]

Public home behavior: deliver useful static HTML first. With a session hint, verify once through the shared client before redirecting to the locale's app home or suppressing the source homepage desktop popup. A 401 clears stale presence; 503 does not create a redirect/sign-in loop or claim logout; 410 follows P5's fenced receipt/auth handling without hydrating deleted profile data. Without a valid session, preserve homepage-only desktop popup timing and the phone cookie-only distinction. Moving source cookie-presence middleware redirection to verified client enhancement is an explicit P5 D13 public SSR/routing difference, not unchanged HTTP parity.

### Public pricing: one money authority, available without authentication

Static HTML must contain usable price cards and schema, not wait for a browser fetch to become a pricing page. Generate both monthly/yearly public views from a billing-owned pure projection of the deployed catalog, including plan hours/expiry, displayed/struck USD amounts, billing interval and public trial facts. Preserve source public PAYG locale flooring for id/pt; it differs from charged quote arithmetic. Do not call a minimum-five-hour purchase quote to derive the displayed one-hour landing rate, or assume PurchaseState's PAYG card is already the locale-aware public projector. [R19]

Extend the **existing** catalog response to expose that public presentation (locale/interval selector and catalog version), retaining a single source for the build exporter and runtime read. Public cards expose no provider plan identifiers, private offer allowlists, customer state or checkout authorization. All numeric price/schema/copy bindings use it; translated Markdown never becomes a second price catalog. No public quote/coupon/checkout is added. CTA preserves sign-in/app handoff; P5 alone obtains an authenticated server quote, terms and gateway handoff, with `display_amount` and charged economics kept separate.

Hash/version the public projection in the static bundle and deployment manifest. A runtime pricing island reads the same public projection; do not silently manufacture prices if it is unavailable. The static published snapshot stays read-only and identifiable, never a promised payable quote. Deploy static/app catalog versions together; a mismatch must fail the publication/readiness check and require a rebuild, not leave stale HTML/JSON-LD indefinitely behind a successful client repaint. Runtime catalogs are authority; the stored build snapshot is an output of that authority, not a separate editable configuration. Billing owner's actual DTO/publication is a prerequisite before consumers wire up.

### Contact, newsletter and delivery are not static-only

Preserve POST `/api/contact-us` `{email,subject,message}`, 405 for other methods, contact mailbox send before the acknowledgement, success navigation only after actual accepted success, and visible failure/retry. Newsletter POST `/api/subscribe` remains the operator-interest notification, not a new double-opt-in/subscriber-campaign product. Reuse existing Mailer/templates/platform configuration in the application host; CMS renders the forms and supplies message keys, not Resend secrets or a new delivery stack. Propose plain-text-safe escaping and visible failed-submit state rather than copying raw submitted HTML and console-only errors. Preserve ordered outcomes and subjects/recipients through local mail fixtures; no external mail is sent during advisor/design work. [R2,R22]

### Packaging

Produce a second **public static image**, with immutable generated HTML/assets/manifests and a non-root static edge/server, alongside the P5 application image and the existing separate P3 processor image. Avoid Node SSR or a CMS database in the preferred path. One deployment edge routes exact API/auth/app/checkout/docs paths before manifest-owned public files and redirects. Preserve proxy streaming/trusted-IP/CORS and long processor transfers; sessions are process-local, so the processor stays a single instance unless its owner explicitly changes storage/routing. A disk volume does not restore sessions after restart. [R3,R4,R13]

Deployment supplies a public runtime-config asset from the same validated host configuration (API/app/processor/auth/public site origins, optional telemetry/captcha public settings only). Its path/schema and injection/escaping owner must be published; do not bake deployment secrets into Markdown or assume a static Caddy template can safely interpolate arbitrary JSON. P5's existing serve-time `tg-config`/`tg-confirm` remains app/checkout-owned. Unknown APIs stay JSON404; unknown files/routes stay a real404, never the app HTML with status200. Add `/health` for the static image and check the full packaged route split, MIME, cache headers and licensed assets. Root lock/workspace/container files retain one integration writer.

Consent-aware telemetry uses P5's ports and D12 decision: no separate GA/Hotjar/New Relic stack. This does **not** mean every third party is blocked before analytics consent. Source/P5 YouTube iframe API loads when the YouTube transcript opens; the public example also has an external YouTube thumbnail. Privacy copy must name those contacts separately from analytics and from server-side public-caption fetching. No claim of cookie-consent-gated YouTube or remote provider deletion is made. [R3,R18,R23]

## 8. Decisions requested: corrections are not hidden source parity

| ID | Recommendation | Observable change and accounting |
|---|---|---|
| SEO-D01 | Advertise only the five intended indexable translation keywords unless lead changes their indexing policy. | Reconcile eleven currently listed noindex keyword URLs explicitly; baseline 4,527 remains a separate immutable audit, not silently replaced by a smaller count. |
| SEO-D02 | Use full route identity for `/tools/{keyword}` indexing/canonical policy. Keep all 138 candidate mappings; enumerate traffic-retained tool paths and deliberate canonical targets. | Current root lookup drops `/tools/`; no blanket deletion or declaration that all 138 become indexable. Revised sitemap delta is a per-URL approved artifact. |
| SEO-D03 | Remove 404, private app/checkout and retired prototype entries from the published sitemap, while retaining their routed/retired baseline dispositions. | No fake public pages to retain a count. Corrected target is computed from explicit decisions; original equality gate and strict row coverage stay. Lead approves the corrected set before P6 can claim completion. |
| SEO-D04 | Unknown language slug returns real404, not default English content; valid pruned slugs still render normally. | Named correction to source's normalization-before-validation. Includes cold-request/no-JS response evidence. |
| SEO-D05 | Alternate only real supported-UI routes with equivalent indexable content; no unreachable source-language hreflang. Explicitly decide self-canonical retained localized translation pages versus intentional English consolidation. | Do not add UI locales or claim an untranslated English template is a translated equivalent. Verify reciprocal links/canonicals and traffic exceptions per URL. |
| SEO-D06 | Preserve all source identities/bytes and captured URL winners; normalize admitted April31 metadata to its valid May1 ISO date, keep April32/33 excluded, and require named collision dispositions. | No new 159-published-pages claim or silent slug loss. Publication/redirect changes are listed before build; no live sitemap count guessed here. |
| P6-C01 | Finite validated reverse-translation/certificate routing instead of arbitrary fabricated reverse metadata or unknown-certificate soft200. | Source reverse fallback can invent language metadata/testimonials; certificates contain three known ids despite two eager paths. Approve known-route eager inventory and real404 for unknowns. This is additional to SEO-D04, not a secretly removed fallback. |
| P6-C02 | Bind all actual price offers/structured prices to billing, and review unsupported translation-language/capability claims (LNG-D10) against real product lists. | Do not implement invented translation support to satisfy SEO copy. Preserve URL/content provenance while lead approves truthful copy/schema corrections. Hardcoded demo remains explicitly an example, not a fake backend. |
| P6-C03 | Full server-built public head/body, safe structured-data/content output and canonical manifest links. | Do not preserve client-only null SEO shells, unsafe script JSON or broken relative links as intended behavior. First-paint/escaping/redirect differences are explicit. |
| P6-C04 | User-gesture capture permissions, guaranteed track/context cleanup and visible conversion/contact failures. | Fix eager microphone permission-stream leakage and console-only failure behavior without adding a recording-duration cap, anonymous paid transcription or new download formats. Tab's existing original-format fallback remains distinct from microphone WAV-only behavior. |

Approve a **two-part URL gate**, not a count waiver: (1) exact 4,527 baseline set and full source reconciliation; (2) exact corrected published set derived by applying only lead-approved per-URL changes, with every removed/redirected/delegated entry and every retained direct page accounted for. If corrections remain undecided, keep the gate open; do not declare a smaller site complete. D6 voice exclusion and the already accepted P5 prototype dispositions remain narrowly identified in that reconciliation.

## 9. Ownership and build order after approval

No assignments/agents are started by this note. Proposed implementation slices have one writer each:

1. **Lead + importer/CMS owner:** freeze baseline oracle, identities, collision/asset/locale reports and correction decisions; publish real CMS route/metadata/render input before dependent builders.
2. **CMS/build + example owner:** pure module, native adapters/E1 candidates and actual Taskr two-locale consumer. Root integration waits for coherent manifests; no empty crates/stubs.
3. **Public-content owner:** Transgate import, family templates, blog/locales/assets, complete source-to-output mapping. Own the site content tree; do not edit P5 catalogs simultaneously.
4. **Public-tools core/client owner:** family presentation and bindings using P3 policy/protocols, recorder/caption presentation and effect contracts. Existing p1X/p22 remain sole integration writers for their shared module declarations/bindings; P3 processor files stay held except concrete coordinated defects.
5. **Public-tools views/browser owner:** pure desktop/phone views, media/network/File/Blob/ZIP adapters and stories/component/e2e source over published VMs. No commercial/filtering logic in Svelte.
6. **Existing billing/host owners:** billing-only public projector/catalog fields; application contact/newsletter/runtime config/routing; P5 session/checkout/CSP reuse. P7 publishes the real documentation artifact and key/API contracts, rather than a guessed CMS replacement.
7. **Independent integration/test owner:** after lead's coherent window, root lock/build/package integration, E1 execution, full manifest/site/tools/browser/container evidence. Fable medium or Claude reviews the converged proposal/implementation after the explicit cross-critique, not during independent research.

Importer/manifest and E1 are prerequisites to committing to a generator. Content migration and tool leaves can then advance independently behind the published seams; pricing depends on billing's concrete projector, and final public routing/docs acceptance depends on P5/P7. Lead alone updates canonical decisions/ledgers/gates. P8 native shells may open public URLs, not clone SEO pages/browser-only capture; P9 owns real deployment/DNS/retention/provider/legal acceptance and cutover.

## 10. Executable acceptance plan, not advisor execution

The following are **proposed implementation deliverables/commands**, not tools that already exist or were run here:

```sh
# Independent test owner, after implementation and coherent locked integration:
./scripts/p6-static/import-baseline.sh --source /readonly/Transgate/frontend --as-of <frozen-UTC>
./scripts/p6-static/experiment-e1.sh --manifest <source-to-output.json>
./scripts/p6-static/build.sh --site transgate --as-of <frozen-UTC>
./scripts/p6-static/verify.sh --baseline <baseline-sitemap.json> --routes <routes.json> --base <owned-static-origin>
./scripts/p6-static/build.sh --site taskr --as-of <frozen-UTC>
# Existing module gate remains unchanged in meaning:
./scripts/gate-modules.sh
```

These scripts must invoke real native build/renderer, inspect actual output/HTTP and exit nonzero on omissions or mismatches. A fixture hash, nonempty directory, Cargo dependency or constant URL count is not the acceptance test.

| Gate | Required observable evidence |
|---|---|
| Source/content/URL completeness | Baseline set equals all 4,527 XML paths and exact sixteen locale totals. Every source candidate/article/asset/duplicate/missing locale has a reasoned disposition. Approved corrected set equals actual sitemap locs; no duplicates, unapproved additions/losses, accidental percent double-encoding or fabricated published records. Check full HTTP status/Location/content identity, not only file count. |
| Pure CMS and product policy | Domain-first boundary tests for locale/slug encoding, canonical/alternate membership, pruning/traffic overrides, dates/collisions, publication cutoff, unsafe links/raw HTML/script closure and redirect loops. Both consumers execute real generation and locale/article navigation through the same module; dependency gate forbids module imports from either product. |
| E1/SEO | Pricing, SEO landing and article in two locales for every candidate; full route build under two minutes with declared conditions. Lighthouse **SEO100** on representative home, pricing, article, keyword, language, translation, feature, tool, legal/contact/certificate samples across locale families. Separately parse and validate title/robots/canonical/OG/hreflang/schema and visible-data correspondence for the complete indexable set; do not rely on Lighthouse to prove schema semantics. Noindex pages have separate expected-noindex assertions, not a weakened score target for indexed samples. |
| Blog/metadata/assets | Exercise April31 admitted/normalized, April32/33 excluded, missing locale/body404, duplicate URL identity/winner, malformed legacy slug301, cross-locale redirect and full sorted index. Inspect actual image/font/logo/license bytes, first-image/fallback, FAQ schema and TOC/inline/table rendering. Every source link resolves or has an explicit disposition. |
| Desktop/phone public flows | Real static responses plus actual Svelte/wasm islands. Header/footer/persona/feature/how-to navigation, all sixteen reachable public catalogs, monthly/yearly cards and signup handoff, FAQ keyboard/disclosure, contact success/failure, legal/certificates, localized blog switch/redirect. Both themes/system, long strings, keyboard/focus/status, no-JS crawlable content and phone touch controls. Stories alone do not close SEO-P rows. |
| Processor seven-family proof | Actual processor binary/FFmpeg and real tiny media through all mounted routes: upload/option errors, synchronous versus202, partial progress before finish, valid playable codec/rate/channels/duration output, no-audio422, two trim parts, mounted local ZIP with independent unzip/content read, separate server ZIP, thumbnails, reset/invalidate/expiry/429/503/network/transfer errors. Assert no account/job/debit; verify disposal, no stale results and temp/child cleanup through the existing P3 lifecycle. Do not replace these with request mocks or relabel P3 API passes as P6 UI proof. |
| Browser capture proof | Chromium actual getUserMedia with synthetic media, pause/resume/stop/preview/WAV bytes, device/permission/error/disposal; controlled actual display/tab-audio session with audible synthetic source, missing-audio and unsupported phone/browser behavior. Independently decode WAV and compare duration/channels/sample rate; facade mocks alone cannot prove capture. Where platform automation cannot drive the picker/audio, report the concrete device limit and require authorized manual evidence rather than fake success. |
| Captions | Real anonymous host endpoint over owned loopback PublicCaptions adapter: URL/lang/format errors, cache/rate outcomes, no captions/unavailable/upstream errors, segments→local format changes without refetch, copy/fullscreen/Escape/download names and independently read TXT/SRT/VTT. Assert no paid dispatch/job/usage. Authorized live YouTube compatibility is a later named prerequisite, not implied by fixtures. |
| Session/pricing/mail seams | Valid/expired/erased/directory-failed hint on public root without redirect loop or unauthorized metadata. Same billing public projection across native static export/runtime locale/interval responses, id/pt public discount flooring, version mismatch refusal, and authenticated P5 repricing after CTA. Local mail capture proves contact-before-ack and failures; newsletter remains an interest notification. No billing/identity policy mocked away to close a whole clause. |
| Packaged routing/privacy | Non-root public/app/processor images: `/health`, direct deep links and redirect status, unknown public404/API JSON404, app allow-list, actual POST checkout precedence, static MIME/cache/CSP/config escaping, no secrets/build tools/runtime downloads, licensed assets present. Network log distinguishes optional consent-gated analytics from YouTube iframe/thumbnail/upstream-caption contacts. All resources and teardown are owned by the independent test run. |

No acceptance item in this table is executed by this advisor. Existing P3/P4 evidence is a regression baseline, not proof of the new public views. Outstanding live checks include actual public host/DNS/locale negotiation, Search Console/traffic data freshness, model/provider/YouTube compatibility, capture device support, email/telemetry delivery, legal/image rights, configured catalog and deployment provenance. They remain named P8/P9/cutover dependencies rather than being silently removed from P6's backend/UI obligations.

## 11. Source references actually read

All `frontend/...` paths below are read-only under `/home/mu/Business/Transgate`; other paths are in this repo. Line references identify inspected behavior, not executed observations.

- **R1:** `docs/prd.md:23-96,112-170`; `docs/plans/p6-static/brief.md:1-21`.
- **R2:** `transgate/docs/discovery/static-pages.md:11-36,40-68,94-145,149-217`; `screens.md:13-34,49,67-89,142-145`; `api.md:39-81,108-126`; `platform.md:25-28,46-49,93-99`.
- **R3:** `docs/plans/p5-web/decisions.md:7-26`; `implementation-contract.md:5-43`; `implementation/hosts.md:9-21,51-75`; `implementation/web-shell.md:76-109,127-152`; `implementation/client-wasm.md:5-47`.
- **R4:** `transgate/docs/discovery/api.md:13-37,128-158`; accepted P5 D5/D13/D14; P4 D6 in `docs/plans/p4-ai/decisions.md`. No independent P7 implementation contract was found in the inspected plans directory; future docs/key interfaces must be published by that owner.
- **R5:** `transgate/docs/discovery/languages.md:11-38,146-156,164-169,208-230`; `frontend/next.config.js:21-24,51-55`.
- **R6:** `frontend/src/config/seoIndexing.ts:1-85,245-325`; `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-54`; `frontend/next-sitemap.config.js:543-686`; `frontend/public/robots.txt:1-9`.
- **R7:** `frontend/src/pages/blog/[[...articleSlug]].tsx:46-158,162-231`; dated source `frontend/public/articles/2025-04-31/en.md:85-100`.
- **R8:** `frontend/src/domain/blog.ts:7-58,61-96,334-368,625-642`; `frontend/src/components/BlogList/BlogList.tsx:64-118`.
- **R9:** `frontend/config/legacyBlogSlugRedirects.js:1-55`; `frontend/src/pages/tools/[keywords].tsx:21-57`.
- **R10:** `frontend/src/pages/languages/[slug].tsx:8-24,57-87`; `frontend/src/components/Language/language.tsx:159-171,229-237`; `frontend/src/pages/translation/[...slug].tsx:149-175,265-278,373-381,451-474,489-580,588-785`.
- **R11:** `frontend/src/components/KeywordsComponent/KeywordsComponent.tsx:24-29,60-118`; `frontend/src/components/SchemaComponent.tsx:6-26,48-130`.
- **R12:** `frontend/src/components/BlogPost/BlogPost.tsx:17-33,79-135`; `BlogPost/FAQ.tsx:14-74`; `frontend/src/components/SEO/StructuredData.tsx:8-18`.
- **R13:** `transgate/core/src/tools.rs:1-155`; `transgate/processor/src/lib.rs:1-56,76-115,119-152`; `transgate/processor/Cargo.toml:1-27`.
- **R14:** `frontend/src/utils/free-tools/{trimAudioServer.ts:53-134,convertAudioServer.ts:29-67,reduceAudioServer.ts:28-76,removeNoiseServer.ts:25-87,videoToAudioServer.ts:24-96,compressVideoServer.ts:24-85,convertVideoServer.ts:18-102}`; `processorClient.ts:22-116`; `frontend/src/hooks/free-tools/useProcessorHealth.ts:56-149`.
- **R15:** `frontend/src/pages/tools/record-audio-online.tsx:13-95,193-263,266-334`; `tools/tab-audio-capture.tsx:14-21,75-83,145-175,265-305,327-377`; `frontend/src/components/TabAudioCapture/DownloadControls.tsx:34-96`; `frontend/src/services/tab-audio-capture/recovery-service.ts:19-75,81-115,134-170`.
- **R16:** `frontend/src/components/TrimAudio/TrimAudioTool.tsx:1-38,198-238,319-382`; `ClientDownloadSection.tsx:1-23,45-71,117-148`; `frontend/src/utils/free-tools/clientZip.ts:9-49`.
- **R17:** `frontend/src/pages/api/youtube/public-transcript.ts:33-144`; `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:110-184`; `transgate/core/src/public_transcript.rs:1-35,87-119`; `transgate/http/src/lib.rs` public-transcript mount/handler.
- **R18:** `frontend/src/components/features/youtube-video-analysis/YouTubeAnalysisLandingPage.tsx:1-9,52-85`; `frontend/src/pages/subtitle-generator.tsx:1-18`; `transgate/web/src/lib/platform/media/youtube-player.ts:7-12`.
- **R19:** `frontend/src/components/LandingPage/Pricing/Pricing.tsx:10-19,34-57,85-94`; `PricingCard/PricingCard.tsx:13-51,86-114`; `frontend/src/domain/packages.ts:12-89`; `modules/billing/core/src/pricing.rs:299-323,358-394`; `modules/billing/http/src/routes.rs:96-118,144-151`; `transgate/billing/src/lib.rs:203-204`; `modules/README.md:3-9`; `scripts/check-prd.py:219-255`; `Cargo.lock` inspected package declarations for Askama/url/quick-xml; `transgate/web/package.json:17-44`.
- **R20:** `frontend/package.json` client-zip/markdown-it/FFmpeg declarations; no dependency installation or execution.
- **R21:** `docs/plans/p5-web/implementation/documents.md:25-36` (one authoritative local font/logo asset set and in-memory document boundary).
- **R22:** `frontend/src/pages/contact/index.tsx:10-51`; `frontend/src/components/LandingPage/ContactUS/ContactUS.tsx:12-37`; `frontend/src/pages/api/contact-us.ts:12-28`; `frontend/src/pages/certification/[id].tsx:22-42,84-113,150-172`; platform discovery EML-007/008.
- **R23:** P6 brief §required outcome and P5 D12; actual P5 YouTube iframe constant in R18; public external demo thumbnail in R18. Analytics consent does not establish a universal third-party-network prohibition.

## 12. Handoff

Independent proposal complete. No implementation authorized or started by this proposal. Only `docs/plans/p6-static/sol/proposal.md` was written. HOLD under the current-batch priority instruction: no cross-critique, next-phase work, agents, launches or provider/model substitution. No sibling proposal was read. Resume only on explicit lead authorization after the reset and batch-completion conditions; elapsed time alone is not authorization.
