# P6 design batch review (Fable)

Reviewer `w23:p27`, anthropic/claude-fable-5-1, MEDIUM reasoning, 2026-10-26. Read `opus/response.md` (9D1C), `sol/response.md` (4DAC), the redirect rows of both critiques and `opus/proposal.md` §3.3/§3.5, PRD rows 67/82/164 and P5 D20. Three targeted source greps, cited inline. No builds, runs, edits outside this file, or agents. Nothing here authorizes implementation.

## Verdict

**Architecturally sound and complete as a design; no genuine design obligation is missing.** What remains is (a) named lead/mu decisions, (b) one sequencing dependency the lead must make explicit, (c) one evidence-precision correction in the Opus response, and (d) seam owners not yet assigned. Both advisors converge; the convergence is substantive, not papered over.

## Architecture against PRD and D20

| Requirement | Converged position | Judgement |
|---|---|---|
| PRD 67: `modules/cms`, Markdown in git → static HTML, sitemap, hreflang, 16 locales, framework by E1 | `cms-core`: typed Markdown tree, escaping, content-addressed writer, route/sitemap/robots serialization. `cms-http`: manifest lookup, redirect rules, per-locale real 404, GET/HEAD, validators, config injection, health. Consumers own templates and renderer | Sound. Module boundary is renderer-neutral, so E1 is not pre-decided |
| PRD 82 E1 criteria and fairness | Run E1 **before** all-family template commitment; Astro, SvelteKit prerender, Rust/Askama all eligible; measure the three PRD pages in two locales plus full varied direct-route output under two minutes, token sourcing, island packaging, Lighthouse SEO 100; tie rule only after measurement | Sound. **Dependency not stated as a step:** "full real varied inventory" means S0 oracle capture and the framework-neutral content import (Markdown, metadata, redirect data) must land before E1 runs. Templates stay per-candidate; content does not. Lead should sequence S0 → content import → E1 explicitly |
| Real taskr consumer | `taskr-site` runs the generator and serves two-locale page, blog index/article, one redirect, sitemap, hreflang, navigation | Correct bar; Cargo reachability rejected as proof by both |
| PRD 164 gate: page count equals today's sitemap | Two mandatory parts: (1) exact immutable 4,527 reconciliation with zero unexplained differences plus every direct-route/content/asset disposition; (2) target after only approved per-URL changes equals emitted sitemap and packaged route behavior; machine-readable old→new diff | Sound and stricter than PRD; keep both parts. 777 bucket split is source-reported and must be reproduced by the S0 importer. 35,914 redirect candidates and ~10.6k output are estimates, not accepted counts |
| Pricing from the billing catalog | Build-time pure billing projector (absent today, R confirmed); digest binds effective catalog value + projector/rule version; paired site/app release and rollback as a P9 gate; catalog route unchanged; schema offers bound to projector (fixes 13.99/20.99 vs 14/21) | Sound. Projector is a real new billing seam needing an owner |
| Bounded fallbacks | Three unbounded source behaviours (unknown reverse language fabrication, unknown language slug → English, unknown certificate soft 200) need lead/mu correction (404 recommended) or a bounded runtime resolver; bounded product rules for configured-language case variants (lowercase captures, then lookup), invalid `/tools` pair → `/`, translation locale mismatch → locale `/translation` | Sound. Cross-locale blog redirects: a lookup over the article identity set is the same semantics as Opus's ~35.9k-row table with no enumeration; implementation choice, not a disagreement |
| Contact and capture lifecycles | Anonymous `/api/contact-us` host adapter: 405 non-POST, operator mail then acknowledgement, 200 only after both, 500 on either; visible failure is a named correction. Capture: stale-effect/session fencing, reset/dispose in every async phase, local abort ≠ remote invalidation, conversion-pending state, guaranteed resource release; recovery promised only after S0 captures mounted call sites | Sound. Note: no abuse control on an anonymous mail endpoint is source parity; both advisors deliberately added none. Lead should acknowledge the risk explicitly rather than inherit it silently |
| Auth upstream and config | Edge routes `/auth/v1/*` to the real GoTrue upstream or configured external auth; no Axum proxy. Injected config has a published schema, validated public origins only, escaping `< > & U+2028 U+2029`; CSP derives origins from validated config, `'wasm-unsafe-eval'` as P5; HTML ETag over injected bytes or omitted | Sound |
| D20 | `/_app/*` app-owned; public `/assets/*` preserved including unreferenced files with dispositions; no alias or catch-all | Respected; treated as closed |
| Locale / legal / P7 / P9 | Public UI and tool-chrome keys complete in 16 locales; editorial/SEO prose preserved with fallback manifest; legal text is mu's; P7 owns 32 docs URLs and keys; P9 owns edge/DNS/TLS/trusted proxies/catalog pairing/cutover | Correctly named as dependencies, not P6 claims |

## Evidence-precision findings

| # | Severity | Finding | Evidence | Correction |
|---|---|---|---|---|
| 1 | Medium | Opus response row 18 says 308/307 are "oracle-confirmed". No oracle has run; Opus's own critique A7 wrote "[INFERENCE, oracle confirms]" as a future step, and Sol §2 states the classes await the authorized oracle | Source mechanism is Next data-fetching `redirect` objects: `translation/[...slug].tsx:625-627,718-720` `permanent:false`; `tools/[keywords].tsx:42-44` `permanent:true`; `blog/[[...articleSlug]].tsx:85-87` `permanent:true`. Next documents 307/308 for these, so the **class** is framework-grounded, but exact status, `Location` encoding and the legacy `legacyBlogSlugRedirects.js` config path (not checked by me) remain S0 HTTP-capture obligations | Lead records statuses as *expected pending S0*; amend row 18 wording. No design change |
| 2 | Low | Opus §2 "about 35.9k redirect candidate rows" is presented as `cms-http` content | Sol correctly labels it peer source-file analysis before date/collision filtering | Do not carry the number into any gate; S0 produces the real set |
| 3 | Low | Opus §1 "Sol accepted C1 to C9 … no genuine remaining disagreement" | Sol §1 confirms each; I found no residual contradiction between the two responses | None |

## Remaining decisions (exact)

**mu:** SEO-D02 per-URL `/tools` indexing policy; SEO-D04 unknown language slug 404; SEO-D05 alternates/canonical intent; SEO-D06 collision losers unreachable (default) vs new slugs; legal privacy text; capability/translation truth claims (LNG-D10); consent-gating the landing embeds; any editorial prose translation beyond UI keys.

**lead:** SEO-D01 (−11) and SEO-D03 (−34) sitemap removals; 404 for unknown reverse language and unknown certificate vs bounded runtime resolver; server-built keyword head/body; schema prices bound to projector; sitemap `<loc>` percent-encoding; JSON-LD date normalization; one embed per viewport; microphone permission on gesture with guaranteed release; visible contact failure; submitted-HTML escaping in contact mail; build-as-of publication replacing ISR; omit GA/Hotjar/NR on public pages; keep `id` PostHog exclusion; responsive single static template convention exception; confirm UI-keys-vs-prose localization split; acknowledge the anonymous contact abuse-control risk.

**sequencing/ownership (lead):** S0 oracle and content import before E1; owners for the billing projector, contact host adapter, p22 auth/probe facade, cms-core/cms-http, taskr consumer, and the independent E1/test owner.

No implementation is authorized by this review.
