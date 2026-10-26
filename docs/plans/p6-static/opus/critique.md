# P6 cross-critique: Opus on Sol

2026-10-26. I read `sol/proposal.md` (frozen) after both proposals were saved, and re-read the source behind every disputed point below. Both proposals stay unchanged. No builds, tests, browser runs, provider calls or new agents. Counts are read-only queries over the source snapshot. Lead decision P5 D20 (app bundle at `/_app/*`, public `/assets/*` preserved) is treated as decided.

## 1. Where Sol is right and my proposal was wrong or incomplete

I adopt each of these in `response.md`.

| # | Sol point | Source check | Effect on my proposal |
|---|---|---|---|
| A1 | Reverse translation fabricates a page for any unknown language | `translation/[...slug].tsx:105-115` matches `/^(audio\|video)-translation-to-([a-z]+)$/i`. Lines 668-700 build fallback metadata (name, challenges, use cases, testimonials) for any unrecognized `[a-z]+` | My table said unknown translation paths are already a 404. That is wrong. The candidate space is unbounded, so static parity is impossible without a correction or a runtime resolver |
| A2 | `KeywordsComponent` renders nothing before hydration | `KeywordsComponent.tsx:61-68` `if (!isClient \|\| router.isFallback) return null`. Head (`:73`) and `KeywordPageIndexing` (`:98`) are inside it | The server HTML of all 2,208 root-keyword and 138 `/tools` keyword pages has no head and no body. My G0 "crawl SSR HTML" oracle would capture emptiness. The oracle must record hydrated DOM for those families, and full server-built content is an explicit correction, not parity |
| A3 | Homepage desktop sign-in popup | `_app.tsx:67,73` mount `SimpleSignInPopup`. `SimpleSignInPopup.tsx:16-48`: desktop only (`innerWidth <= 768` or a mobile UA hides it), `/` only, 2 s delay, Google OAuth `redirectTo: origin/home` | Missing from my inventory. It needs a narrow P5/p22 Google-auth facade and P5 UI-03 suppression after verified sign-in |
| A4 | Convert audio is synchronous | Sol R14 `convertAudioServer.ts` (200 after completion); API-024 in rules | My generic `ProcessorTool` assumed 202 plus polling for every family. The family descriptor must carry sync or async |
| A5 | Structured-data offers are hardcoded and disagree with the catalog | `SchemaComponent.tsx:109-236` emits `price` 0, 1.49, 13.99, 20.99. The catalog is 14 and 21 (`domain/packages.ts:25,35`) | Binding schema offers to the billing projection is a correction with visible schema change (13.99 becomes 14). My "capture from oracle, never invent" rule would have frozen a wrong price |
| A6 | Microphone permission is requested on page load and the stream is never released | `record-audio-online.tsx:193-209`: `getAudioDevices()` calls `getUserMedia({audio:true})` in a mount effect and discards the stream | A correction candidate (permission on user gesture, tracks released). It is not parity |
| A7 | Redirect statuses | Next.js `permanent: true` is 308 and `false` is 307 [INFERENCE, oracle confirms] | I wrote 301 and 302 in places. Use 308 for blog cross-locale, legacy blog and `/tools` invalid pairs, and 307 for translation locale mismatch, unless the oracle shows otherwise |
| A8 | Tab-capture recovery service, contact failure visibility, Lighthouse is not schema proof, separate noindex assertions | `services/tab-audio-capture/recovery-service.ts` (Sol R15); `ContactUS.tsx:21-32` logs to the console only | Adopted. My recovery note was a placeholder [verify]. Sol's source citation is the better basis |

## 2. Where Sol is wrong, overbuilt or underspecified

### C1. Pricing at runtime is a second path with no authority gain (disagree)

Sol extends `/api/billing/catalog` with a public projection and adds a runtime pricing island, a version check and a "mismatch must fail publication/readiness" rule (§7).

The catalog is compiled code, not runtime data:

- `transgate/billing/src/lib.rs:113` sets `catalog: catalog::catalog()`.
- `transgate/billing/src/catalog.rs:28` is a constant constructor.
- No environment variable or database feeds it.

A site build and an app built from one commit evaluate the identical function, so the "runtime catalog is authority" premise does not hold. It adds:

- A public API contract change.
- A client repaint path whose only effect is to diverge from the indexed HTML and JSON-LD.
- A cross-image runtime readiness probe.

Smallest correct design: the billing owner's pure `public_pricing(catalog, locale)`, evaluated at site build, plus a test that rendered values equal the projector. Both images carry one release identity (P9). The catalog route stays as it is. If mu later moves the catalog to runtime configuration, a runtime read becomes necessary, and that change would bring it. **Resolution sought:** drop the runtime island and route extension, or name a concrete runtime pricing source that exists.

### C2. The serving layer is real work, not an optional HTTP adapter (disagree)

Sol makes `modules/cms/http` conditional ("only if E1/routing actually requires one") and leaves the static image as "a non-root static edge/server". Routing does require it:

- The cross-locale blog redirect space alone is **35,914 candidate rows**: every (article, locale) times the article's other distinct slugs, computed from the 159 directories, before date and collision filtering.
- Add 27 legacy redirects, the `/tools` invalid-pair rule, translation locale mismatches, a 404 status per locale, health, config injection and exact decoded-path lookup for non-ASCII slugs (raw UTF-8, up to 168 bytes per segment).

A file server cannot express 308s without a generated config of that size. Sol itself warns that a Caddy template cannot safely interpolate JSON config. The taskr consumer also needs a real served output to prove routing, not only files.

So the module needs a serve crate with real content (manifest lookup, redirect table, 404, ETag/cache, one injection marker). That is not an empty crate.

### C3. A renderer inside the module risks a template platform (partial disagree)

Sol's `modules/cms/build` holds "the chosen HTML renderer; the preferred adapter uses Askama". Askama templates compile into the crate that owns them, so Transgate's family templates cannot live in a reusable module without making that module a template host.

Agreed split:

- The module holds Markdown to a typed tree, the escaping rules, and the output writer (content-addressed files, routes, sitemap, robots).
- Each consumer owns its templates.

This also keeps E1 renderer-neutral at the module boundary.

### C4. Newsletter is not a mounted public flow (disagree on P6 scope)

Sol keeps `POST /api/subscribe` as a P6 contact or newsletter obligation (§7). The only caller is `components/LandingPage/Newsletter/Newsletter.tsx`. Its one mount is commented out (`pages/index.tsx:233` `{/* <Newsletter /> */}`), and no other `<Newsletter` exists.

API-F37 and EML-008 stay API and platform rows for their owners. P6 page parity does not require them, and P6 must not add a visible newsletter form. Contact (`/api/contact-us`, mounted on 32 baseline URLs) is required, and we agree there.

### C5. Third-party contact on page load is missing (gap in Sol)

Sol names only the YouTube transcript iframe and the analysis-example thumbnail. Two more facts belong in the privacy statement:

- The public home loads two `youtube-nocookie.com/embed/*` iframes on page load, one per breakpoint, hidden by CSS, with no consent gate (`Hero/VideoSection.tsx:5-16,95-140`).
- The caption tool result loads `i.ytimg.com` thumbnails (`YouTubeTranscriptTool.tsx:13,462`).

The existing policy (`legal/privacy-policy-en.tsx:516`) says YouTube cookies apply "when you interact with embedded YouTube videos". It also lists Google Analytics (`:268`) and Supabase storage (`:362`). Legal copy is mu's decision. P6 cannot publish it unchanged and claim accuracy.

### C6. UTM attribution and analytics locale exclusion (gap in Sol)

Source `PostHogProvider.tsx:28-31,52-53` skips PostHog for locale `id` and calls `captureUTM()` in `loaded`, independent of consent. Most campaign landings are public pages. Without capture there, attribution recorded today (`transgate_client::attribution`, key `transgate_utm_data`) is lost.

P6 needs either a reuse of the client's `attribution::capture`, or a decision to drop it.

### C7. Localization scope split (partial disagree)

Sol applies P5 D3 ("translate every reachable key, no copied English") to "tools/editorial/navigation". I agree for UI chrome and tool UI. Tab capture, for example, has hardcoded English (`tab-audio-capture.tsx:932,994-1022`).

Translating editorial and SEO body copy changes the content of indexed pages, and it adds a large, unmeasured translation load. The source catalogs are 35 MB across 16 locales. That is a scope decision for mu, not a default. Proposed default: translate UI and tool chrome under D3; preserve editorial text as source-rendered; publish the fallback manifest.

### C8. Session probe semantics (precision)

Sol says "a 401 clears stale presence". Clearing `transgate.session` from a public page can race a refresh in another tab under the client's cross-tab lock (`client/src/auth.rs:31` `LOCK_NAME`). The probe must be the p22 client's restore and verify path, with whatever storage semantics that client already has. P6 must not add its own clearing rule. The 410 and 503 handling Sol lists is right.

### C9. Sol's "Other routes 777" bucket is not classified

The gate needs the exact split, which I computed from `sitemap-0.xml`. The 777 divide as:

| Bucket | URLs |
|---|---:|
| Core | 224 |
| Persona | 336 |
| How-to | 128 |
| Root keyword | 38 |
| P7 API docs | 32 |
| P5 app routes | 15 |
| `/signin` | 1 |
| `/404` | 16 |
| Prototypes | 2 |
| Voice test | 1 |

SEO-D03 then removes exactly 34 from the sitemap: 16 + 15 + 2 + 1. `/signin` and the 32 docs stay, the docs as P7 dispositions. One app-route row, `/home/all-transcripts/all-transcripts`, matches no `route.rs` route today. The correction accounting needs these numbers, not a bucket.

### C10. Correction IDs collide by name, not meaning

Both proposals number local corrections P6-C01 and up with different meanings. Response uses a merged meaning table and leaves numbering to the lead.

## 3. Points both proposals already share (no dispute)

- Exact 4,527 baseline plus a separate approved corrected profile. A two-part gate, never a silent reduction.
- Eager finite inventory beyond the sitemap (all reachable pruned pages). No loading shells. ISR becomes rebuild-on-release.
- Blog identity is (directory, locale). Collisions and malformed dates get named dispositions, decided by the oracle's captured winners. April 31 normalizes, April 32 and 33 stay excluded.
- Product SEO policy lives in Transgate. The module receives resolved decisions.
- Real processor and caption endpoint, a core presenter leaf, File and Blob handles never copied, the source client ZIP for trim, and server `/download-zip` remains API parity.
- Contact backend required. Pricing from a billing pure projector with the id/pt floor. No module Postgres crate. A real taskr consumer. E1 measured on the full inventory, including islands.
- D12 telemetry. P7 owns docs and keys. P9 owns deploy and live. P5 D13 verified probe. D6 narrow.
