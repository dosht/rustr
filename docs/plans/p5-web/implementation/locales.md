# P5 locale source handoff

Owner: preserved Sol session `w23:p24`, actual model `openai-codex/gpt-6.1-sol`. Exclusive locale/script slice plus the separately lead-authorized E4 evidence-pointer correction. No root UI, core, manifest/lock, book or canonical edits.

## Current source-ready status

**Current published P5 locale integration is source-ready, not executed or accepted.** All16 locale directories contain actual catalogs, including the final core message meanings, p21's current AI keys, frame/workspace accessibility labels and FINAL D22. English meanings come from `implementation/core.md`, actual source bytes, or the owning view author's explicit UI-label contract; no product-policy meaning was invented. Lead owns execution and acceptance.

No builds, tests, lint, formatters, browser/provider calls, new-tool smoke or source-application execution by this owner. Catalog data was authored through static file-tool I/O and TypeScript AST inspection, not by executing the capture/import/validation scripts. Two attempted nested tooling workers failed at capacity before authoring; no retry/provider hopping, and all their intended script work was completed directly in this visible session. Lead subsequently prohibited further nested fanout; none followed.

## Published files and consumer contract

- `transgate/web/locales/config.json`: authoritative locale/resource/resolver configuration.
- `transgate/web/locales/<code>/{common,app,ai,dashboard,auth}.json`: flat UTF-8 string maps. Codes exactly `en,tr,pt,es,de,id,it,fr,el,ru,da,sv,no,fi,kk,az`.
- `transgate/web/locales/reachable-keys.json`: `{version:1,namespaces:{[ns]:{[key]:{english,arguments,sources}}},dynamicSourceCalls:[...]}`. Source references and unresolved dynamic-source seams are explicit, not guessed namespace keys.
- `transgate/web/locales/fallback-manifest.json`: `{version:1,baseline:[{locale,ns,key,reason,source}],active:[],policy}`. Baseline distinguishes missing source keys, source-hardcoded labels, placeholder mistakes and invalid German resource syntax. It is discovery evidence, **not runtime fallback permission**. D3 now authorizes filling reachable keys; do not initialize foreign strings from English or load baseline fallback as runtime resources.
- `transgate/web/locales/source-provenance.json`: source paths/SHA-256, the six German repairs, authored translation-input hash and source-equivalent reuse records. Source repository stays read-only.
- `scripts/p5-locales/scope.json`: explicit bounded app source readset, excluded unmounted/P6-only paths, source snapshot hashes and explicit source-literal English meanings.
- `scripts/p5-locales/translations.json`: curated D3 additions keyed by English meaning and locale, with explicit locale-specific plural additions. This is authored source, not a network/English-copy initializer.
- `scripts/p5-locales/translation-inputs/<code>.json`: retained source-gap/placeholder input inventory for each non-English locale.

`Msg` maps directly: `ns` chooses the resource, `key` is the exact literal key, `args` supplies interpolation and is always present (`{}` when empty). Translate nested `Arg::Msg` first; numeric `count` drives plurals. Flat source nested paths remain literal dotted keys, so `sign_in.email_label` is a literal key in `auth`, not an object traversal.

Exact i18next options in config:

```json
{
  "fallbackLng": false,
  "keySeparator": false,
  "nsSeparator": false,
  "ignoreJSONStructure": false,
  "pluralSeparator": "_",
  "contextSeparator": "_",
  "returnNull": false,
  "returnEmptyString": false,
  "returnObjects": false,
  "saveMissing": false,
  "interpolation": {"escapeValue": false}
}
```

Svelte escapes text/attributes; never inject a message or argument as HTML. Literal colons, periods and English sentence keys must not be split into namespaces/objects. Missing active keys must fail owner tests instead of returning an English key and calling it localized. No Rust crate compile-time catalog dependency is introduced by this slice; core carries language-neutral keys and generated fixtures carry actual messages for consumer validation.

P6-only tools/SEO/keyword/language-marketing corpora are not imported. Source SSR preloads of `translation`/absent `footer` do not themselves establish a reachable key. Five source-resource catalogs are retained, but the published core `Msg` namespaces are exactly `common/app/ai/auth`; retained `dashboard` inventory includes the P7-dependent key-screen source, not an invented P5 `Ns` variant or implemented key administration. Source metadata language pickers, list subtitles and transcript tabs retain raw English language names; only auto-detect is translated (`LanguageDropdown.tsx:122-127`, `MobileLanguageSheet.tsx:99-104`). The settings language switcher separately translates common language-name keys. Catalog availability never changes raw metadata into localized copy.

## Source content and translation work

The current conservative app-source union contains common app/auth/queue/list/transcript/billing/settings/consent/AI/export text and dynamic enum/header candidates. Source-only unmounted filter and thrown billing-information form paths, plus Head-only P6 SEO metadata, are excluded. It does not authorize mounting an unmounted control.

Auth labels are imported from the actual inline `const locales` object in `frontend/src/pages/signin.tsx:244-1157`, not assumed to exist in JSON. They are flattened under `auth`, including provider interpolation. The source hard-coded password/cookie/checkout/invoice/AI-section labels have explicit English keys and translations. Existing translated source text is retained; exact-meaning reuse across source namespaces is recorded; missing strings and placeholder errors receive real authored translations in all required languages.

Source German `app.json` has **six**, not four, malformed closing quotation delimiters: lines 115,118,121,124,127,130. The importer repairs only those recorded boundaries in copied data. Source file bytes are unchanged and hashed. Source duplicate members use the source's last-wins JSON meaning, with duplicate evidence in capture; authored catalogs reject duplicates/colliding flattened keys.

Count-driven countdown variants include Russian few/many and Portuguese/Spanish/French/Italian many forms, explicitly authored rather than silently choosing English. Placeholder identifiers such as `provider`, `count`, `date`, `received`, `total` remain protocol identifiers, never translated. Proper product/format names and genuine shared-language words are not falsely labelled filler merely for matching English spelling.

FINAL D22 is integrated from p1X's corrected contract, not the superseded relay: modal refusal is `POST` consent false and preserves completed results; the existing settings action remains `DELETE`, revoking consent and purging account-side saved AI content. Its label and confirmation explicitly say turning AI off **and deleting saved AI data**. Provider retention/deletion remains unverified; saved results belong to the account, not only the browser. `cleanup_pending` says deletion is still in progress, never completed. No endpoint, control or provider guarantee was added.

## Final current-batch key publication

- All final `app:p5.consent.*` keys are present in all16: `provider_processing` (`provider`), `provider_retention`, `account_results`, `decline_keeps`, `settings_delete`, `turn_off_delete_action`, `turn_off_delete_title`, `turn_off_delete_confirm`, `turn_off_delete_button`, `cleanup_pending`. The three settings captions are “Turn off AI and delete saved AI data”, “Turn off AI and delete your saved AI data?”, and “Turn off and delete”. Superseded D22 key relays were never published. The old common settings-confirmation body is removed from active resources; its source fallback evidence is retained.
- D1's truthful `app:This answer could not be saved. It may disappear after refresh.` replaces the withdrawn local-persistence claim, without an alias. D21-withdrawn `Failed to record consent` / `Failed to revoke consent` were not added. p21's withdrawn `AI content deletion is in progress.` is not added; use `app:p5.consent.cleanup_pending`. The remaining published introduction/consent/limits/badge/quota/action-error keys are integrated; quota placeholders retain their actual source names.
- Workspace/player consumers use p1Z's exact app keys: `p5.transcript.edit_line`, `p5.transcript.save_line`, `p5.transcript.edit_speaker_label`, `p5.history.translation_job`, `p5.player.skip_back`, `p5.player.skip_forward`, `p5.player.speed`, `p5.player.play`, `p5.player.pause`, `p5.youtube.mode_video`, `p5.youtube.mode_strip`, `p5.youtube.mode_float`. There are no newly added natural-key compatibility aliases for those machine keys. Source `Expand player`, `Close player`, `Player mode`, and `Untitled` also have app keys. Media Session job titles and the `Transgate AI` artist brand remain data/names; action identifiers remain protocol values.
- `common:Download all formats`, `common:Edit speaker`, and `common:Password updated successfully!` are present. Queue duration is `app:p5.duration.minutes` with `minutes`; the balance formats retain their separate published keys. Final core erasure/session/auth/support/status/YouTube/list/header meanings are integrated.
- Frame copy is `app:Light mode`, `Dark mode`, `System mode`, `Collapse Sidebar`, `Expand Sidebar`, `Main navigation menu`, `More options`, `My Transcripts`, `All Transcripts`, and `p5.web.config_unavailable`. Actual stored common resources contain the exact `Email: `, `Joined on: ` (trailing spaces in the keys), `Change Language`, and `Loading...` keys in all16.
- D17 checkout failure copy is `common:Please sign in and try again.` and `common:Something went wrong while preparing your payment. No charge was made — please try again.` in all16. Existing `Checkout`, `We couldn't start your checkout`, and `Back to plans` remain. The failure template consumes escaped labels, never the raw failure message.
- PRE-S1 billing dependency closure consumes R `billing.md` sections 7–10 and p1X's executable `presentation/billing.rs` mapping. All16 now contain argument-free `app:p5.billing.no_active_subscription`, `p5.billing.payment_unconfirmed`, `p5.billing.customer_not_found`, and `p5.billing.lookup_failed` with the exact accepted English anchors. The unconfirmed-payment warning remains uncertain and explicitly says not to pay again.
- Newly reached common source-fallback labels `Coupon is not valid`, `Coupon has expired`, and `Error` have actual all16 translations. Remaining confirmation/subscription/invoice/admin/special-offer labels retain R section 9/source keys and existing native resources; their reachability/provenance references are recorded. Document delivery failure reuses existing `app:An unexpected error occurred. Please try again.`, without a new key. Final `ConfirmMessages.status` is optional; the unconfirmed warning appears once as the message, not a repeated status caption.
- `common:redirectMessage` is called with numeric `count` and resolves the actual source `redirectMessage_one` / `redirectMessage_other` family plus explicit locale CLDR variants. No fake bare resource value `"redirectMessage"` was added. `common:Renews on ` retains its trailing-space key and no arguments: core owns `renewal_at_ms`, and the shell locale-formats that timestamp without renewal arithmetic. `SpecialClientAlert.message` remains verbatim host data; no catalog lookup, percentage reconstruction or future-copy alias was introduced.

## S1 working-source repair: frozen failure preserved

p1Y's pinned-Node24 frozen gate `s1-20261026142204` reported **FAILED**, exactly six `invalid_value` issues in `pt/common`: `10`, `11`, `12`, `18`, `20`, `21`. Evidence: `/home/mu/.cache/rustr-integration/s1-20261026142204/logs/locales.log`. That directory, failed result and frozen source were not changed. No worker validator/import/capture/test/build/browser/provider execution or fresh-gate pass is claimed.

Confirmed cause: those six values are already empty strings in the original Portuguese source resource. Static catalog authoring retained them without an explicit correction. The numeric-looking keys are opaque IDs, not their English display values. Working corrections use the actual English resource meanings:

```json
{
  "10": {"english": "Footer", "pt": "Rodapé"},
  "11": {"english": "09", "pt": "09"},
  "12": {"english": "10", "pt": "10"},
  "18": {"english": "Thank You", "pt": "Obrigado"},
  "20": {"english": "Confirm Checkout", "pt": "Confirmar compra"},
  "21": {"english": "Not Enough Balance", "pt": "Saldo insuficiente"}
}
```

The six corrections are explicit `translations.json.overrides.pt.common` entries, so normal regeneration cannot prefer the original blanks. `translatedSourceCorrections`, resolution records, the Portuguese source-gap input and fallback baseline now record them coherently as `source_empty_value`; original source hashes remain unchanged and `active` remains `[]`. `capture.mjs` records empty/whitespace source values as this baseline reason without modifying captured source data. A synthetic regression for preserved blank/whitespace evidence was authored, not run. Validator and importer code were not changed or weakened; no selected key was dropped to hide the failure.

Separately, p1Z's same-slice source-control request is saved in all16 app catalogs: `Resize video player (drag up or down)`, `Move to top-left`, `Move to top-right`, `Move to bottom-left`, `Move to bottom-right`. Source: `YouTubePlayerLayout.tsx:364,331-336`. These are exact natural keys, with no arguments or aliases for the earlier machine keys. Their authored translations, reachability and source-gap evidence are included in the hashes below.

SHA-256 of changed payload/source files, calculated only for the requested repair handoff. This list excludes this self-referential handoff document; its final hash is reported separately to lead/Y.

```text
0a6c7a9b2c5d2895b74d9a8572d3936a171e787fde4d860dd52b6fe3705f0585  transgate/web/locales/pt/common.json
a62ff7af9a35368b1abea84ecf570d734746e864964d9bf78946ce79ea3e4bc5  scripts/p5-locales/capture.mjs
07b111213862b578106f782ed054bf434d02cbea1e7cd17e1f212db3259dfa78  scripts/p5-locales/capture.test.mjs
53514a62351b7d540e2089049cf144942ad9053efc013700e2a5c917b76b1c07  scripts/p5-locales/translations.json
477060b5e52e0432195f51cc7f7055120fc78b9f26b34c91b8d452dcc57e654e  scripts/p5-locales/translation-inputs/pt.json
25bdc58b63f4a079755040d8ee44f33722081f25df46062778f90288a59b1f38  scripts/p5-locales/scope.json
b95a08390c8dc6404bd2a288c6d4288bfeb8459538aea324888fce5d50762778  transgate/web/locales/reachable-keys.json
00ba148b7741b5e7f167691344374df4984d68d3f186adbab50e1fd966d9b25a  transgate/web/locales/fallback-manifest.json
f1014229cc38260f45f8092c337046fa7e37adb014b143a5c0589bd4fb776bc2  transgate/web/locales/source-provenance.json
735951817a70b143acce4e7b7c93f30301048161e5d1ed4b79f2ecbfeac087e3  transgate/web/locales/en/app.json
b33cc9b74a0092e428027a00539ca79f8c7a39024451cc5029bf1acc7ee71b53  transgate/web/locales/tr/app.json
9f6afbf6c5cbaaccc2ada7a0159a9ca1508d5d59d168d683d7d033a58945bc4e  transgate/web/locales/pt/app.json
f182d5dd1ecce28c241ecaa92de076a65ea5ff567be31abe1c34aa2173279a12  transgate/web/locales/es/app.json
5d6f20f016dbd5e0a8947d270e4d65986b67b3d6f98c7201cf149200f3b2c7f3  transgate/web/locales/de/app.json
84aad51894bd82655f6b40bf48539aec5b38ebec07621aec1fe4a3fe3dc81127  transgate/web/locales/id/app.json
156b7cb291ac8ae2a2538200acf0a3e69069018b731f9f4b8d21d97d0a4ba62e  transgate/web/locales/it/app.json
90c404b0e204c100879d629518d32e9d5db84b0c3f785f4d43271a922e30e321  transgate/web/locales/fr/app.json
6a6f01b0e3a2f161787fe6714213943658b73fd746883f5562d770531752e116  transgate/web/locales/el/app.json
93c4a5396a81640f28e80e4989b3c54c8cd3fd446f135591b74ceb197e5543d2  transgate/web/locales/ru/app.json
916e8a95ab5dbd7eab8e5ae17785ce964a94b6cf1bb3379df5e9dafcb1901a6c  transgate/web/locales/da/app.json
6a3b4145b8bf532d8b56f9bb8a32bc48ddc1c401cd80298db6f0e78937b8142a  transgate/web/locales/sv/app.json
6a98a64bfe3716dfa70f180a4d757aa36ff94a18bb936053bdd9f99577a3f796  transgate/web/locales/no/app.json
ecdf5c0d696ec8ea176d76946a6a5aa9584ceb44ffcca7509aa5e34e94704283  transgate/web/locales/fi/app.json
e9f4d57d0202357d2f6d567e9e55b428fd2d775253410b4035eb60846809218a  transgate/web/locales/kk/app.json
fdd6b5b636096933b25ae52a772475781df79ec9a63da5012081f84c526cc8b6  transgate/web/locales/az/app.json
```

Unchanged strict validator/importer reference hashes:

```text
20b603b4dcc0fcf8ecabdc5844295cc994b98933ad5289e52dcb093cbd9b8f08  scripts/p5-locales/validate.mjs
fdb79c82c24ae29d29194be171691e8586ebf8d9daf7468377538fc25cd41532  scripts/p5-locales/import.mjs
```



## Runnable script source and regression coverage

- `capture.mjs`: AST-only actual-source capture, pinned path/hash boundaries, inline auth extraction, explicit dynamic calls/literals, exact German repair, source last-wins duplicate evidence, locale resource/provenance report. Does not import/transpile/execute app modules. Requires the source's already-installed TypeScript, no install/network. Output is create-only and may not target the original source tree or an escaping symlink.
- `import.mjs`: builds complete flat catalogs from capture plus authored translations. Missing translation or changed interpolation aborts before file output. No English or cross-category automatic initializer. Explicit CLDR variants; validates final data before writing. Refuses original-source/symlinked output and confines metadata filenames.
- `validate.mjs`: exact 16-locale/namespace contract, strict duplicate JSON parsing, flat/nonempty values, literal key semantics, placeholder sets, English source/approved anchors, CLDR categories, source provenance, and no active fallback.
- `capture.test.mjs`, `import.test.mjs`, `validate.test.mjs`: authored consumer-visible failure boundaries: traversal/symlink/source snapshot change; duplicate/colliding JSON; German strict repair; missing locale/key; dropped/renamed placeholders; Russian plural categories; no English initializer; no P6 namespace creep. These regression tests have not been run by this owner; the actual frozen S1 validator failure and working-source repair are recorded above.

## Lead-only execution commands

From repository root, use D16's pinned Node 24.22.1 with source TypeScript already installed. Use a **fresh** capture filename; `capture.mjs` deliberately refuses to overwrite evidence.

```sh
P5_SOURCE_FRONTEND=/home/mu/Business/Transgate/frontend node --test scripts/p5-locales/validate.test.mjs scripts/p5-locales/capture.test.mjs scripts/p5-locales/import.test.mjs
mkdir -p target/p5-locales
node scripts/p5-locales/capture.mjs --source-root /home/mu/Business/Transgate/frontend --scope scripts/p5-locales/scope.json --output target/p5-locales/source-capture.json
node scripts/p5-locales/import.mjs --capture target/p5-locales/source-capture.json
node scripts/p5-locales/validate.mjs
```

These are unexecuted commands, not passing evidence. Source drift must be reviewed before changing pinned hashes. Import regeneration rewrites only owned locale JSON/metadata. It does not create user-facing placeholders if a key meaning or translation is missing.

p1Y owns actual C2 all16 desktop/phone browser proof; p1V owns i18next loading/escaped rendering and localized accessibility names; p1X/p21 own the final emitted key meanings. Both layouts, long strings, plural/interpolation behavior and actual typed/nested Msg consumers still need lead-run integration. Linguistic source has not been reviewed by native speakers; no native-speaker or live-provider claim is made.

The original billing locale prerequisite is now closed against the published R table and p1X core mapping; no unpublished billing product meanings were guessed. p1Y owns validator execution using pinned Node 24 and the package-lock/clean-ci setup. Generated composition, browser evidence and P7 key administration remain integration/phase gates, not claims made by this source-only handoff.

## Separately authorized E4 evidence update

Only `docs/plans/p4-ai/implementation/e4.md:7` changed: latest lead-reported **PASS81**, `target/p4-e4/run-dd8Tk7/report.json` (artifact787), rebuilt real examples. No new elapsed-time claim, code change or rerun. Source-baseline/D1/D8 and live-provider/retention/region/cost caveats remain intact.

## Current-batch hold and preserved work

All available original-source locale content is authored in all16 catalogs. Explicit D3 corrections also replace source-inherited untranslated Portuguese auth progress, Greek recovery confirmation, Norwegian auth buttons, German timestamp labels and other recorded English phrases; product names such as Google Drive remain names. These are stored in `translations.json.overrides`, not hidden runtime fallback. Dynamic `SectionHeader` titles and `transcriptFileSaver.ts` labels have explicit source references and translations.

The earlier Claude usage hold was subsequently cleared by lead-confirmed reset and restoration of the original owner sessions. Only this original locale assignment resumed; no Claude implementation was absorbed or model/provider transfer made. Current published message integration is now saved. Hold after handoff: no new agents, phase work, retries, timer, checks or provider calls; resume only on explicit lead dispatch.

No owned smoke process, container, listener or scheduled job was created. The two failed tooling jobs exited before authoring and remain idle; no running nested worker needs cancellation. No source/session/findings were discarded. Lead owns all verification and current-batch completion decisions.
