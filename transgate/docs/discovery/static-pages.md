# Static pages

The public site uses Next.js pages-router templates and Markdown blog content across 16 locales.
Source-derived sitemap candidates reconcile exactly to the checked-in 4,527-URL sitemap after filtering and URL normalization.
Public media tools require runtime services; they are not all static content or browser-only processing.

## Inventory

Paths below are relative to Transgate. Counts were computed read-only from page files, exported configuration and article metadata on 2026-10-24. No build or production request was run. There are 277 non-API TSX page files, including 190 at the page root; these include app routes, framework wrappers and dynamic generators, so they are not the public URL count.

### Public route families

| Family | Source count and locales | Metadata and sitemap treatment | Source |
|---|---|---|---|
| Root keyword landing pages | 138 files: English 46, French 20, Spanish 17, Indonesian 16, Italian 14, Portuguese 13, Greek 12. Each has 16 locale candidates | 2,208 candidates minus 2,170 exclusions = 38 retained locale/slug pairs. Curated keepers and traffic exceptions; other pages noindex/follow and cluster canonical | `frontend/src/config/seoIndexing.ts:27-79,245-272`; `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-54`; `frontend/next-sitemap.config.js:24-165,553-562` |
| Editorial how-to pages | Eight root `how-to-convert-*` and `how-to-transcribe-*` files, covering audio/speech/video/voice; 16 locales | Shared Howto content, not the pruned root-keyword template | `frontend/src/pages/how-to-transcribe-audio-to-text.tsx:1`; `frontend/src/pages/how-to-convert-audio-to-text.tsx:1` |
| Persona and business pages | students, engineers, teachers, journalists, transcriptionists, consultants, public-speakers, business, sales-representatives, writers, podcasters, content-creators, coaches, doctors, marketers, administrative-professionals, hr-professionals, researchers, therapists, lawyers; plus sales-teams | 21 root routes, translated templates; preserve content and CTA, not claims of separate app features | `frontend/src/pages/students.tsx:1`; `frontend/src/pages/business.tsx:1`; `frontend/src/pages/sales-teams.tsx:1` |
| Core public pages | `/`, aboutus, what-is-transgate, why-transgate, how-to-use, frequently-asked-questions, supported-languages, pricing, transcription, translation, notetaker, subtitle-generator, video-to-transcript, youtube-video-analysis | 14 named routes, generally 16 locales. Per-page SEO/schema, not a uniform global schema. Default robots is index/follow | `frontend/src/pages/_app.tsx:43-50`; `frontend/src/pages/pricing.tsx:23-94`; `frontend/src/pages/youtube-video-analysis.tsx:1` |
| Features | 45 page files: 10 at features root, 8 ai-meeting-summarization children, 8 ai-transcription children plus 1 comparison, 9 highlights children, 9 chat children | 705 sitemap URLs: 45 English plus 44 x 15 other locales. Includes feature/FAQ/how-to/comparison content; use the existing page-specific JSON-LD, not fabricated capabilities | `frontend/src/pages/features/index.tsx:1`; `frontend/src/pages/features/ai-transcription/comparison/otter-vs-transgate.tsx:1`; `frontend/src/config/features.ts:1` |
| Language matrix | 226 configured slugs, 3,616 possible locale pairs; no eager static paths, blocking generation | Only 112 traffic-earning URLs injected into sitemap. Indexable localized URLs self-canonical; others canonical to English and noindex | `frontend/src/config/languages.ts:1-228`; `frontend/src/pages/languages/[slug].tsx:18-24,57-87`; `frontend/src/components/Language/language.tsx:165-171,229-235`; `frontend/next-sitemap.config.js:543-550,682-686` |
| Translation matrix and keywords | 51 forward languages x 2 file types, plus matching UI-language variants; reverse list has 56 languages x 2, not the comment's 57; 16 English keyword pages | 40 sitemap URLs under `/translation/`. Matrix uses traffic allowlist; keyword rendering indexes only five curated slugs. English canonical, forward hreflang English plus source language, reverse English only; SoftwareApplication and keyword HowTo | `frontend/src/pages/translation/[...slug].tsx:149-237,265-352,373-474,489-580`; `frontend/src/config/seoIndexing.ts:283-298` |
| `/tools/{keywords}` | 138 localized keyword routes: de 84, tr 20, kk 34 | Generator validates locale/slug, invalid requests permanently redirect to `/`; template uses KeywordPageIndexing, creating the mismatch recorded below | `frontend/src/domain/types/keywords.ts:2-147`; `frontend/src/pages/tools/[keywords].tsx:21-57`; `frontend/src/components/KeywordsComponent/KeywordsComponent.tsx:6` |
| Public tools | Three explicit tools: youtube-transcript-generator, record-audio-online, tab-audio-capture | 48 explicit-tool locale URLs plus 138 keyword tools = 186 sitemap URLs. Runtime behavior in R4 | `frontend/src/pages/tools/youtube-transcript-generator.tsx:14-53`; `frontend/src/pages/tools/record-audio-online.tsx:195-230`; `frontend/src/pages/tools/tab-audio-capture.tsx:168-171` |
| Free tools | Ten routes: index, trim-audio, audio-trimmer, video-trimmer, convert-audio, reduce-audio, remove-noise, video-to-audio, compress-video, convert-video | 160 sitemap URLs, public templates with interactive runtime tools. Three trimming pages share the trim processor | `frontend/src/pages/free-tools/index.tsx:1`; `frontend/src/pages/free-tools/audio-trimmer.tsx:1`; `frontend/src/pages/free-tools/video-trimmer.tsx:1`; processor table in R4 |
| Blog | 159 dated article directories; JavaScript date filter includes 157, excludes 2025-04-32 and 2025-04-33. 2025-04-31 normalizes to May 1 and is included | 2,508 sitemap URLs, including 16 indexes and a separate English editorial page. File count differs from distinct URL count because of missing locale files, metadata languages and duplicate slugs | `frontend/src/pages/blog/[[...articleSlug]].tsx:119-158,177-231`; `frontend/src/domain/blog.ts:625-636`; `frontend/src/pages/blog/how-to-use-ai-meeting-summarization.tsx:1` |
| Legal | index, privacy-policy-en, privacy-policy-tr, terms-and-conditions-en, terms-and-conditions-tr | Five English-prefixed-route sitemap entries; suffix indicates document language, not sixteen translated policies | `frontend/src/pages/legal/index.tsx:1`; `frontend/src/pages/legal/privacy-policy-en.tsx:1`; `frontend/src/pages/legal/terms-and-conditions-tr.tsx:1` |
| Contact and certificates | contact, contact/success; certificate ids evan and bisan pregenerated in English, fallback true | 32 contact URLs and 2 certificate URLs. Certificate output includes locale canonical/alternates but only two eager paths | `frontend/src/pages/contact/index.tsx:1`; `frontend/src/pages/contact/success.tsx:1`; `frontend/src/pages/certification/[id].tsx:84-110,150-172` |
| Auth, API docs, errors, app routes and prototypes | Covered in screens.md | Some are in the sitemap despite not being public content; see defect SEO-D03 | `frontend/public/sitemap-0.xml`; `frontend/next-sitemap.config.js:603-621` |

### Blog and pricing details

Articles live at `frontend/public/articles/{date}/{locale}.md`. `<+++>` separates body from numbered metadata. `Language` and `4` select locale and slug; `1`, `2`, `3` provide title/description/long description. Missing requested locale returns notFound, not an English-content fallback. A known slug from another locale redirects permanently to the requested locale's slug. Paths are `/blog/{slug}` in English and `/{locale}/blog/{slug}` otherwise. Sources: `frontend/src/pages/blog/[[...articleSlug]].tsx:46-115,162-225`; `frontend/src/domain/blog.ts:625-636`.

The blog index sorts newest first and renders the full list, with no pagination. BlogPost derives the social/schema image from the first content image, falling back to the homepage thumbnail; author images and Article/FAQ structured data are explicit. Sources: `frontend/src/components/BlogList/BlogList.tsx:64-118`; `frontend/src/components/BlogPost/BlogPost.tsx:17-28,79-127`; `frontend/src/components/BlogPost/FAQ.tsx:42-48`.

Pricing shows PAY AS YOU GO, PREMIUM and BUSINESS, a monthly/yearly toggle and free-trial CTA. Values come from `findPackage` and `localeBasedDisctount` in domain/packages, not from translation strings or a pricing-page database query. Displayed card amounts use `$`; package labels/descriptions are translated. Cards show original and discounted amounts, hours and expiry. Public CTA goes to sign-in; the signed-in billing modal and checkout own purchasing. Sources: `frontend/src/components/LandingPage/Pricing/Pricing.tsx:10-19,34-57,85-94`; `frontend/src/components/LandingPage/PricingCard/PricingCard.tsx:1,13-51,86-114`. Billing topic owns actual currencies and monetary rules.

## Rules

| ID | Rule | Source | pgTAP | Rust test | Status |
|---|---|---|---|---|---|
| SEO-001 | English is the default unprefixed locale; the configured UI/site locales are en,tr,pt,es,de,id,it,fr,el,ru,da,sv,no,fi,kk,az | `frontend/next.config.js:21-24` | none | tbd | ledger |
| SEO-002 | Root keyword pages index only in their curated locale, unless their locale-qualified path earns traffic | `frontend/src/config/seoIndexing.ts:245-251` | none | tbd | ledger |
| SEO-003 | Retained keyword pages self-canonicalize; pruned pages use the slug-language editorial or cluster canonical | `frontend/src/config/seoIndexing.ts:254-272` | none | tbd | ledger |
| SEO-004 | Keyword templates emit noindex/follow when pruned; hreflang only when indexable, using slug locale and English x-default | `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-54` | none | tbd | ledger |
| SEO-005 | Language matrix indexing depends on traffic; retained localized pages self-canonicalize, other matrix pages canonicalize to English | `frontend/src/config/seoIndexing.ts:283-285`; `frontend/src/components/Language/language.tsx:165-171,229-235` | none | tbd | ledger |
| SEO-006 | Language pages generate on demand with blocking fallback; invalid slugs currently normalize to the default English-audio slug before validation | `frontend/src/pages/languages/[slug].tsx:18-24,57-87` | none | tbd | ledger |
| SEO-007 | Translation matrix indexing depends on traffic; generated forward pages include English and matching configured UI locale, reverse pages generate English | `frontend/src/config/seoIndexing.ts:287-289`; `frontend/src/pages/translation/[...slug].tsx:489-555` | none | tbd | ledger |
| SEO-008 | Translation keyword rendering indexes only five curated English slugs; translate-into-voice and audio-translation-online canonicalize to voice-translator-online | `frontend/src/config/seoIndexing.ts:291-298`; `frontend/src/pages/translation/[...slug].tsx:373-381,451-459` | none | tbd | ledger |
| SEO-009 | Tool keyword generation uses de/tr/kk lists; invalid locale/keyword pairs permanently redirect home | `frontend/src/pages/tools/[keywords].tsx:21-57`; `frontend/src/domain/types/keywords.ts:155-158` | none | tbd | ledger |
| SEO-010 | Blog publication uses JavaScript date comparison against now; missing locale/body is notFound and cross-locale slugs redirect permanently | `frontend/src/pages/blog/[[...articleSlug]].tsx:62-100,151-158` | none | tbd | ledger |
| SEO-011 | Blog successful pages and index revalidate after 31536000 seconds; blog links use localized metadata slugs | `frontend/src/pages/blog/[[...articleSlug]].tsx:46-58,109-115`; `frontend/src/domain/blog.ts:56-58` | none | tbd | ledger |
| SEO-012 | Blog index renders all available requested-locale articles newest first; Article images use first content image or default thumbnail | `frontend/src/components/BlogList/BlogList.tsx:64-118`; `frontend/src/components/BlogPost/BlogPost.tsx:79-107` | none | tbd | ledger |
| SEO-013 | Sitemap removes pruned root pairs and non-earning matrix paths, adds earning language paths, deduplicates full URLs and splits at 7000 | `frontend/next-sitemap.config.js:553-562,603-621,659-686` | none | tbd | ledger |
| SEO-014 | Sitemap uses weekly frequency, default priority 0.7, transcription and keyword-translation 0.9, forward translation 0.8 | `frontend/next-sitemap.config.js:623-655` | none | tbd | ledger |
| SEO-015 | Legacy malformed blog slug redirects are permanent and locale-exact; unchanged mappings are omitted | `frontend/config/legacyBlogSlugRedirects.js:45-52`; `frontend/next.config.js:51-55` | none | tbd | ledger |
| SEO-016 | Robots allows crawling of all paths and points to the sitemap index; noindex is a page metadata decision | `frontend/public/robots.txt:1-9`; `frontend/src/pages/_app.tsx:43-50` | none | tbd | ledger |

## Defects

| ID | Current behavior | Intended behavior | Proposal |
|---|---|---|---|
| SEO-D01 | All 16 translation keyword paths survive sitemap transform but only five render indexable (`frontend/next-sitemap.config.js:618-650`; `frontend/src/pages/translation/[...slug].tsx:451-459`) | Sitemap advertises only intended indexed URLs | Mu approves reconciling 11 noindex paths; preserve baseline count separately |
| SEO-D02 | `/tools/{keywords}` uses root KeywordPageIndexing, whose lookup omits `/tools/`; sitemap excludes only root keyword paths (`frontend/src/components/KeywordsComponent/KeywordsComponent.tsx:6`; `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-29`; `frontend/src/config/seoIndexing.ts:249`; `frontend/next-sitemap.config.js:553-562`) | Route-specific indexing and canonical decisions | Record mismatch; do not silently prune the 138 localized tool URLs |
| SEO-D03 | Sitemap contains 404 in all locales, signed-in dashboard and home-list routes, checkout/confirm and prototypes (`frontend/public/sitemap-0.xml`; exclusion logic `frontend/next-sitemap.config.js:603-621`) | Indexable public content only | Keep 4527 as observed baseline; obtain explicit corrected target before P6 gate |
| SEO-D04 | Unsupported language slug is replaced by default before the notFound check (`frontend/src/pages/languages/[slug].tsx:18-24,57-63`) | Unknown routes should not impersonate a valid page | Mu decides redirect versus 404 |
| SEO-D05 | Forward translation advertises the source-language hreflang even when that language is not a configured UI locale, and localized matrix pages canonicalize to English (`frontend/src/pages/translation/[...slug].tsx:157-163,273-278`) | Alternates resolve and canonical intent is deliberate | Review locale support and retained localized traffic before changing |
| SEO-D06 | Blog date parser accepts April 31 and several locale metadata entries share URL slugs; file count is not URL count (`frontend/src/pages/blog/[[...articleSlug]].tsx:153,192-203`) | Stable migration identity without lost content | Preserve source directories and current URLs; resolve collisions explicitly during content migration |

## Keep / throw

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Home, editorial, personas, features, FAQ, legal and contact copy | `frontend/src/pages/pricing.tsx:1`; `frontend/src/config/features.ts:1`; route inventory above | port | Markdown content plus reusable data templates; preserve URLs and locale coverage | modules/cms |
| Root keyword, language and translation matrices | `frontend/src/config/seoIndexing.ts:245-298` | keep-knowledge | Keep intentional pruning, traffic exceptions and redirect/canonical lessons, not duplicate React pages | CMS URL manifest and templates |
| Blog Markdown, images and metadata | `frontend/src/pages/blog/[[...articleSlug]].tsx:162-225` | port | Existing git content fits CMS; migrate metadata without losing slugs | modules/cms |
| Pricing | `frontend/src/components/LandingPage/PricingCard/PricingCard.tsx:29-37` | simplify | Template reads billing catalog; no second price authority | CMS plus billing catalog |
| trim-audio, audio-trimmer and video-trimmer | `frontend/src/utils/free-tools/trimAudioServer.ts:60-132` | port | Shared trim UI, runtime processor, files and downloads | Public interactive shell plus runtime service |
| convert-audio | `frontend/src/utils/free-tools/convertAudioServer.ts:37-64` | port | Real conversion, not Markdown | Public interactive shell plus runtime service |
| reduce-audio | `frontend/src/utils/free-tools/reduceAudioServer.ts:37-73` | port | Real compression and progress | Public interactive shell plus runtime service |
| remove-noise | `frontend/src/utils/free-tools/removeNoiseServer.ts:34-76` | port | Real denoise and progress | Public interactive shell plus runtime service |
| video-to-audio | `frontend/src/utils/free-tools/videoToAudioServer.ts:40-89` | port | Real extraction and progress | Public interactive shell plus runtime service |
| compress-video | `frontend/src/utils/free-tools/compressVideoServer.ts:33-77` | port | Real compression and progress | Public interactive shell plus runtime service |
| convert-video | `frontend/src/utils/free-tools/convertVideoServer.ts:44-96` | port | Real conversion and progress | Public interactive shell plus runtime service |
| Recorder and tab capture | `frontend/src/pages/tools/record-audio-online.tsx:195-230`; `frontend/src/components/TabAudioCapture/DownloadControls.tsx:59-96` | port | Browser capture/download islands, not transcription jobs | Public web shell |
| Free YouTube captions | `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:136-139` | port | Public endpoint distinct from paid transcription | Public web plus API adapter |
| Voice experiment | `frontend/src/pages/voice-agent-test.tsx:3-10,28-31`; `frontend/src/features/voice-demo/hooks/usePersonaPlex.ts:147-162` | gap | Separate websocket voice product; presumed excluded by PRD R4 | No port unless mu changes scope |
| Generated documentation site | `frontend/site/index.html`; `frontend/site/arc42/index.html` | keep-knowledge | Architecture documentation, not the production public page router | Reference material only |

## Parity rows

SEO obligations apply to web-desktop and web-phone. CLI/TUI/native apps may open these public URLs but do not need cloned SEO pages or browser-only recording widgets. Current authenticated source/transcription parity is in screens.md and jobs/API topics.

| ID | Flow | Surface | Today | Test | Status |
|---|---|---|---|---|---|
| SEO-P001 | Public home, editorial, personas and feature navigation | web-desktop | Route families and counts above; `frontend/src/pages/index.tsx:1`; `frontend/src/config/features.ts:1` | tbd | inventoried |
| SEO-P002 | Public home, editorial, personas and feature navigation | web-phone | Same content with responsive layout; `frontend/src/pages/index.tsx:1`; `frontend/src/config/features.ts:1` | tbd | inventoried |
| SEO-P003 | Root keyword and language matrix metadata | web-desktop | Indexing, canonical and hreflang rules SEO-002 to SEO-006; `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-54` | tbd | inventoried |
| SEO-P004 | Root keyword and language matrix metadata | web-phone | Same indexability and content; `frontend/src/components/SEO/KeywordPageIndexing.tsx:26-54` | tbd | inventoried |
| SEO-P005 | Translation matrix and keyword pages | web-desktop | Forward/reverse/keyword templates and locale redirects; `frontend/src/pages/translation/[...slug].tsx:489-580,618-628` | tbd | inventoried |
| SEO-P006 | Translation matrix and keyword pages | web-phone | Same templates and route behavior; `frontend/src/pages/translation/[...slug].tsx:489-580,618-628` | tbd | inventoried |
| SEO-P007 | Localized tool keyword pages | web-desktop | 138 de/tr/kk routes and invalid-slug redirect; `frontend/src/pages/tools/[keywords].tsx:21-57` | tbd | inventoried |
| SEO-P008 | Localized tool keyword pages | web-phone | Same route and content; `frontend/src/pages/tools/[keywords].tsx:21-57` | tbd | inventoried |
| SEO-P009 | Blog list, article, images, locale and legacy redirects | web-desktop | Metadata slugs, full sorted list, Article/FAQ data; `frontend/src/pages/blog/[[...articleSlug]].tsx:46-115`; `frontend/src/components/BlogList/BlogList.tsx:64-118` | tbd | inventoried |
| SEO-P010 | Blog list, article, images, locale and legacy redirects | web-phone | Same content and responsive list; `frontend/src/components/BlogList/BlogList.tsx:63-118` | tbd | inventoried |
| SEO-P011 | Pricing, FAQ and signup CTA | web-desktop | Monthly/yearly cards from package catalog; `frontend/src/components/LandingPage/Pricing/Pricing.tsx:34-57,85-94` | tbd | inventoried |
| SEO-P012 | Pricing, FAQ and signup CTA | web-phone | Stacked cards and same catalog; `frontend/src/components/LandingPage/Pricing/Pricing.tsx:54-57` | tbd | inventoried |
| SEO-P013 | Legal, contact, certificates and supported-language content | web-desktop | Existing routes and generated certificate ids; `frontend/src/pages/certification/[id].tsx:150-172`; `frontend/src/pages/contact/index.tsx:1`; `frontend/src/pages/legal/index.tsx:1` | tbd | inventoried |
| SEO-P014 | Legal, contact, certificates and supported-language content | web-phone | Same content on responsive pages; `frontend/src/pages/certification/[id].tsx:150-172`; `frontend/src/pages/contact/index.tsx:1` | tbd | inventoried |
| SEO-P015 | Free-tool index and three trimming routes | web-desktop | Pick file, trim, processor progress, two outputs/ZIP; `frontend/src/utils/free-tools/trimAudioServer.ts:60-132` | tbd | inventoried |
| SEO-P016 | Free-tool index and three trimming routes | web-phone | Same processing with phone file picker and controls; `frontend/src/components/TrimAudio/TrimAudioTool.tsx:154-158,243-246` | tbd | inventoried |
| SEO-P017 | Convert audio | web-desktop | Upload, format/options, convert, download; `frontend/src/utils/free-tools/convertAudioServer.ts:37-64` | tbd | inventoried |
| SEO-P018 | Convert audio | web-phone | Same server-first flow; `frontend/src/components/ConvertAudio/ConvertAudioTool.tsx:114-118,152-155` | tbd | inventoried |
| SEO-P019 | Reduce audio | web-desktop | Upload, quality, process/poll, download; `frontend/src/utils/free-tools/reduceAudioServer.ts:37-73` | tbd | inventoried |
| SEO-P020 | Reduce audio | web-phone | Same runtime flow; `frontend/src/utils/free-tools/reduceAudioServer.ts:37-73` | tbd | inventoried |
| SEO-P021 | Remove noise | web-desktop | Upload, level, denoise/poll, download; `frontend/src/utils/free-tools/removeNoiseServer.ts:34-76` | tbd | inventoried |
| SEO-P022 | Remove noise | web-phone | Same runtime flow; `frontend/src/utils/free-tools/removeNoiseServer.ts:34-76` | tbd | inventoried |
| SEO-P023 | Extract video audio | web-desktop | Upload, format, extract/poll, download; `frontend/src/utils/free-tools/videoToAudioServer.ts:40-89` | tbd | inventoried |
| SEO-P024 | Extract video audio | web-phone | Same runtime flow; `frontend/src/utils/free-tools/videoToAudioServer.ts:40-89` | tbd | inventoried |
| SEO-P025 | Compress video | web-desktop | Upload, quality, compress/poll, download; `frontend/src/utils/free-tools/compressVideoServer.ts:33-77` | tbd | inventoried |
| SEO-P026 | Compress video | web-phone | Same runtime flow; `frontend/src/utils/free-tools/compressVideoServer.ts:33-77` | tbd | inventoried |
| SEO-P027 | Convert video | web-desktop | Upload, format/options, convert/poll, download; `frontend/src/utils/free-tools/convertVideoServer.ts:44-96` | tbd | inventoried |
| SEO-P028 | Convert video | web-phone | Same runtime flow; `frontend/src/utils/free-tools/convertVideoServer.ts:44-96` | tbd | inventoried |
| SEO-P029 | Free YouTube caption extraction and export | web-desktop | Paste URL, request captions, copy/download or signup CTA; `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:136-139` | tbd | inventoried |
| SEO-P030 | Free YouTube caption extraction and export | web-phone | Same public caption endpoint; `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:136-139` | tbd | inventoried |
| SEO-P031 | Record microphone audio locally | web-desktop | Permission, device selection, record/stop/preview/download; `frontend/src/pages/tools/record-audio-online.tsx:195-230` | tbd | inventoried |
| SEO-P032 | Record microphone audio locally | web-phone | Same browser-permission flow with supported MIME fallback; `frontend/src/pages/tools/record-audio-online.tsx:15-25,195-230` | tbd | inventoried |
| SEO-P033 | Capture tab audio locally | web-desktop | Pick display/tab, record, WAV download; `frontend/src/pages/tools/tab-audio-capture.tsx:168-171`; `frontend/src/components/TabAudioCapture/DownloadControls.tsx:59-96` | tbd | inventoried |
| SEO-P034 | Tab-capture capability/error behavior | web-phone | Preserve browser unsupported/permission outcome, not a promised native capture capability; `frontend/src/pages/tools/tab-audio-capture.tsx:168-171,268-305` | tbd | inventoried |
| SEO-L01 | English sitemap manifest | web-desktop | 356 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L02 | Turkish sitemap manifest | web-desktop | 282 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L03 | Portuguese sitemap manifest | web-desktop | 267 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L04 | Spanish sitemap manifest | web-desktop | 273 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L05 | German sitemap manifest | web-desktop | 343 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L06 | Indonesian sitemap manifest | web-desktop | 287 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L07 | Italian sitemap manifest | web-desktop | 266 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L08 | French sitemap manifest | web-desktop | 263 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L09 | Greek sitemap manifest | web-desktop | 264 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L10 | Russian sitemap manifest | web-desktop | 305 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L11 | Danish sitemap manifest | web-desktop | 265 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L12 | Swedish sitemap manifest | web-desktop | 264 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L13 | Norwegian sitemap manifest | web-desktop | 263 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L14 | Finnish sitemap manifest | web-desktop | 261 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L15 | Kazakh sitemap manifest | web-desktop | 300 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |
| SEO-L16 | Azerbaijani sitemap manifest | web-desktop | 268 URLs; source derivation in R5, `frontend/public/sitemap-0.xml` | tbd | inventoried |

## Research

### R4: product and runtime boundary

`frontend/src/utils/free-tools/clientWasmEnabled.ts:5` sets `CLIENT_WASM_ENABLED = false`. Server-first dispatch is explicit in `frontend/src/components/TrimAudio/TrimAudioTool.tsx:154-158,243-246` and `frontend/src/components/ConvertAudio/ConvertAudioTool.tsx:114-118,152-155`. The seven processor families below are public session/file workflows, not authenticated paid transcription jobs. CMS HTML alone cannot replace them.

All paths in this table are relative to the separate processor host, not Next.js `/api`. Each family has POST `/upload`, GET `/progress`, GET `/download`, DELETE `/invalidate`; the processing action and additional GET routes are listed separately. GET `/health` is defined in `frontend/services/free-tools-processor/src/index.ts:34-36`. The client host is NEXT_PUBLIC_FREE_TOOLS_PROCESSOR_URL, default localhost:8080 (`frontend/src/utils/free-tools/processorUrl.ts:7-8`).

| Family | Processing POST | Additional GET | Exact route declaration sources |
|---|---|---|---|
| `/trim-audio` | `/trim` | `/download-zip` | `frontend/services/free-tools-processor/src/routes/trim-audio.ts:476,588,664,745,802,848` |
| `/convert-audio` | `/convert` | none | `frontend/services/free-tools-processor/src/routes/convert-audio.ts:325,414,510,525,572` |
| `/reduce-audio` | `/reduce` | none | `frontend/services/free-tools-processor/src/routes/reduce-audio.ts:408,517,609,644,692` |
| `/remove-noise` | `/denoise` | none | `frontend/services/free-tools-processor/src/routes/remove-noise.ts:400,499,554,605,657` |
| `/video-to-audio` | `/extract` | `/thumbnail` | `frontend/services/free-tools-processor/src/routes/video-to-audio.ts:503,649,684,782,834,886` |
| `/compress-video` | `/compress` | `/thumbnail` | `frontend/services/free-tools-processor/src/routes/compress-video.ts:579,719,751,813,861,912` |
| `/convert-video` | `/convert` | `/thumbnail` | `frontend/services/free-tools-processor/src/routes/convert-video.ts:528,661,696,815,866,918` |

This is 39 family endpoints plus health. API/RTL rules and endpoint parity belong to the backend worker; platform owns deployment and ten-minute cleanup. Phone and desktop public-tool UI remain static-topic parity obligations despite the filename.

| Public or paid surface | Execution | Sign-in, balance and job boundary | Source |
|---|---|---|---|
| Free processor tools | Upload/process/poll/download against endpoints above | No user account or transcription-minute debit in the client; processor sessions, not jobs table | `frontend/src/utils/free-tools/trimAudioServer.ts:60-132`; `frontend/services/free-tools-processor/src/index.ts:22-50` |
| `/tools/youtube-transcript-generator` | POST `/api/youtube/public-transcript`, copy/download captions | Free caption extraction with no signup; absent captions lead to paid-product CTA, not silent transcription | `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:136-139`; `frontend/src/pages/tools/youtube-transcript-generator.tsx:59-82` |
| `/tools/record-audio-online` | getUserMedia, MediaRecorder, local recording/download | No automatic job or balance debit; user separately uploads recording to Transgate | `frontend/src/pages/tools/record-audio-online.tsx:195-230,666-670` |
| `/tools/tab-audio-capture` | getDisplayMedia and local WAV download | No automatic job or debit; permission/support errors matter on phones | `frontend/src/pages/tools/tab-audio-capture.tsx:168-171`; `frontend/src/components/TabAudioCapture/DownloadControls.tsx:59-96` |
| `/youtube-video-analysis`, `/subtitle-generator` | Product landing content and signup/app CTA | Hardcoded analysis demo and sign-in-gated subtitle UI, not separate anonymous processing backends; actual use is existing signed-in transcript/AI/export flow | `frontend/src/components/features/youtube-video-analysis/YouTubeAnalysisLandingPage.tsx:8-9,52-64`; `frontend/src/pages/subtitle-generator.tsx:7-18` |
| Signed-in YouTube and URL sources | YouTube add/resolve and transcription API | Creates jobs and uses existing billing/freemium rules; backend topic owns JOB/UPL/TRX | `frontend/src/features/transcription/viewmodels/useTranscriptionViewModel.ts:50-66,890-967` |
| `/voice-agent-test` and landing voice demo | Websocket audio using useVoiceAgent or PersonaPlex hook | Separate voice conversation runtime, not evidence of paid transcription. Presume out of scope | `frontend/src/pages/voice-agent-test.tsx:3-10,28-31`; `frontend/src/features/voice-demo/hooks/usePersonaPlex.ts:147-162` |

### R5: exact sitemap counts and deliberate pruning

Read-only derivation enumerated concrete page routes, expanding pages exporting getStaticProps across the 16 configured locales and other concrete routes in English; framework `_app`/`_document` were excluded and `/404` included. Then it added tool keyword lists, blog localized metadata slugs and indexes, translation generated paths, and two certificate ids. It applied the current sitemap exclude/transform/additionalPaths functions, deduplicated and normalized trailing slashes and XML entities for comparison. Result: 4,527 unique paths, zero source-only paths and zero snapshot-only paths against `frontend/public/sitemap-0.xml`. This is source/config evaluation, not a build or deployment verification.

Arithmetic by URL prefix: blog 2508 + languages 112 + translation children 40 + tools 186 + features 705 + free-tools 160 + legal 5 + certification 2 + contact 32 + other routes 777 = 4527. Generators and filters are cited in the route inventory. `frontend/next-sitemap.config.js:603-686` defines the transform and `frontend/src/pages/blog/[[...articleSlug]].tsx:119-158` the article paths.

| Locale | Published Markdown files | Blog URLs including index | Sitemap URLs |
|---|---:|---:|---:|
| en | 157 | 159 | 356 |
| tr | 156 | 156 | 282 |
| pt | 157 | 158 | 267 |
| es | 157 | 157 | 273 |
| de | 156 | 154 | 343 |
| id | 157 | 158 | 287 |
| it | 157 | 158 | 266 |
| fr | 157 | 155 | 263 |
| el | 156 | 157 | 264 |
| ru | 157 | 158 | 305 |
| da | 157 | 158 | 265 |
| sv | 157 | 157 | 264 |
| no | 157 | 157 | 263 |
| fi | 156 | 155 | 261 |
| kk | 156 | 156 | 300 |
| az | 155 | 155 | 268 |

The English blog URL count includes the separate editorial TSX page. File counts count filenames; URL counts use metadata Language/slug and deduplicate, which explains discrepancies. The scope excludes a stray `.md` file with no supported locale filename from the per-locale file count.

Pruning is deliberate, not missing implementation: 138 root templates formerly expanded into 2208 locale pairs; 2170 are excluded. Language matrix has 3616 candidates but only 112 traffic paths are listed. Most forward/reverse translation matrix paths are pruned. The 359-entry traffic allowlist records paths with at least five Search Console clicks and overrides broad pruning; it includes other route families too. Pruned pages still render for direct links/ads, with noindex and canonical consolidation. Sources: `frontend/src/config/seoIndexing.ts:1-14,245-319,321-681`; `frontend/next-sitemap.config.js:553-562,618-621`.

## Needs mu

- Approve whether SEO-D01 to SEO-D06 should change the P6 target count. Assumption: 4527 is the current baseline, not proof all entries deserve indexing.
- Confirm voice-demo exclusion and whether the separate `transgate-new-landing` experiment has any live role. Assumption: `frontend/src/pages` is the product source; do not migrate a second landing app by accident.
- Confirm public processor deployment settings at cutover without exposing secrets. Assumption: retain the runtime dependency with local fixtures; no new browser-WASM default.

## Open questions for the design

- E1 must compare generators against the actual URL manifest, not a handful of pages or all 3616 language candidates.
- CMS should own Markdown/metadata/URL generation; browser capture and free-media processing stay interactive shells and service adapters.
- Migrate duplicate blog slugs and malformed dates with explicit identity/redirect decisions, rather than dropping content because paths deduplicate.
- Preserve pricing catalog linkage, per-locale canonical intent and traffic exceptions while reducing duplicated template code.
