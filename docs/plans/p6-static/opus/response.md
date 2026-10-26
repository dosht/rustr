# P6 Opus response after cross-critique

2026-10-26. This response answers `sol/critique.md` and records convergence with Sol. It supersedes `opus/proposal.md` only where a row below says so explicitly. Everything else in the proposal stands. Nothing here is lead approval or acceptance. No builds, checks, browser runs, providers or new agents. P5 D20 (`/_app/*` for the app, public `/assets/*` preserved) is decided and closed.

## 1. Accepted from Sol (supersedes my proposal)

| Meaning | Supersedes | Converged position |
|---|---|---|
| **Unbounded source fallbacks** | Proposal §0.2, §3.5 ("every URL is enumerable"; "unknown translation is already 404") | Three routes render for unbounded input:<br>• Reverse translation fabricates metadata and a testimonial for any `[a-z]+` language, matched case-insensitively (`translation/[...slug].tsx:105-115,654-743`).<br>• The language matrix normalizes any slug to the English default (SEO-D04).<br>• A certificate with any id returns a 200 "not found" page.<br>Forward translation also matches case-insensitively, so case variants of valid slugs render [owner capture]. A finite static cutover therefore **requires named corrections** for these spaces. If the lead declines them, a bounded runtime resolver with the same semantics is needed. Silent 404s are not parity |
| **Bounded route rules beside the finite inventory** (final precision from lead relay) | Proposal §3.5 and the case-variant wording above | `detectPageType` (`translation/[...slug].tsx:85-114`) matches both translation patterns with `/i` and lowercases the captures. So a case variant of a **configured** language renders the same 200 page with the same canonical. Correct handling is a source-compatible product rule lookup (lowercase the captures, then look up the configured language): not a global case fold, and not an unapproved 308.<br>`/tools/[keywords].tsx:36-45` redirects any invalid single-segment keyword/locale pair to `/` with a permanent redirect. That is a rule, not a list of exact rows.<br>So `transgate-site` serves the finite pre-rendered HTML inventory **plus** a short, product-owned rule list: translation lowercase-capture lookup to an existing page, invalid `/tools/{x}` to `/`, cross-locale blog and legacy redirects. It has no dynamic content renderer. Unknown reverse languages still need an explicit correction. Every other unknown path is a real 404. The rules and their source lines live in Transgate site policy, never in `modules/cms` |
| **Client-only keyword head and body** | §2.2, G0 | `KeywordsComponent.tsx:61-68` returns null before hydration, so the server HTML for 2,208 root-keyword and 138 `/tools` keyword pages is empty. The oracle records the hydrated DOM for these families. Server-built head and body is a named correction |
| **Oracle pinning and winner rules** | §3.2, G0, SEO-D06 row | Pin the source snapshot hash, Node version, TZ, a frozen clock and the observed directory and file traversal order. Capture raw input records separately from the route-selected identity, redirect and body.<br>`resolveArticle` takes the **first article whose any-locale slug equals the request** (`blog/[[...articleSlug]].tsx:138-158`), so winners are defined per requested URL, not only by the 4 same-locale collisions. Also capture metadata-language overwrite and substring filename selection (`:177-207`).<br>April 31 follows the JavaScript date result; April 32 and 33 stay excluded. **No new suffixed slug for a collision loser without mu's approval**. The default is the oracle winner, with the loser recorded as unreachable. The importer and oracle stay in the repository until the final cutover report |
| **Two-part URL gate, both required** | §5 ("passes when 1 **or** 2") | **Both** parts are required: (1) exact immutable 4,527 baseline reconciliation plus complete direct-route dispositions, and (2) exact deployed sitemap and output after only approved per-URL changes. Packaged routing proof is separate and also required. The 32 P7 docs and 15 app, 1 sign-in, 16 `/404`, 2 prototype and 1 voice dispositions stay explicit |
| **Pricing digest contract** | §3.9 | Build-time pure billing projector, as both proposals now agree. The output digest binds the **effective catalog value plus the projector and rule version**, not a git tag, with paired publication and rollback of site and app (P9). If that pairing cannot be enforced, drift prevention stays an open P9 gate rather than being claimed |
| **Contact semantics** | §2.3 | The source `contact-us.ts:12-28`:<br>• answers 405 for any non-POST method<br>• awaits the operator mail, then the acknowledgement<br>• returns 200 only after both, 500 on either failure<br>The client navigates to success only after 200; the visible failure state is a named correction (the source only logs). Submitted-HTML escaping is a lead decision. No captcha, idempotency or outbox additions |
| **Tool and capture lifecycles** | §3.10 | Add:<br>• stale-effect, file and session fencing<br>• reset or dispose during upload, process and download<br>• local abort is distinct from remote invalidation<br>• conversion pending before a recording is downloadable<br>• guaranteed release of tracks, AudioContext, timers and object URLs<br>Recorder cleanup today happens only after successful WAV conversion (`record-audio-online.tsx:241-251`). Tab recovery serializes Blobs to base64 in localStorage and tolerates quota failure; its one-hour validity is not a duration cap. Capture the mounted call sites before promising recovery. Convert audio is synchronous (200). Permission on user gesture plus guaranteed cleanup is a named correction |
| **CSP, ETag, auth upstream** | §3.8, §3.1 `cms-http` | `script-src` needs `'wasm-unsafe-eval'`, as P5 uses (`web.rs:106-110`). Origins are derived from the validated config (processor, auth, telemetry), `blob:` for media.<br>HTML ETags hash the **served bytes after injection**, or HTML omits the ETag; HEAD length matches the served bytes.<br>`/auth/v1/*` routes to the actual GoTrue upstream or the configured external auth URL. The app has no GoTrue proxy, so it never goes to Axum. Injection schema and escaping are a published contract |
| **Redirect statuses** | §2.2, §3.5 | Expected Next.js defaults: 308 for blog cross-locale, legacy blog and `/tools` invalid pairs; 307 for translation locale mismatch. No oracle has run. Exact status, Location encoding and the legacy config path remain S0 HTTP-capture obligations. Lead corrected the evidence wording after the Fable MEDIUM review; the routing proposal is unchanged |
| **Homepage desktop sign-in popup** | §2.3 missing | `SimpleSignInPopup` (`_app.tsx:67,73`; `SimpleSignInPopup.tsx:16-48`) shows on `/` only, on desktop only (hidden at `innerWidth <= 768` or on a mobile UA), after 2 s, and starts Google OAuth to `/home`. Port it through a narrow p22 auth facade over the P5 GoTrue PKCE client, with P5 UI-03 suppression after a verified session |
| **Structured-data price truth** | §3.4 | `SchemaComponent.tsx:109-236` hardcodes offer prices of 13.99 and 20.99 against catalog prices of 14 and 21. Schema prices bind to the projector, as a named correction. Unsupported capability and translation-language claims (LNG-D10) go to lead review |
| **E1 order and fairness** | §3.7 | Run E1 **before** the all-family template migration. Use real varied pages, the full direct-route output and island packaging. Astro is measured, not pre-disqualified; the tie rule applies only after the measured criteria. My Rust prediction is not a decision |
| **Taskr proof** | §3.1 | `taskr-site` must actually run its generator and serve: a two-locale page, blog index and article, one redirect, sitemap, hreflang and navigation. Meeting the cargo graph is not proof |
| **Evidence strength** | G0 to G3 | Every family records its evidence class (rendered oracle, hydrated oracle, or configuration evaluation). A gap stays named and never promotes G1 or G3 |

## 2. Converged after Sol accepted my critique points

| Meaning | Position |
|---|---|
| Pricing runtime path | Sol accepts build-only pricing (its R04). The compiled catalog (`transgate/billing/src/lib.rs:113`, `catalog.rs:28`) means no runtime island and no catalog route extension. The route stays unchanged |
| Serve crate | Sol accepts a concrete reusable `cms-http` (manifest lookup, about 35.9k cross-locale blog redirect candidate rows, legacy 308s, per-locale 404, injection, health). It is real content, not an empty crate |
| Landing embeds and privacy | Sol adopts the two on-load `youtube-nocookie` iframes plus the `i.ytimg` thumbnails. One iframe per viewport is a named correction; consent-gating the embeds is a separate decision. The legal text is mu's |
| Public assets | Unreferenced public URLs are preserved with explicit excluded, dormant and license dispositions. P5 document fonts and logo are not overwritten |

Sol accepted C1 to C9 in direct exchange on 2026-10-26, so there is no genuine remaining disagreement between the advisors. §3 records the resulting positions.

## 3. Resolved by direct exchange (Sol accepted)

- **C3, renderer location.** The module holds Markdown to a typed tree, escaping, the content-addressed output writer, sitemap, robots and `cms-http`. Consumers own their templates. This keeps E1 neutral at the module boundary.
- **C4, newsletter.** Both agree the `/api/subscribe` API and EML-008 rows stay required for their API and platform owner. Its only UI mount is commented out (`index.tsx:233`), so P6 adds no visible form and does not count it as a P6 page flow.
- **C6, UTM and analytics `id` exclusion.** Reuse `transgate_client::attribution::capture` and its storage key, independent of telemetry consent, loaded lazily only when `utm_*` parameters are present. No second SDK. The `id` PostHog exclusion is preserved until an explicit decision under D12 says otherwise.
- **C7, localization.** Complete the public UI and tool-chrome keys in all 16 locales. Preserve editorial and SEO prose as rendered by the source, with a fallback manifest. Any broader translation is mu's decision. P5 D3 does not silently authorize rewriting P6 prose.
- **C8, probe.** p22 owns the probe, refresh, storage and 401 fencing. P6 adds no storage-clearing rule of its own. 410 and 503 keep their distinct P5 handling.
- **C9, accounting.** The exact 777 split (§1 of my critique) is the gate basis. Removing all SEO-D03 entries takes out 34.

## 4. Merged correction table by meaning (numbering left to lead)

| Meaning | Source | Recommendation | Count effect | Decider |
|---|---|---|---|---|
| Sitemap lists 11 noindex translation keyword pages | SEO-D01 | Drop them from the sitemap; the pages remain | −11 | lead |
| `/tools/{kw}` indexing lookup omits `/tools/` | SEO-D02 | Route-identity policy. Traffic-earning paths self-canonical and indexable; the rest noindex and out of the sitemap. Per-URL artifact | −(138 − traffic hits) | mu |
| `/404`, app, prototype and voice entries in the sitemap | SEO-D03 | Drop 34; keep `/signin` and the P7 docs | −34 | lead |
| Unknown language slug renders the English default | SEO-D04 | 404 | 0 | mu |
| Forward translation hreflang names non-UI languages; canonical intent | SEO-D05 | Alternates only to real equivalent 200 pages; decide canonical intent explicitly | 0 | mu |
| Blog collisions, metadata-less files, malformed dates | SEO-D06 | Oracle winners keep the URL; losers unreachable unless mu approves new slugs; JavaScript-date normalization | 0 by default | mu |
| Reverse translation fabrication for unknown languages | new | Configured routes only; unknown reverse language is 404. Case variants of configured slugs keep the source 200 through the bounded rule lookup (not a correction) | 0 | lead |
| Unknown certificate id renders 200 | new | 404 for unknown; 3 ids × 16 eager | 0 | lead |
| Client-only keyword head and body | new | Server-built head and body | 0 | lead |
| Schema offers 13.99 and 20.99 | new | Bind to the projector (14 and 21) | 0 | lead |
| Sitemap `<loc>` raw UTF-8 | new | Percent-encode; compare decoded | 0 | lead |
| Blog JSON-LD `datePublished` as the raw directory name | new | Normalized ISO date | 0 | lead |
| Two hidden or duplicate landing iframes | new | One per viewport | 0 | lead |
| Microphone permission on load, stream leak, cleanup only on success | new | User-gesture permission, guaranteed release | 0 | lead |
| Contact failure logged only | new | Visible failure and retry | 0 | lead |
| Cookie-presence redirect at `/` | P5 D13 | Verified probe | 0 | decided |
| ISR request-time publication | new | Build-time publication at an explicit as-of instant | 0 today | lead |
| Privacy policy wrong about GA, Supabase and YouTube | legal | mu rewrites the text; no P6 edit | 0 | mu |
| GA, Hotjar and NR on public pages | D12 extension | Omit | 0 | lead |
| English fallback keys in public UI | locale | Converged: complete public UI and tool-chrome keys in all 16 locales; editorial and SEO prose preserved with a fallback manifest | 0 | lead confirms; broader prose translation is mu's |
| Desktop and phone view convention for static content | convention | Responsive single template for static pages; islands keep both views | 0 | lead |

## 5. Remaining decisions for lead or mu

1. The per-URL SEO-D01 to D06 dispositions, giving the corrected target profile.
2. Restricting the unbounded source fallbacks (unknown reverse language, unknown language slug, unknown certificate), or authorizing a bounded runtime resolver instead. Configured-language case variants and invalid `/tools` pairs keep their source outcomes through bounded product rules.
3. Collision losers: unreachable, which is the default, or new slugs.
4. Confirm the converged localization split: public UI keys completed, editorial prose preserved. Any broader prose translation is mu's.
5. Escaping submitted contact HTML.
6. The legal privacy text, schema and capability claims (LNG-D10), and the treatment of the landing embeds.
7. Whether to keep the `id` analytics exclusion; UTM capture runs outside consent, as in the source.

These remain explicit gates: P7 docs and keys, P9 edge, DNS, TLS, trusted proxies and paired catalog release, and live YouTube, mail, capture device and search evidence.
