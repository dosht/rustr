# Languages

- Today the app has 16 UI locales (path prefix, default `en`), a 61 language picker, and 53 translation targets that exist only as Soniox one-way translation (no LLM translation path exists).
- Routing is by language support: Soniox covers 53 of the 61, Deepgram (whisper-large) and RunPod (large-v3) are declared to cover all 61, so no language is RunPod-only; 8 languages can only go to Deepgram once RunPod is dropped.
- Speaker labels are not gated by language in code (the "Indonesian only" claim is a stale prompt line in voice-actor); 4 picker languages (Georgian, Latin, Uzbek, Luxembourgish) are missing from the migration-defined DB enum.

## Inventory

All paths are relative to `/home/mu/Business/Transgate/` (frontend = `frontend/`). Fixtures: `transgate/fixtures/languages/*.json` in the rustr repo.

### 1. UI locales (16)

Source of the list: `frontend/next.config.js:21-24` and `frontend/next-i18next.config.js:6-9` (identical arrays: en, tr, pt, es, de, id, it, fr, el, ru, da, sv, no, fi, kk, az). Default `en`. Switcher labels and flags: `frontend/src/components/Settings/LanguageSwitcher/LanguageSwitcher.tsx:33-50`. All 16 are left to right: the only direction code is `frontend/src/pages/_document.tsx:7` (sets `lang`, never `dir`).

Key counts are leaf strings per JSON file in `frontend/public/locales/<code>/`, computed by a script, compared with `en`. "Missing" means a key in `en` and not in the locale. `namespaces_complete` in the fixture is true only when every one of the 13 shared namespaces exists, parses and has zero missing keys: true for `en`, false for the other 15.

| Code | Name | Dir | Default | Files | common | app | ai | tools | Missing common / app / ai / tools |
|---|---|---|---|---|---|---|---|---|---|
| en | English | ltr | yes | 98 | 1003 | 161 | 773 | 412 | 0 / 0 / 0 / 0 |
| tr | Turkish | ltr | no | 34 | 1004 | 161 | 621 | 409 | 17 / 0 / 152 / 4 |
| pt | Portuguese | ltr | no | 15 | 961 | 160 | 621 | 412 | 50 / 1 / 152 / 1 |
| es | Spanish | ltr | no | 15 | 960 | 160 | 620 | 412 | 44 / 1 / 153 / 1 |
| de | German | ltr | no | 99 | 990 | invalid JSON | 621 | 412 | 14 / n/a / 152 / 1 |
| id | Indonesian | ltr | no | 15 | 959 | 161 | 621 | 413 | 45 / 0 / 152 / 72 |
| it | Italian | ltr | no | 15 | 989 | 160 | 621 | 412 | 15 / 1 / 152 / 1 |
| fr | French | ltr | no | 15 | 989 | 160 | 620 | 412 | 15 / 1 / 153 / 1 |
| el | Greek | ltr | no | 15 | 959 | 160 | 621 | 410 | 46 / 1 / 152 / 75 |
| ru | Russian | ltr | no | 14 | 959 | 160 | 621 | 412 | 45 / 1 / 152 / 0 |
| da | Danish | ltr | no | 14 | 992 | 160 | 621 | 412 | 12 / 1 / 152 / 0 |
| sv | Swedish | ltr | no | 14 | 989 | 160 | 621 | 412 | 15 / 1 / 152 / 0 |
| no | Norwegian | ltr | no | 14 | 989 | 160 | 621 | 412 | 15 / 1 / 152 / 0 |
| fi | Finnish | ltr | no | 14 | 989 | 160 | 621 | 409 | 15 / 1 / 152 / 3 |
| kk | Kazakh | ltr | no | 48 | 989 | 160 | 621 | 409 | 15 / 1 / 152 / 3 |
| az | Azerbaijani | ltr | no | 14 | 992 | 160 | 621 | 410 | 12 / 1 / 152 / 75 |

Namespaces with identical key counts in all 16 locales: dashboard 186, howto 71, cs 420, kw 1174, languages 5600, seo-keyword-pages 234, transcription 246, translation 293, translation-language-specific 201. The `en` locale alone also has `translation-keyword.json` (used by 11 `useTranslation('translation-keyword')` calls), the 3 `Master-translate-language-*` files and 81 `reverse-translation/*.json`. Extra locale files: `kw-<code>.json` for de, el, es, fr, id, it, pt; `kw/` folders in de (84 files), tr and kk.

Loading: `appWithTranslation` (`frontend/src/pages/_app.tsx:87`), pages call `serverSideTranslations(locale, [namespaces])` (`common` in about 258 of the calls, plus kw, cs, ai, app, dashboard, howto, tools, translation...). In `common` the keys are the English sentences, so a missing key shows the English text.

### 2. Transcription languages (61)

Picker list and name to code table: `frontend/src/domain/languages.ts:1-63` (names) and `:65-127` (initials). Code sent = initials in lower case (`:220`). Provider support: `frontend/src/services/unified-transcription/providers/capabilities.ts:13-105`. Fixture: `transcription-languages.json` (61 rows). Provider columns: S = soniox (`stt-async-v3`) and soniox_v5 (`stt-async-v5`, canary), D = deepgram (`whisper-large`), R = runpod (`large-v3`). All three send the same lower case code. Auto-detect, speaker labels and grammar correction are explained under Rules.

| Name | Code | S | D | R | Auto-detect result | Speaker labels | Grammar correction | languages.ts line | Note |
|---|---|---|---|---|---|---|---|---|---|
| Afrikaans | af | yes | yes | yes | yes | yes | no | 2 |  |
| Arabic | ar | yes | yes | yes | yes | yes | no | 3 |  |
| Armenian | hy | no | yes | yes | no | yes | no | 4 | deepgram/runpod only |
| Azerbaijani | az | yes | yes | yes | yes | yes | no | 5 |  |
| Belarusian | be | yes | yes | yes | yes | yes | no | 6 |  |
| Bosnian | bs | yes | yes | yes | yes | yes | no | 7 |  |
| Bulgarian | bg | yes | yes | yes | yes | yes | no | 8 |  |
| Catalan | ca | yes | yes | yes | yes | yes | no | 9 |  |
| Chinese | zh | yes | yes | yes | yes | yes | no | 10 |  |
| Croatian | hr | yes | yes | yes | yes | yes | no | 11 |  |
| Czech | cs | yes | yes | yes | yes | yes | no | 12 |  |
| Danish | da | yes | yes | yes | yes | yes | no | 13 |  |
| Dutch | nl | yes | yes | yes | yes | yes | no | 14 |  |
| English | en | yes | yes | yes | yes | yes | no | 15 |  |
| Estonian | et | yes | yes | yes | yes | yes | no | 16 |  |
| Finnish | fi | yes | yes | yes | yes | yes | no | 17 |  |
| French | fr | yes | yes | yes | yes | yes | no | 18 |  |
| Galician | gl | yes | yes | yes | yes | yes | no | 19 |  |
| German | de | yes | yes | yes | yes | yes | no | 20 |  |
| Greek | el | yes | yes | yes | yes | yes | no | 21 |  |
| Hebrew | he | yes | yes | yes | yes | yes | no | 22 |  |
| Hindi | hi | yes | yes | yes | yes | yes | no | 23 |  |
| Hungarian | hu | yes | yes | yes | yes | yes | no | 24 |  |
| Icelandic | is | no | yes | yes | no | yes | no | 25 | deepgram/runpod only |
| Indonesian | id | yes | yes | yes | yes | yes | no | 26 |  |
| Italian | it | yes | yes | yes | yes | yes | no | 27 |  |
| Japanese | ja | yes | yes | yes | yes | yes | no | 28 |  |
| Kannada | kn | yes | yes | yes | yes | yes | no | 29 |  |
| Kazakh | kk | yes | yes | yes | yes | yes | no | 30 |  |
| Korean | ko | yes | yes | yes | yes | yes | no | 31 |  |
| Latvian | lv | yes | yes | yes | yes | yes | no | 32 |  |
| Lithuanian | lt | yes | yes | yes | yes | yes | no | 33 |  |
| Macedonian | mk | yes | yes | yes | yes | yes | no | 34 |  |
| Malay | ms | yes | yes | yes | yes | yes | no | 35 |  |
| Marathi | mr | yes | yes | yes | yes | yes | no | 36 |  |
| Maori | mi | no | yes | yes | no | yes | no | 37 | deepgram/runpod only |
| Nepali | ne | no | yes | yes | no | yes | no | 38 | deepgram/runpod only |
| Norwegian | no | yes | yes | yes | yes | yes | no | 39 |  |
| Persian | fa | yes | yes | yes | yes | yes | no | 40 |  |
| Polish | pl | yes | yes | yes | yes | yes | no | 41 |  |
| Portuguese | pt | yes | yes | yes | yes | yes | no | 42 |  |
| Romanian | ro | yes | yes | yes | yes | yes | no | 43 |  |
| Russian | ru | yes | yes | yes | yes | yes | no | 44 |  |
| Serbian | sr | yes | yes | yes | yes | yes | no | 45 |  |
| Slovak | sk | yes | yes | yes | yes | yes | no | 46 |  |
| Slovenian | sl | yes | yes | yes | yes | yes | no | 47 |  |
| Spanish | es | yes | yes | yes | yes | yes | no | 48 |  |
| Swahili | sw | yes | yes | yes | yes | yes | no | 49 |  |
| Swedish | sv | yes | yes | yes | yes | yes | no | 50 |  |
| Tagalog | tl | yes | yes | yes | yes | yes | no | 51 |  |
| Tamil | ta | yes | yes | yes | yes | yes | no | 52 |  |
| Thai | th | yes | yes | yes | yes | yes | no | 53 |  |
| Turkish | tr | yes | yes | yes | yes | yes | no | 54 |  |
| Ukrainian | uk | yes | yes | yes | yes | yes | no | 55 |  |
| Urdu | ur | yes | yes | yes | yes | yes | no | 56 |  |
| Vietnamese | vi | yes | yes | yes | yes | yes | no | 57 |  |
| Welsh | cy | yes | yes | yes | yes | yes | no | 58 |  |
| Georgian | ka | no | yes | yes | no | yes | no | 59 | deepgram/runpod only, not in DB enum |
| Latin | la | no | yes | yes | no | yes | no | 60 | deepgram/runpod only, not in DB enum |
| Uzbek | uz | no | yes | yes | no | yes | no | 61 | deepgram/runpod only, not in DB enum |
| Luxembourgish | lb | no | yes | yes | no | yes | no | 62 | deepgram/runpod only, not in DB enum |

Counts: Soniox 53, Soniox v5 53, Deepgram 61, RunPod 61. Not on Soniox (8): Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek, Luxembourgish (`capabilities.ts:37,67`). The comment at `capabilities.ts:35` says 46 languages and "all except Armenian, Icelandic, Maori, Nepali"; the code removes 8. The comments at `:14,:89` say 57 for Deepgram and RunPod; the code gives 61.

`auto_detect` in the fixture is true when the language can be the result of an Auto-detect job, which only Soniox serves (derived from code, not tested against the Soniox API). `runpod_only` is false for all 61.

Languages in the WhisperX worker (`runpod-whisper-x/src/whisperx_worker/whisperx/utils.py:9-110`): 100 Whisper codes. All 61 codes above are inside that set; 39 other Whisper codes (for example `bn`, `km`, `lo`, `mn`, `yo`) are never offered by the app.

### 3. Translation targets (53)

Engine: Soniox async one-way translation (`frontend/src/services/unified-transcription/providers/soniox.ts:156-173`, same in `soniox-v5.ts:161-173`): `translation = {type: 'one_way', target_language: <code of translate_to[0]>}`. The UI list is the intersection of the languages of every provider with `supportsTranslation` true (`capabilities.ts:121-137`), so it equals the Soniox set. The same list is used for the source side in translation mode (`useTranscriptionViewModel.ts:190-250`). Fixture: `translation-targets.json` (53 rows, same code and name as above).

Targets (code name): af Afrikaans, ar Arabic, az Azerbaijani, be Belarusian, bs Bosnian, bg Bulgarian, ca Catalan, zh Chinese, hr Croatian, cs Czech, da Danish, nl Dutch, en English, et Estonian, fi Finnish, fr French, gl Galician, de German, el Greek, he Hebrew, hi Hindi, hu Hungarian, id Indonesian, it Italian, ja Japanese, kn Kannada, kk Kazakh, ko Korean, lv Latvian, lt Lithuanian, mk Macedonian, ms Malay, mr Marathi, no Norwegian, fa Persian, pl Polish, pt Portuguese, ro Romanian, ru Russian, sr Serbian, sk Slovak, sl Slovenian, es Spanish, sw Swahili, sv Swedish, tl Tagalog, ta Tamil, th Thai, tr Turkish, uk Ukrainian, ur Urdu, vi Vietnamese, cy Welsh.

No separate LLM translation call exists in the live job path. Soniox owns speech translation. The AI layer generates analysis, chat and a separate general summary: analysis computes a target-aware language option but ignores it downstream, general summary also ignores its language option, and chat forwards the job/source language (LNG-030, AI R2). Deepgram and RunPod declare supportsTranslation=false (`capabilities.ts:31,104`). The public API has no translation target field; its translate boolean is a carried Whisper option rather than the Soniox target selector.

### 4. Routing as code expresses it

Web uploads call `POST /api/v1/transcriptions` with job ids (`features/transcription/services/FileUploadService.ts:108`), which reaches `ProviderManager.selectProvider` (`services/unified-transcription/services/transcription.ts:507-521`). Rules in evaluation order (full data in `provider-routing.json`):

| Step | When | Provider and model | Source |
|---|---|---|---|
| 1 | job.transcription_provider is already set and the provider passes validate (canHandle, translation capable if translate_to set, available, health thresholds) | that provider (per provider) | .../unified-transcription/services/provider-manager.ts:245-262; .../unified-transcription/services/transcription.ts:507-509 |
| 2 | job.duration > SONIOX_MAX_DURATION_MINUTES (env, default 250) and no translate_to | deepgram (whisper-large) | .../unified-transcription/services/provider-manager.ts:273-287 |
| 3 | original file extension (from job_name) is qta and no translate_to | deepgram (whisper-large) | .../unified-transcription/services/provider-manager.ts:297-308 |
| 4 | request.preferredProvider is set and passes validate | that provider (per provider) | .../unified-transcription/services/provider-manager.ts:311-323 |
| 5 | job language is Arabic, Indonesian, Japanese, Korean, Chinese, Urdu or Tamil and soniox passes validate | soniox (stt-async-v3) | .../unified-transcription/services/provider-manager.ts:168-175,341-351 |
| 6 | clientType is api or web (every request) and soniox passes validate | soniox (stt-async-v3) | .../unified-transcription/services/provider-manager.ts:193-196,353-361 |
| 7 | none of the above chose: chain soniox, deepgram, runpod re-sorted by health score, first that passes validate; translate_to set keeps soniox only | first valid in health-sorted chain (per provider) | .../unified-transcription/services/provider-manager.ts:363-370,1366-1520 |
| 8 | chain gave nothing: first of soniox, deepgram, runpod that canHandle and isAvailable; translate_to set keeps soniox only and throws 'Translation jobs require Soniox, but Soniox is unavailable' when none | first available (per provider) | .../unified-transcription/services/provider-manager.ts:372-380,1526-1610 |
| 9 | language is null (Auto-detect in the UI) | soniox (stt-async-v3) | .../unified-transcription/providers/deepgram.ts:52-58 (deepgram.canHandle returns false for null); .../unified-transcription/providers/base.ts:118-121 (soniox and runpod accept null); .../unified-transcription/providers/soniox.ts:114-142 |
| 10 | translate_to has at least one language | soniox (stt-async-v3) | .../unified-transcription/services/provider-manager.ts:1125-1133 (validate rejects providers with supportsTranslation false); .../unified-transcription/providers/capabilities.ts:54,104; .../unified-transcription/providers/soniox.ts:156-186 |
| 11 | language is Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek or Luxembourgish (soniox canHandle is false) | deepgram (first in default chain, runpod if health scores reorder) (whisper-large) | .../unified-transcription/providers/capabilities.ts:36-39,66-69; .../unified-transcription/services/provider-manager.ts:363-370 |
| 12 | soniox_v5 selection | soniox_v5 (stt-async-v5) | .../unified-transcription/providers/soniox-v5.ts:133-135; frontend/src/utils/transcriptionProvider.ts:25-36 (only when transcription_provider is set to soniox_v5 by hand; never in any chain) |
| 13 | deepgram request for a hardcoded user id | deepgram (nova-2) | frontend/src/utils/deepgram.ts:134-139 |

Note on steps 7 and 8: the chain is re-sorted by a health score computed from the last hour of job metrics (`getEnhancedProviderHealth`, `provider-manager.ts:791-830`, cached 30 seconds), so the order of Deepgram and RunPod is not fixed. The old selector `frontend/src/utils/transcriptionProvider.ts` (returns the job provider or soniox) has no importers.

### 5. Other language lists in the code (reference)

| List | Where | Size | Owner |
|---|---|---|---|
| DB enum `public.language` | `supabase/migrations/20251130113249_remote_schema.sql:129-189` | 57 | data model |
| `jobs.translate_to` | `supabase/migrations/20251130114026_new_changes.sql:113` (`language[]`) | same enum | data model |
| Picker list | `src/domain/languages.ts:1-63` | 61 | languages |
| AI language names | `src/lib/ai/locale-map.ts:5-21` | 57 codes (no ka, la, uz, lb) | AI (R2) |
| SEO slugs | `src/config/languages.ts` `supportedSlugs` | 226 slugs, 56 languages | static site |
| Public supported languages page | `src/components/LandingPage/SupportedLang/SupportedLang.tsx:5` | 56 | static site |
| SEO forward translation data | `src/data/languageTranslationData.ts` | 51 (6 not in product: Bengali, Zulu, Amharic, Khmer, Lao, Mongolian) | static site |
| SEO reverse translation pages | `src/pages/translation/[...slug].tsx:535-566` | 56 (includes Armenian, Icelandic, Maori, Nepali, which the product cannot translate) | static site |
| RTL set | `src/utils/textDirection.ts:8-14` | arabic, hebrew, persian, farsi, urdu | languages |

## Rules

pgTAP is `none`: no file in `frontend/supabase/tests/` tests a language rule. A row is `tested` only when every clause has executed evidence ([clause-reconciliation.md](../../../docs/plans/clause-reconciliation.md), 2026-10-27). Browser clauses are covered by P5 receipts: LNG-001 by LOC-001..016 and WEB-039/040; LNG-011/012 by WEB-013/014.

Lead decisions 2026-10-27:

- **LNG-003/004:** `tested` on the core rule (`presentation::route` first-visit locale and signed-in root target). Applying that rule at the public root `/` moves to P6.
- **LNG-005:** `changed` under P5 D3: there is no runtime English fallback, every locale ships its keys, and a missing key is reported and shown as the key. Evidence is Y's committed `transgate/web/e2e/locale-no-fallback.spec.ts`, run on both projects (candidate9, 3 runs, all passing).
- **TRX-023/LNG-013:** the core leg is proven by the cited tests. The stored-invalid-target leg is reopened with p1T and p1X (`transgate-jobs-postgres` `tests/p3_close.rs`), so both rows stay `ledger`.

Other rows stay `ledger`.

| ID | Rule | Source | pgTAP | Rust test | Status |
|---|---|---|---|---|---|
| LNG-001 | The app has exactly 16 UI locales: en, tr, pt, es, de, id, it, fr, el, ru, da, sv, no, fi, kk, az. The default locale is en. | frontend/next.config.js:21-24; frontend/next-i18next.config.js:6-9 | none | cargo:transgate-core:languages::tests::ui_locales_match_the_fixture; cargo:transgate-core:presentation::route::tests::home_views_jobs_and_pages_parse_with_or_without_a_locale_prefix | tested |
| LNG-002 | The default locale en has no URL prefix. Every other locale is a path prefix (/tr/home, /id/home ...). The language switcher does a full page load to /<code>/home and writes no cookie. | frontend/src/components/Settings/LanguageSwitcher/LanguageSwitcher.tsx:34-59 | none | cargo:transgate-core:presentation::route::tests::lng_002_only_non_default_locales_are_prefixes | tested |
| LNG-003 | First visit locale choice is the Next.js built-in detection (NEXT_LOCALE cookie, then Accept-Language on /): the i18n block does not turn detection off and no app code reads the cookie. This is framework behavior, not repo code. | frontend/next.config.js:21-24 (no localeDetection key) | none | cargo:transgate-core:presentation::route::tests::lng_003_first_visit_locale_is_cookie_then_accept_language_then_default | tested |
| LNG-004 | A visitor with a Supabase session cookie who opens / is redirected to /<request locale>/home (/home for en). | frontend/src/middleware.ts:34-40 | none | cargo:transgate-core:presentation::route::tests::lng_004_the_signed_in_root_target_is_the_locale_home | tested |
| LNG-005 | A missing translation falls back to English: fallbackLng is not set, so next-i18next uses the default locale. In common the key is the English sentence. Framework default, to confirm in the new i18n. | frontend/next-i18next.config.js:6-12; frontend/public/locales/en/common.json | none | playwright:e2e/locale-no-fallback.spec.ts#locale without fallback every sign-in label in each non-English locale is that locale's own text; playwright:e2e/locale-no-fallback.spec.ts#locale without fallback every signed-in home label in each non-English locale is that locale's own text; playwright:e2e/locale-no-fallback.spec.ts#locale without fallback a key missing from a catalog is reported and shown as the key, never as English | changed |
| LNG-006 | The html lang attribute is the request locale, default en. No UI locale sets dir: all 16 render left to right. | frontend/src/pages/_document.tsx:7 | none | cargo:transgate-core:presentation::route::tests::lng_006_every_ui_locale_renders_left_to_right | tested |
| LNG-007 | Transcript text direction is right to left when the job language (case insensitive, trimmed) is arabic, hebrew, persian, farsi or urdu; null or anything else is left to right. During streaming the job language is used; for a finished translation job the selected language tab decides. | frontend/src/utils/textDirection.ts:8-28; frontend/src/features/transcriptLines/viewmodels/useTranscriptLinesVM.ts:143-156; frontend/src/features/transcriptLines/domain/mappers.ts:87 | none | cargo:transgate-core:presentation::transcript::tests::edt_013_lng_007_direction_switches_when_a_streaming_job_completes | tested |
| LNG-008 | The transcription picker offers 61 languages from one list, with Auto-detect pinned first in transcription mode. Preferred languages for the user come from past jobs; defaults en, tr, de, id are in getPreferredLanguages (not wired). | frontend/src/domain/languages.ts:1-63; frontend/src/features/transcription/viewmodels/useTranscriptionViewModel.ts:203-210; frontend/src/features/transcription/ui/mobile/utils/languageHelpers.ts:26-33 | none | cargo:transgate-core:languages::tests::transcription_languages_are_a_superset_of_the_fixture; cargo:transgate-core:jobs::routing::tests::soniox_takes_its_languages_and_auto_detect; cargo:transgate-core:jobs::routing::tests::eight_languages_go_to_deepgram | tested |
| LNG-009 | Auto-detect stores jobs.language = null. The provider then sends no language_hints and sets enable_language_identification true. A language already chosen sends language_hints = [code] and keeps identification on. | frontend/src/features/transcription/ui/mobile/utils/languageHelpers.ts:71-93; frontend/src/services/unified-transcription/providers/soniox.ts:114-142 | none | cargo:media-adapters:soniox_real_post_keeps_source_reference_and_explicit_v5_language_translation_auth; cargo:transgate-core:presentation::upload_queue::tests::start_needs_a_language_choice | tested |
| LNG-010 | A job with language null can only go to Soniox: Deepgram.canHandle returns false for null; Soniox and RunPod accept null. | frontend/src/services/unified-transcription/providers/deepgram.ts:52-58; frontend/src/services/unified-transcription/providers/base.ts:118-121 | none | cargo:transgate-core:jobs::routing::tests::preferred_provider_needs_to_be_valid; cargo:transgate-core:jobs::routing::tests::soniox_takes_its_languages_and_auto_detect | tested |
| LNG-011 | Auto-detect is not offered in translation mode: one-way translation needs a known source language. | frontend/src/features/transcription/viewmodels/useTranscriptionViewModel.ts:203-210 | none | cargo:transgate-core:presentation::upload_queue::tests::translation_languages | tested |
| LNG-012 | In translation mode both the source list and the target list are the translation languages (53, the intersection of providers with supportsTranslation). The target list never contains the chosen source language. Switching to translation sets translate_to; switching back clears it. | frontend/src/services/unified-transcription/providers/capabilities.ts:121-137; frontend/src/features/transcription/viewmodels/useTranscriptionViewModel.ts:190-250,795-810,1006 | none | cargo:transgate-core:presentation::upload_queue::tests::translation_languages | tested |
| LNG-013 | A job sends one translation target only (translate_to[0]) as Soniox one_way translation with the lower case ISO code. A target that has no code makes the job fail with 'Translation language mapping failed'; there is no fallback. | frontend/src/services/unified-transcription/providers/soniox.ts:156-186; frontend/src/services/unified-transcription/providers/soniox-v5.ts:161-185 | none | cargo:transgate-core:jobs::app::tests::trx_023_lng_013_an_unmapped_stored_target_fails_the_job_at_dispatch; cargo:transgate-core:jobs::app::tests::trx_023_lng_013_only_the_first_target_is_sent_as_its_code; cargo:transgate-jobs-postgres:trx_023_lng_013_a_stored_target_without_a_code_fails_the_job_and_never_breaks_a_read | tested |
| LNG-014 | Only providers with supportsTranslation true may run a job with translate_to: soniox and soniox_v5 true, deepgram and runpod false. Routing, failover and last resort all filter on it and fail with 'Translation jobs require Soniox, but Soniox is unavailable' instead of falling back. | frontend/src/services/unified-transcription/providers/capabilities.ts:54,85,104; frontend/src/services/unified-transcription/services/provider-manager.ts:1125-1133,1431-1470,1553-1603,570-585 | none | cargo:transgate-core:jobs::routing::tests::long_audio_goes_to_deepgram; cargo:transgate-core:jobs::routing::tests::pins_that_cannot_execute_fail_over; cargo:transgate-core:jobs::routing::tests::qta_goes_to_deepgram_unless_translation; cargo:transgate-core:jobs::routing::tests::translation_requires_soniox | tested |
| LNG-015 | Soniox and Soniox v5 accept a job when its language is null or one of the 61 minus Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek, Luxembourgish (53). Comparison is case insensitive on the language name. | frontend/src/services/unified-transcription/providers/capabilities.ts:34-39,64-69; frontend/src/services/unified-transcription/providers/base.ts:118-129 | none | cargo:transgate-core:jobs::routing::tests::soniox_takes_its_languages_and_auto_detect; cargo:transgate-core:jobs::routing::tests::eight_languages_go_to_deepgram | tested |
| LNG-016 | Deepgram accepts any of the 61 languages (not null); RunPod accepts null or any of the 61. | frontend/src/services/unified-transcription/providers/capabilities.ts:13-15,88-90 | none | cargo:transgate-core:jobs::routing::tests::soniox_takes_its_languages_and_auto_detect; cargo:transgate-core:jobs::routing::tests::eight_languages_go_to_deepgram | tested |
| LNG-017 | Language code sent to every provider is the 2 letter lower case initials of the language name (Chinese zh, Norwegian no, Malay ms, Welsh cy, Tagalog tl, Maori mi, Persian fa). RunPod gets the same code lower cased. | frontend/src/domain/languages.ts:65-127,220; frontend/src/utils/deepgram.ts:150-160; frontend/src/utils/runpod.ts:257 | none | tbd | ledger |
| LNG-018 | Provider order: any provider already stored on the job; then long audio and qta rules; then the request's preferred provider; then the language preference (Arabic, Indonesian, Japanese, Korean, Chinese, Urdu, Tamil to soniox); then the client type preference (api and web to soniox); then the health sorted chain soniox, deepgram, runpod; then the last resort. | frontend/src/services/unified-transcription/services/provider-manager.ts:166-196,245-380 | none | cargo:transgate-core:jobs::routing::tests::pins_that_cannot_execute_fail_over; cargo:transgate-core:jobs::routing::tests::preferred_provider_needs_to_be_valid; cargo:transgate-core:jobs::routing::tests::stored_provider_comes_first | tested |
| LNG-019 | Audio longer than SONIOX_MAX_DURATION_MINUTES (default 250) goes to Deepgram unless the job is a translation job. | frontend/src/services/unified-transcription/services/provider-manager.ts:273-287 | none | cargo:transgate-core:jobs::routing::tests::long_audio_goes_to_deepgram; cargo:transgate-core:jobs::routing::tests::qta_goes_to_deepgram_unless_translation | tested |
| LNG-020 | A file whose original name ends in .qta goes to Deepgram unless the job is a translation job. | frontend/src/services/unified-transcription/services/provider-manager.ts:290-308 | none | cargo:transgate-core:jobs::routing::tests::long_audio_goes_to_deepgram; cargo:transgate-core:jobs::routing::tests::qta_goes_to_deepgram_unless_translation | tested |
| LNG-021 | Soniox runs model stt-async-v3. soniox_v5 (stt-async-v5, same languages) runs only for a job whose transcription_provider is already soniox_v5; it is in no routing chain. | frontend/src/services/unified-transcription/providers/soniox.ts:129; frontend/src/services/unified-transcription/providers/soniox-v5.ts:135; frontend/src/services/unified-transcription/services/provider-manager.ts:1426-1428 | none | cargo:media-adapters:soniox_real_post_keeps_source_reference_and_explicit_v5_language_translation_auth | changed |
| LNG-022 | Deepgram runs model whisper-large for every user except one hardcoded user id, which gets nova-2. Parameters: smart_format, punctuate, paragraphs true, diarize from job.speakers. | frontend/src/utils/deepgram.ts:134-139,159-164 | none | tbd | ledger |
| LNG-023 | RunPod runs model large-v3. Its endpoint is STAGING when the UI locale is id, unless a feature_flags.runpod row names an environment. | frontend/src/services/unified-transcription/providers/runpod.ts:288-291; frontend/src/utils/runpod.ts:50-60 | none | cargo:transgate-core:jobs::routing::tests::pins_that_cannot_execute_fail_over; cargo:transgate-core:jobs::routing::tests::eight_languages_go_to_deepgram | changed |
| LNG-024 | Speaker labels do not depend on the language: the toggle is shown for every language and job.speakers is sent as Soniox enable_speaker_diarization, Deepgram diarize, and RunPod diarization. The claim 'speaker labels only for Indonesian' is not in the app code; it appears only as a stale line in an evaluation prompt. | frontend/src/features/transcription/ui/mobile/components/ConfigurationSection.tsx:121-136; frontend/src/services/unified-transcription/providers/soniox.ts:145-148; frontend/src/utils/deepgram.ts:151,164; voice-actor/.../evaluator_actor.ex:64 | none | cargo:transgate-core:presentation::upload_queue::tests::upl_020_lng_024_the_speaker_toggle_applies_to_every_draft_in_every_language | tested |
| LNG-025 | Grammar correction has no language rule. jobs.fix_grammer is set true only for URL jobs and YouTube jobs and is echoed in API and callback payloads; no code reads it to change a transcript. | frontend/src/services/unified-transcription/services/url-job-creator.ts:137; frontend/src/pages/api/youtube/add.ts:119; frontend/src/services/unified-transcription/services/callback-delivery.ts:164 | none | tbd | ledger |
| LNG-026 | A YouTube job language is: the language the user chose, else the base of the video's defaultAudioLanguage or defaultLanguage mapped through the initials table, else English. | frontend/src/pages/api/youtube/add.ts:86-97; frontend/src/features/transcription/services/youtube/YouTubeMetadataService.ts:92-93 | none | cargo:transgate-core:jobs::app::tests::youtube_add_creates_the_draft; cargo:transgate-core:sources::tests::youtube_language_and_duration | tested |
| LNG-027 | A URL or API job with no language is created as English. The API accepts any string as language (type check only); only enum names store. | frontend/src/services/unified-transcription/services/url-job-creator.ts:136; frontend/src/app/api/v1/transcriptions/route.ts:670-676 | none | tbd | ledger |
| LNG-028 | The public API has no translation target. The v1 'translate' boolean is carried but no provider acts on it. | frontend/src/services/unified-transcription/types/index.ts:95; frontend/src/app/api/v1/transcriptions/route.ts | none | tbd | ledger |
| LNG-029 | A freemium job over 20 minutes and up to 45 minutes is allowed when the user's locale is one of the 54 listed entries (48 distinct; 10 of the 16 UI locales: tr, de, fr, es, it, pt, sv, no, da, fi), or the job language, or the first translation target, is one of the 37 listed language names (lower case). Otherwise it is blocked with 'Not enough balance.'. Jobs of 20 minutes or less are never blocked. Owner of the limit itself: freemium topic. | frontend/src/utils/freemiumLimits.ts:9-90; frontend/src/services/unified-transcription/services/transcription.ts:411-420 | none | cargo:transgate-billing:p3_close::job_006_lng_029_a_start_batch_mixing_an_exempt_and_a_normal_draft_follows_the_lowest_id_draft; cargo:billing-core:freemium::tests::fre_018_only_the_first_draft_by_id_decides_the_exception_in_any_request_order | tested |
| LNG-030 | Analysis computes a target-aware language (translation target first, then job language/fallback) and passes it to generateWithTemplateStream, but that generator ignores the option. Therefore source does not guarantee the requested analysis output language. Chat reads job language rather than translate_to and forwards that option to its prompt. General completion summary is a separate path owned by AI. | frontend/src/pages/api/v1/ai/analyze.ts:172-200; frontend/src/lib/ai/ai-service-manager.ts:355-380; frontend/src/pages/api/v1/ai/chat.ts:108-148 | none | cargo:transgate-core:ai::analysis::sanitize_tests::chat_language_instructions_keep_source_exact_code_lookup_and_raw_unknown_fallback; cargo:transgate-core:ai::analysis::sanitize_tests::canonical_analysis_is_raw_while_summary_and_chat_drop_blank_lines_and_sanitize | tested |
| LNG-031 | There is no per language beta consent. The translation beta consent table was added and archived in December 2025; today no language needs consent. | frontend/supabase/migrations/20251208095855_add_translation_beta_consent.sql; frontend/supabase/migrations/20251218000000_archive_translation_beta_consent.sql | none | tbd | ledger |
| LNG-032 | Indonesian UI locale users are not tracked by PostHog (PostHog is not initialized when router.locale is id). | frontend/src/components/PostHogProvider/PostHogProvider.tsx:28-31 | none | playwright:e2e/cookie-consent.spec.ts#cookie consent PostHog does not start for the id locale | tested |
| LNG-033 | jobs.language and jobs.translate_to use the public.language enum. Migrations describe 57 names, but the read-only production probe confirms Georgian, Latin, Uzbek and Luxembourgish also exist there. Preserve all 61 offered languages; do not turn migration drift into a new restriction. | frontend/supabase/migrations/20251130113249_remote_schema.sql:129-189; frontend/supabase/migrations/20251130114026_new_changes.sql:113; transgate/docs/production-facts.md:39 | none | cargo:transgate-core:languages::tests::transcription_languages_are_a_superset_of_the_fixture | tested |

## Defects

| ID | Current behavior | Intended behavior | Proposal |
|---|---|---|---|
| LNG-D01 | The picker offers 61 languages but migrations/generated types describe 57. The read-only production probe confirms Georgian, Latin, Uzbek and Luxembourgish exist in production (`../production-facts.md:39`). This is schema drift, not a verified production save failure. | Every offered language is preserved and migrations describe deployed values. | Keep all 61 (PRD 2 and 7.4); include the four drifted values in the new schema and migration fixtures. No further production-enum query is needed for this question. |
| LNG-D02 | Capability comments say Soniox 46 languages and Deepgram and RunPod 57; the code gives 53 and 61 (`capabilities.ts:14,35,89`). | Docs match the data. | Use the code values. Fixtures carry the code values. |
| LNG-D03 | Deepgram uses nova-2 for one hardcoded user id; the comment says nova-3 (`deepgram.ts:134-139`). | No per user model override in code. | Throw. Model choice is a config value, not a user id. |
| LNG-D04 | jobs.transcription_provider has DEFAULT 'deepgram' in the migrations, and the provider manager honors a stored provider before any language rule (`remote_schema.sql:1420`, `transcription.ts:507`). If production still has the default, web jobs skip language routing and go to Deepgram, while the docs say Soniox is the default. | One routing rule decides. | Mu checks the production default and the provider split of recent jobs. The new system routes by the table in `provider-routing.json` and stores the provider only after it is chosen. |
| LNG-D05 | The 8 languages Soniox cannot take go to Deepgram or RunPod by a health score computed from the last hour of live job metrics (`provider-manager.ts:791-830`). Which one wins can change from hour to hour. | Deterministic language to provider mapping. | New routing: first provider in the configured order that supports the language, then duration. RunPod is not in the list for now. |
| LNG-D06 | The language preference for Arabic, Indonesian, Japanese, Korean, Chinese, Urdu and Tamil to soniox never changes a result, because the client type preference is soniox for every request. | One default rule. | Copy the outcome (soniox first), drop the duplicated table. |
| LNG-D07 | `de/app.json` is not valid JSON (unescaped quote at line 115, column 182), so the German `app` namespace cannot load (`public/locales/de/app.json`). | All 16 locales parse. | Fix in the new locale files; test that every locale file parses. |
| LNG-D08 | Locale files are incomplete: 15 locales lack 152 keys of `ai`, 12 to 50 keys of `common`, and az, el, id have 72 to 75 renamed or stale keys in `tools`. | Every key exists in every locale. | Parity target is the current state (English fallback), recorded per locale in the LOC rows. Mu decides whether to fill the gaps. |
| LNG-D09 | The freemium language lists contain duplicates and values that no UI locale can have (zh, ja, ko, pl, cs ... as locales) so only 10 of the 16 UI locales get the 45 minute allowance (`freemiumLimits.ts:11-19`). | The rule is stated as data. | Copy the effective sets (10 locales, 37 language names). Do not clean up in the port. |
| LNG-D10 | The public pages promise translation for Armenian, Icelandic, Maori and Nepali (reverse translation pages) and for 6 languages not in the product (Bengali, Zulu, Amharic, Khmer, Lao, Mongolian); the product offers neither. | SEO pages match the product. | Static site topic decides. Not part of the language fixtures. |
| LNG-D11 | Chat forwards the source/job language rather than translate_to; analysis computes target-aware language but its generator drops that option (`frontend/src/pages/api/v1/ai/chat.ts:108-148`; `frontend/src/lib/ai/ai-service-manager.ts:355-380`). | AI language behavior matches an explicit contract. | Record AI-D02 and LNG-030 together. Do not claim translation-language output from an unused option; any intended outcome change is recorded before implementation. |
| LNG-D12 | Cosmetic: Latin uses the Latvian flag, Persian RTL list has an alias 'farsi' that no job language can be, `getPreferredLanguages` is never called. | None. | Copy flags as data; drop the dead alias only if mu agrees. |

## Keep / throw

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Master-translate-language-az---.json, -id---.json, -kk---.json | frontend/public/locales/en/ | throw | Three identical 14090 byte copies of one Swedish to English page template. No code, script, page or test in the Transgate tree names them (searched the whole tree without node_modules, .next, .git). | nowhere |
| reverse-translation/ folders | frontend/public/locales/*/reverse-translation/ | keep-knowledge | Only the page template shape is knowledge. en has 81 files, 56 are loaded by `translation/[...slug].tsx:725`; 25 are unused (albanian, amharic, basque, bengali, cebuano, chichewa, corsican, esperanto, filipino, georgian, gujarati, haitian-creole, hausa, hawaiian, hmong, igbo, irish, javanese, khmer, kinyarwanda, kurdish, kyrgyz, lao, latin, luxembourgish). The 15 other locales hold 1 file each and no path generates them (reverse paths are built for en only, `:535-566`). Static site topic decides. | modules/cms |
| supportedSlugs SEO list | frontend/src/config/languages.ts | keep-knowledge | 226 SEO slugs for 56 languages. Reference only; owned by the static site topic. Not a product capability list. | modules/cms |
| translate_languages_page.py | frontend/translate_languages_page.py | throw | One off script that writes `public/locales/en/languages.json` (5600 SEO keys) from a language list of 56 names. The output is already committed; the new static site generates pages from content files. | nowhere |
| Unified provider plugin framework language fields | frontend/src/services/unified-transcription/framework/ | throw | `supportedLanguages` per plugin duplicates `capabilities.ts`; PRD drops the plugin framework. | Transcriber port |
| utils/transcriptionProvider.ts selectTranscriptionProvider | frontend/src/utils/transcriptionProvider.ts | throw | No importers. The live selector is the provider manager. | nowhere |
| Dead language helpers | frontend/src/utils/languageContext.ts: createLanguageContext, getJobLanguage, getEffectiveLanguage, getEffectiveLanguageWithFallback, mapLanguageToCodeSafe, getUserLocale, mapDatabaseLanguageToCodeSafe, getLanguageErrorMessage, logLanguageError | throw | No non-test importer. The live ones are getJobAILanguageWithErrorHandling, getJobLanguageWithErrorHandling, mapDatabaseLanguageToCode and handleLanguageError. | nowhere |
| Language lists, names, initials, flags | frontend/src/domain/languages.ts | port | Source of `transgate/core/src/languages.rs`; generated from the fixtures. | transgate/core |
| Soniox set of 53, translation targets of 53, RTL set, freemium language sets, AI fallback to English | capabilities.ts, textDirection.ts, freemiumLimits.ts, languageErrorHandler.ts | port | Rules LNG-007, 015, 029, 030 as data and pure functions. | transgate/core |
| RunPod/WhisperX path | utils/runpod.ts, providers/runpod.ts, runpod-whisper-x/ | gap | Dropped for now (PRD). No language of the 61 depends on it (see Research). The routing slot stays. | Transcriber port |
| Languages outside the 61 that WhisperX could take | runpod-whisper-x/.../utils.py:9-110 | gap | 39 Whisper codes the app never offers. No product impact. | nowhere |
| Locale fallback to English and per locale gaps | public/locales/* | gap | 15 locales miss 152 AI keys and some common keys; German `app` does not parse. The new app copies the English fallback unless mu fills them. | modules/cms and web app |

## Parity rows

One row per UI locale. Namespaces are in `frontend/public/locales/<code>/`.

| ID | Flow | Surface | Today | Test | Status |
|---|---|---|---|---|---|
| LOC-001 | App renders in locale en (English) | locale | public/locales/en (98 files) | tbd | inventoried |
| LOC-002 | App renders in locale tr (Turkish) | locale | public/locales/tr (34 files) | tbd | inventoried |
| LOC-003 | App renders in locale pt (Portuguese) | locale | public/locales/pt (15 files) | tbd | inventoried |
| LOC-004 | App renders in locale es (Spanish) | locale | public/locales/es (15 files) | tbd | inventoried |
| LOC-005 | App renders in locale de (German) | locale | public/locales/de (99 files) (app.json does not parse) | tbd | inventoried |
| LOC-006 | App renders in locale id (Indonesian) | locale | public/locales/id (15 files) | tbd | inventoried |
| LOC-007 | App renders in locale it (Italian) | locale | public/locales/it (15 files) | tbd | inventoried |
| LOC-008 | App renders in locale fr (French) | locale | public/locales/fr (15 files) | tbd | inventoried |
| LOC-009 | App renders in locale el (Greek) | locale | public/locales/el (15 files) | tbd | inventoried |
| LOC-010 | App renders in locale ru (Russian) | locale | public/locales/ru (14 files) | tbd | inventoried |
| LOC-011 | App renders in locale da (Danish) | locale | public/locales/da (14 files) | tbd | inventoried |
| LOC-012 | App renders in locale sv (Swedish) | locale | public/locales/sv (14 files) | tbd | inventoried |
| LOC-013 | App renders in locale no (Norwegian) | locale | public/locales/no (14 files) | tbd | inventoried |
| LOC-014 | App renders in locale fi (Finnish) | locale | public/locales/fi (14 files) | tbd | inventoried |
| LOC-015 | App renders in locale kk (Kazakh) | locale | public/locales/kk (48 files) | tbd | inventoried |
| LOC-016 | App renders in locale az (Azerbaijani) | locale | public/locales/az (14 files) | tbd | inventoried |

## Research

### R1: which languages does each provider cover, and which reach only RunPod/WhisperX

Answer from code (`capabilities.ts`), with U = the 61 picker languages:

| Provider | Model | Languages | Translation |
|---|---|---|---|
| Soniox | stt-async-v3 | 53 = U minus 8 | yes, one-way, 53 targets |
| Soniox v5 (canary, manual) | stt-async-v5 | 53, same set | yes |
| Deepgram | whisper-large (nova-2 for one user id) | 61 = U | no |
| RunPod / WhisperX | large-v3 | 61 = U (worker knows 100 Whisper codes) | no |

Set arithmetic:

- Soniox = 61 - |{Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek, Luxembourgish}| = 61 - 8 = 53.
- Deepgram = 61. RunPod = 61.
- RunPod-only = RunPod - (Soniox union Deepgram) = 61 - (53 union 61) = 61 - 61 = 0 languages.
- Deepgram-only (not Soniox) = 61 - 53 = 8 languages: Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek, Luxembourgish. When RunPod is dropped these still have Deepgram. If Deepgram were dropped too, these 8 would reach only RunPod.
- Inside the 57 migration enum names: 53 on Soniox, 4 Deepgram-only (Armenian, Icelandic, Maori, Nepali). Production has the other four too (`../production-facts.md:39`); migration enum coverage is not production coverage.
- Translation = Soniox only: 53 targets. Languages without translation: the same 8.
- The WhisperX worker knows 100 Whisper codes; U is a subset (all 61 codes are in `utils.py:9-110`). The 39 other codes are not reachable from the product.

So today no language reaches only RunPod/WhisperX by the code's own tables. The known gap for the keep/throw list is therefore small: no language is lost when RunPod is dropped, provided Deepgram stays and supports those 8 with whisper-large. Two things are not provable from the repo and are listed under Needs mu: the real Deepgram whisper-large language list (the code claims all 61) and the real Soniox list (the docs say '60+', the code excludes 8).

Also named as a gap: RunPod is the only place where WhisperX alignment and an Indonesian STAGING endpoint exist; neither is a language capability the product exposes today.

### Fixture shapes for the 7.4 test

All files are valid JSON arrays or objects, UTF-8, no comments. A superset test compares the new lists to these by `code` (and `name`).

- `ui-locales.json`: array of 16 objects `{"code": string, "name": string, "direction": "ltr", "default": bool, "namespaces_complete": bool}`. `code` is the Next.js locale and URL prefix. Exactly one `default` (en). `namespaces_complete` is true only for en (see Inventory 1).
- `transcription-languages.json`: array of 61 objects `{"code": string, "name": string, "providers": [{"provider": "soniox"|"soniox_v5"|"deepgram"|"runpod", "model": string, "code_sent": string}], "auto_detect": bool, "speaker_labels": bool, "grammar_correction": bool, "runpod_only": bool, "source": string}`. `name` is the DB enum style English name (the value stored in jobs.language). `code` equals `code_sent` for every provider. `auto_detect` true means Soniox covers it. `speaker_labels` is true for all. `grammar_correction` is false for all (no code acts on it). `runpod_only` is false for all.
- `translation-targets.json`: array of 53 objects `{"code": string, "name": string, "engine": string, "source": string}`. Every entry also exists in `transcription-languages.json` with soniox as a provider. The engine string is the same for all rows.
- `provider-routing.json`: object `{"order": [3 provider names], "rules": [{"when": string, "provider": string, "model": string, "source": string}], "fallback": [3 provider names], "notes": [string]}`. Text copied from the code; it is documentation for the routing test, not an executable spec.

A superset test should assert: the new UI locale codes contain the 16; the new transcription codes contain the 61 with the same names; the new translation codes contain the 53; every code of `translation-targets.json` has soniox in `transcription-languages.json`.

## Needs mu

- Production enum question is answered: Georgian, Latin, Uzbek and Luxembourgish exist (`../production-facts.md:39`). Preserve all 61.
- Production provider split is answered for the last 90 days: Soniox 7741, unset 1774, Deepgram 114 (`../production-facts.md:37`). The column default is still unverified because REST cannot inspect it. Do not infer the default from this distribution; use the declared routing rule for the rewrite.
- Real provider language lists: Soniox (code excludes 8; docs say 60+) and Deepgram whisper-large (code claims 61). Not in the repo and no network used. Assumed: the code tables are the truth for the fixtures.
- Whether Armenian, Icelandic, Maori, Nepali, Georgian, Latin, Uzbek, Luxembourgish should keep working with Deepgram only after RunPod is dropped. Assumed yes.

## Open questions for the design

- Language as a value: a Rust enum of 61 with name, code, flag, direction, and per provider support, generated from the fixtures. Where do the 8 non-Soniox languages live: data in the Transcriber capabilities, not code branches.
- Routing rule shape for P3: ordered providers, filter by language support, then translation, then duration (250) and file type (qta), same for every pipeline.
- Should the new DB store language as text with a check against the 61, or an enum? The old enum forced a migration for every new language.
- UI locale fallback: copy English fallback and per locale gaps, or fill the 152 AI keys? Mu decides; parity needs the current state.
- Freemium language allowance (LNG-029) needs a data table owned by the billing module; confirm it stays keyed by UI locale and language name.
- The duration unit in the 250 rule: `job.duration` is compared with minutes while the log field says seconds (`provider-manager.ts:275-283`); confirm with the data model topic.
