# P5 document model and byte adapter

Owner: saved `w23:p1W` (original Opus resource/processor session, continued on Sol). Authority: **P5-DOCS-FINAL-A**, D15 and D18. Status: **W source-ready/frozen; unbuilt and unrun**. Sol completed this P5 source and mapping; earlier Opus evidence is not relabelled as Sol verification. The two briefly started read-only scouts exited without findings and were not restarted. Current user hold permits finishing this existing non-Claude slice only.

## Dependencies and platform boundary

The adapter remains `transgate-export`; p1X alone adds `pub mod documents` to `transgate-core`. No host analysis-export route, JS encoder, latest-result lookup or new document service.

Selected dependency constraints (root lock integration belongs to p1Y/lead; unicode-segmentation intentionally permits compatible updates):

```toml
krilla = { version = "=0.8.2", default-features = false, features = ["raster-images"] }
rustybuzz = { version = "=0.20.1", default-features = false, features = ["std"] }
unicode-bidi = { version = "=0.3.18", default-features = false, features = ["std", "hardcoded-data"] }
unicode-linebreak = { version = "=0.1.5", default-features = false }
unicode-segmentation = { version = "1.12", default-features = false }
```

Krilla 0.8.2 is MIT OR Apache-2.0, MSRV 1.92; rustybuzz is MIT; the Unicode crates are MIT OR Apache-2.0. The published Krilla manifest, **not current main**, declares `pdf-writer 0.15.0`, `subsetter 0.2.6` with `variable-fonts`, and compatible `skrifa ^0.42`. Direct positioned glyphs are shaped with rustybuzz; Krilla's optional `simple-text`, `rayon`, `comemo` and PDF-import features are disabled. Raster images are enabled only to preserve the real source invoice logo. Unicode bidi determines visual runs; shaping retains original Unicode cluster ranges for PDF extraction; Unicode line/grapheme boundaries govern wrapping, never byte slicing/reversing Arabic.

p1Y reports the lock resolved krilla 0.8.2, pdf-writer 0.15.0, subsetter 0.2.6, **skrifa 0.42.1**, rustybuzz 0.20.1 and lopdf 0.38.0. The exact unicode-segmentation pin was not an API requirement; it was relaxed to `^1.12` after Y reported the workspace-wide downgrade, allowing the existing 1.13.3 resolution. Y owns the lock refresh; W ran no checks. The existing ZIP dependency remains for OOXML; serde/serde_json and billing-core serve the pure typed model. Independent-reader dev dependencies are roxmltree 0.21 and native-only lopdf =0.38.0; they do not enter wasm production code.

**REPAIR-P5-13 (lopdf bump, lead-approved).** The dev-only independent reader moved `lopdf =0.38.0` → `=0.46.0` (`default-features = false`; the 0.38.0 mentions above are superseded). Root cause: Krilla's ToUnicode CMap (no `/CIDInit` wrapper, with `/CMapVersion` and `/WMode`) is accepted by `qpdf --check` and Poppler `pdftotext`, but lopdf 0.38's parser demands the wrapper and rejects those metadata lines (`ToUnicodeCMap(Parse(Error))`); 0.46 parses it. Lock refresh by `cargo update -p lopdf --precise 0.46.0`: lopdf 0.38.0→0.46.0, cbc 0.1.2→0.2.1, ecb 0.1.2→0.2.1; added brotli-decompressor 5.0.3, alloc-no-stdlib 2.0.4, alloc-stdlib 0.2.4, cpubits 0.1.1, weezl 0.2.1; removed bytecount, cipher 0.4.4, inout 0.1.4, md-5 0.10.6, nom_locate 5.0.0, rand 0.9.5, rand_chacha 0.9.0, rand_core 0.9.5. Cargo.lock sha256 before `c7ea6155916e5496bd063a324fadbfb4a91fa29b6a39187d8b478c494e8e9f8c`, after `bd4e4c2675a3e526274092528786628639af250d99217aa707bb786d43d885ff`. Dev-only, native-only; wasm production graph unchanged.

**Test-tooling requirement and logical-text reading (REPAIR-P5-13).** `transgate-export` tests need Poppler `pdftotext` (poppler-utils) and **fail, not skip,** when it is absent; `.github/workflows/gates.yml` installs it. lopdf stays the reader for ordinary text assertions. Why `extract_text`-style visual extraction cannot be exact for the offered Arabic, Hebrew and Indic scripts: the PDF content stream holds positioned glyphs in **visual** order, and shaping decomposes or reorders them (Arabic dots/marks are separate glyphs; Devanagari `ि` is drawn before its consonant; ligatures merge several characters into one glyph), so a per-glyph ToUnicode lookup yields reversed order, U+FFFD or split clusters (lopdf gave `हि� न्दी�`, `ة`+`العربي`). The spec mechanism for the logical text of such a cluster is the marked-content `/ActualText` span, which Krilla emits. The strict check in `offered_language_scripts…` therefore reads the spans structurally (`logical_words`: ActualText string, else each glyph's ToUnicode, reversed for RTL) and requires every word of all 16 offered-script samples to be carried **exactly**; Poppler must additionally contain each word once whitespace is removed (it inserts spaces between joined glyph chunks, e.g. `ف ارسی`). Mark y offsets are deliberately kept (zeroing them would misplace Arabic marks).

This is a pure in-memory Rust dependency path suitable for native and wasm32-unknown-unknown: no system-font discovery, filesystem, process, host clock, native HarfBuzz or browser JS engine is required by the adapter. Actual wasm compilation, independent extraction and Arabic visual correctness remain **lead-run verification**, not claims from these reads.

Primary sources: [published manifest](https://docs.rs/crate/krilla/0.8.2/source/Cargo.toml), [Font](https://docs.rs/krilla/0.8.2/krilla/text/struct.Font.html), [positioned glyphs](https://docs.rs/krilla/0.8.2/krilla/text/struct.KrillaGlyph.html), [Surface](https://docs.rs/krilla/0.8.2/krilla/surface/struct.Surface.html), [rustybuzz manifest](https://raw.githubusercontent.com/harfbuzz/rustybuzz/v0.20.1/Cargo.toml), [bidi](https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/), [line breaks](https://docs.rs/unicode-linebreak/0.1.5/unicode_linebreak/), [graphemes](https://docs.rs/unicode-segmentation/1.12.0/unicode_segmentation/). Published Krilla source commit: `3ffdf0588cf98050aad6edba51ca70162e1fb5b5`.

## Assets, licenses and font port

Adapter code does **not** `include_bytes!` production fonts. Shells load the following locally bundled assets once, supply owned bytes, and retain one parsed `DocumentWriter` across exports. Tests embed the same assets with `include_bytes!`; no test/runtime CDN or system font substitution.

- `transgate/export/fonts/NotoSans.ttf` (Google Fonts `ofl/notosans/NotoSans[wdth,wght].ttf`).
- `transgate/export/fonts/NotoSans-Italic.ttf` (same family, italic variable font).
- `transgate/export/fonts/NotoSansArabic.ttf` (`ofl/notosansarabic/NotoSansArabic[wdth,wght].ttf`).
- `transgate/export/fonts/NotoSansSymbols2-Regular.ttf` (`ofl/notosanssymbols2/NotoSansSymbols2-Regular.ttf`), for source rating/action/status symbols.
- `transgate/export/fonts/NotoSansSymbols.ttf` (`ofl/notosanssymbols/NotoSansSymbols[wght].ttf`, v2.003, 372,684 bytes), added by REPAIR-P5-13 for U+2192: the PDF-only sentiment line "initial → final" (`core/src/documents.rs` `sentiment`) failed with `MissingGlyph('→')` because neither Noto Sans nor Symbols 2 covers it. It is the smallest OFL Noto face that does (Noto Sans Math is 1.0 MB, CJK 16 MB). Notice `NotoSansSymbols-OFL.txt`, provenance in `fonts/provenance.json`.
- Each family ships its unchanged upstream `*-OFL.txt` notice under **SIL Open Font License 1.1**; CJK additionally ships `NotoSansCJK-COPYRIGHT.txt`, extracted from its font copyright name record. Redistribution/embedding permitted; fonts are not relicensed as application code. `fonts/provenance.json` records upstream identities, git blobs, acquired hashes, sizes and matching notices.
- `transgate/export/assets/invoice-logo.jpg`: unchanged owned-product source `frontend/public/assets/Light-Logo-2x-100.jpg`; provenance recorded alongside it, not represented as an OFL font.

Docker/static packaging uses one authoritative `transgate/export/fonts` + `assets` copy, imported by p1V's web `?url` module and emitted as hashed same-origin assets; no duplicate `web/public` copy. Every OFL notice and the CJK copyright notice must ship. Logo provenance is `transgate/export/assets/provenance.json`. Native shells supply the same bundle. **TXT/MD/HTML and DOCX retain all source Unicode without loading fonts or checking glyph coverage.** D18 authorizes the extra offered-script PDF pack below, not a Latin/Arabic-only refusal policy or universal Unicode coverage.

Upstream font metadata/licenses: [Noto Sans](https://github.com/google/fonts/tree/main/ofl/notosans), [Arabic](https://github.com/google/fonts/tree/main/ofl/notosansarabic), [Symbols 2](https://github.com/google/fonts/tree/main/ofl/notosanssymbols2). Noto Sans contains the supplied Latin/Greek/Cyrillic faces; Arabic uses its own shaping font; symbols are a fallback, never a replacement for user text.

Exact adapter API (`transgate_export::documents`):

```rust
pub struct FontBytes {
    pub latin: Vec<u8>,
    pub latin_italic: Vec<u8>,
    pub arabic: Vec<u8>,
    pub symbols: Vec<u8>,
    pub arrows: Vec<u8>, // Noto Sans Symbols: U+2192
}
pub struct DocumentAssets { pub fonts: FontBytes, pub invoice_logo: Vec<u8> }
pub struct DocumentWriter { /* reusable parsed fonts and logo; no I/O */ }
pub struct DocumentBytes { pub filename: String, pub content_type: String, pub bytes: Vec<u8> }
impl DocumentWriter {
    pub fn new(assets: DocumentAssets) -> Result<Self, DocumentBytesError>;
    pub fn invoice(&self, document: &InvoiceDocument)
        -> Result<DocumentBytes, DocumentBytesError>;
    pub fn add_fallback(&mut self, family: FallbackFont, bytes: Vec<u8>)
        -> Result<(), DocumentBytesError>;
    pub fn has_fallback(&self, family: FallbackFont) -> bool;
}
pub fn analysis_bytes(document: &AnalysisDocument, format: DocumentFormat,
                      pdf_writer: Option<&DocumentWriter>)
    -> Result<DocumentBytes, DocumentBytesError>;
pub enum FallbackFont {
    Hebrew, Armenian, Georgian, Devanagari, Kannada, Tamil, Thai, Cjk,
}
// Clone + Copy + Debug + PartialEq + Eq + serde; snake_case wire values.
impl FallbackFont {
    pub const ALL: [Self; 8];
    pub fn filename(self) -> &'static str;
    pub fn license_filename(self) -> &'static str;
}
pub fn analysis_fonts(document: &AnalysisDocument)
    -> Result<Vec<FallbackFont>, DocumentBytesError>;
pub fn invoice_fonts(document: &InvoiceDocument)
    -> Result<Vec<FallbackFont>, DocumentBytesError>;
```

This owned-byte input is the font/assets port. Loading/saving/sharing is shell mechanics; no asynchronous font trait or platform I/O enters the pure model. `analysis_bytes(..., None)` encodes DOCX/TXT/MD with **no font or logo availability prerequisite**; only PDF uses `Some(&cached_writer)`. The initial unpublished `writer.analysis` signature was cleanly replaced and p22 notified. Clipboard uses the pure model directly. Fonts are retained/shared rather than recopied per streamed section.

## Exact pure model contract

Leaf: `transgate/core/src/documents.rs`, public path `transgate_core::documents` after p1X's declaration. Existing `export.rs` transcript models/behavior remain unchanged.

```rust
pub enum DocumentFormat { Docx, Pdf, Txt, Md }
pub struct AnalysisMetadata {
    pub title: String,
    pub job_id: Option<JobId>,
    pub exported_at: Micros,
    pub utc_offset_minutes: i16,
}
pub struct AnalysisDocument { /* owned immutable captured progress/status/metadata */ }
impl AnalysisDocument {
    pub fn capture(displayed: &AnalysisProgress, status: GenerationStatus,
                   metadata: AnalysisMetadata) -> Result<Self, DocumentError>;
    pub fn filename(&self, format: DocumentFormat) -> String;
    pub fn text(&self) -> String;
    pub fn html(&self) -> String;
}
pub struct InvoiceDocument { /* owned server-derived facts and effective bill-to lines */ }
impl InvoiceDocument {
    pub fn capture(facts: &billing_core::presentation::invoice::InvoiceFacts,
                   bill_to: &billing_core::presentation::invoice::BillTo,
                   temporary_bill_to: &str) -> Result<Self, DocumentError>;
    pub fn filename(&self) -> String;
}
```

Models/metadata/format/error are serde-serializable pure values. AnalysisDocument is Clone + Debug + PartialEq; DocumentFormat additionally is Copy + Eq, with lowercase wire values `docx`, `pdf`, `txt`, `md`. Capture deep-copies displayed typed progress **once at the click**, before asynchronous work. All five generation statuses retain visible partials; missing sections stay missing. Source title selection belongs to the panel: desktop `job_name || fileName || "Analysis"`, phone `fileName || "Analysis"`. The click supplies job id, Micros and local offset **east of UTC** (`-Date.getTimezoneOffset()`). Source DOCX/PDF dates are en-US local dates, not UI-locale formatting; no host clock or IANA lookup enters the writer. TXT/MD share text(); rich clipboard HTML comes from the same capture.

Invoice facts and BillTo use p1R's exact published `billing.md` §3 types. The document leaf does no VAT/rounding/identity lookup. Nonblank temporary Bill To lines replace only the address/identity block, never invoice amounts, dates or server facts; the input is unstored. Filename follows source `invoice-{invoice_id}.pdf`.

Source provenance: `frontend/src/components/AIAnalysis/export/{useExportAnalysis.ts,docx-exporter.ts,pdf-exporter.ts}`, `frontend/src/lib/ai/analysis-utils.ts`, `frontend/src/pages/billing/index.tsx:155-304,508-509`. Existing transcript OOXML is reusable container/XML infrastructure, **not** existing analysis support.

## Verification handoff boundary

Regression and reference-capture source is saved; exact selectors appear below. **No build, test, formatter, browser or product-provider call was run by W.** Static source/font acquisition and license/hash inspection are input provenance, not behavioral verification. Actual source captures, reader assertions, Arabic visual comparison, native/wasm execution and browser delivery remain lead/integration gates. No row is claimed passing from source inspection.

## Source font boundary and required scripts (not a PDF acceptance claim)

The actual 16 locales are `en,tr,pt,es,de,id,it,fr,el,ru,da,sv,no,fi,kk,az`
(`frontend/next-i18next.config.js:9`; `core/src/languages_data.rs:70-87`).
They require Latin (including Turkish/Azeri), Greek and Cyrillic (including
Kazakh); the bundled Noto Sans family targets these, with actual glyph and
visual proof still pending.

The **61 accepted transcript languages** (`core/src/languages_data.rs:6-68`)
also require Arabic/Persian/Urdu, Hebrew, Armenian, Georgian, Devanagari
(Hindi/Marathi/Nepali), Kannada, Tamil, Thai and Han/Kana/Hangul. There is no
claim that four existing font files cover all of these or arbitrary user
Unicode/emoji.

Source `pdf-exporter.ts:21-61,141-152` only detects Arabic ranges and attempts
Noto Sans Arabic; otherwise it uses built-in Helvetica. Source body text
`한국어 日本語 中文 नमस्ते שלום` without Arabic takes the Helvetica path.
Unsupported-glyph/corrupt PDF output on this path is **source-inferred, not
executed evidence**. Arabic font-fetch failure also returns to Helvetica;
reproducing that silent corruption is not a claimed implementation goal.

### D18 saved offered-script fallback pack

All files below are saved, unchanged upstream binaries under `transgate/export/fonts`. The base four fonts cover Latin/Greek/Cyrillic, Arabic and symbols; eight extra families target the remaining offered scripts. `fonts/provenance.json` is the machine-readable map, including exact upstream git blobs and notice links.

| File | Script/role | Bytes | SHA-256 |
|---|---|---:|---|
| NotoSans.ttf | Latin, Greek, Cyrillic; variable normal/bold | 2049096 | bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d |
| NotoSans-Italic.ttf | Same scripts; real italic | 2322640 | 58e6e0ebd1931b29a365aa2d3e2ee9a9e831a3af7cf3ad1462d4e72154f0b291 |
| NotoSansArabic.ttf | Arabic, Persian, Urdu | 844676 | 63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e |
| NotoSansSymbols2-Regular.ttf | Rating/action/status symbols | 1233128 | 7d5fb73b7ca67a6798101741f5d280a3d016a56a197afcd4199dbb57b4b82a21 |
| NotoSansSymbols.ttf | Arrow U+2192 (PDF sentiment line) | 372684 | f7e7e04b4a24b6c78893d50cbfd2b2f6cae49617ab047bfef668d252adb128f7 |
| NotoSansHebrew.ttf | Hebrew | 112640 | 7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88 |
| NotoSansArmenian.ttf | Armenian | 221936 | 0870908d8318435a5daf1cd280ae15063f990cd9ad60f3e94c734ce9e1ffef71 |
| NotoSansGeorgian.ttf | Georgian | 231988 | dc591156f36842d38996c4a7a17fee9bb58e45da3e2cac7a31b7d33de700adb9 |
| NotoSansDevanagari.ttf | Hindi, Marathi, Nepali | 641944 | 14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e |
| NotoSansKannada.ttf | Kannada | 641372 | cca4f3b3a8cb12fb261f1b43baf5d2f7f59d90fe123d41f0065ed3a183997ec9 |
| NotoSansTamil.ttf | Tamil | 340668 | aa3a9b321f4b0bb2c40203ffbde9af89713227866e0e13f76e5b9eeea727cf88 |
| NotoSansThai.ttf | Thai | 218652 | 5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8 |
| NotoSansCJKjp-Regular.otf | One pan-CJK regular: Han, Kana, Hangul, Bopomofo | 16467736 | 68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5 |

Extra-family notices are `NotoSans{Hebrew,Armenian,Georgian,Devanagari,Kannada,Tamil,Thai,CJK}-OFL.txt`. Google Fonts [family sources](https://github.com/google/fonts/tree/main/ofl) and [Noto CJK binary](https://github.com/notofonts/noto-cjk/blob/main/Sans/OTF/Japanese/NotoSansCJKjp-Regular.otf), [CJK OFL](https://github.com/notofonts/noto-cjk/blob/main/Sans/LICENSE) identify the upstreams; immutable blob identities are in provenance.json. The CJK copyright record retains © 2014–2021 Adobe. Fonts remain unmodified.

Call analysis_fonts/invoice_fonts over the already captured value, fetch only missing required extra families from bundled SAME-ORIGIN URLs, and add_fallback once per family to the cached writer. Serialized enum values map to the exact filenames through FallbackFont::filename(); no shell derives script rules. invoice_fonts scans only the effective Bill To plus captured facts, not overridden identity. No runtime CDN, system fonts, initial CJK byte payload or font requirement for DOCX/TXT/MD is introduced. Parsed regular/bold instances are cached; the regular-only CJK asset is reused without synthesizing bold or non-Latin italic.

This is target script coverage, **not observed all-language PDF acceptance**. Arabic shaping/logical extraction, regional Han forms, rare CJK extensions, style behavior and actual browser memory/payload remain verification gates. A concrete outside-pack candidate is Bengali `বাংলা` (first character ব U+09AC): the authored regression expects an explicit MissingGlyph rather than silent omission/substitution, but it has not run. Arbitrary Unicode/emoji coverage is not claimed.

Packaging interface already sent to V/22/Y before Claude suspension: include `transgate/export/fonts/*.ttf`, `*.otf`, `*-OFL.txt`, `*-COPYRIGHT.txt`, `fonts/provenance.json`, `assets/invoice-logo.jpg` and `assets/provenance.json` in the appropriate build context. V's URL module owns web imports and emitted notice URLs; font fetching/cache belongs to 22. Final web/Docker additions and served-byte hashes are not verified here.

## D15 approved plain-text output correction

Lead explicitly approved one changed output: source
`frontend/src/lib/ai/analysis-utils.ts:99-105` interpolates interview
`nextSteps` objects and emits `1. [object Object]`. The shared plain TXT/MD/copy
now emits `1. Arrange pairing (Priority: high)` for the named synthetic input.
`core/src/documents/fixtures/d15-nextsteps.json` records the exact full before
and after strings; `d15_interview_plain_next_steps_have_an_explicit_before_after_differential`
checks the corrected output. This is **not unchanged source parity**.
Source HTML already renders action/priority and is not normalized to the plain
format. Plain/HTML still omit `assessment.position`; HTML still omits
`concept.examples`. `source_format_specific_field_omissions_are_not_broadly_normalized`
retains those format-specific boundaries.

## Saved implementation and regression source

| Path | Responsibility |
|---|---|
| `transgate/core/src/documents.rs` | Owned serde capture, six typed format branches, text/HTML, source filenames/dates, bounded model and invoice capture |
| `transgate/core/src/documents/tests.rs`, `documents/fixtures/{analysis.json,d15-nextsteps.json}` | Partial ownership, nested source data/order, D15 differential/omissions, Bill To isolation and limits |
| `transgate/export/src/documents.rs` | Byte API, cached parsed assets and explicit encoding errors |
| `transgate/export/src/documents/{docx,pdf,text,fonts}.rs` | Real OOXML; Krilla PDF/invoice; bidi/shaping/wrapping; pure required-font selection |
| `transgate/export/tests/documents.rs`, `tests/fixtures/offered-scripts.json` | Independent ZIP/XML/lopdf readers, offered scripts, long RTL/wrapping/footer, authoritative invoice values |
| `transgate/export/tests/reference/capture-source.mjs` | Lead-only synthetic source capture, no source repo edits or replacement formatter |
| `transgate/export/fonts/`, `assets/` | Licensed binary pack, notices, source logo and provenance |

Core selectors, all under `documents::tests::`:

- `a_clicked_partial_is_owned_before_later_sections_arrive`
- `a_visible_streaming_summary_does_not_require_type_or_complete_sections`
- `six_shapes_keep_nested_sections_zero_false_and_non_latin_content`
- `format_specific_source_order_and_rating_representation_are_preserved`
- `d15_interview_plain_next_steps_have_an_explicit_before_after_differential`
- `source_format_specific_field_omissions_are_not_broadly_normalized`
- `temporary_bill_to_replaces_only_identity_not_financial_facts`
- `oversized_snapshots_and_invalid_offsets_fail_explicitly_without_truncation`

Native export test selectors:

- `six_analysis_shapes_are_real_docx_pdf_and_identical_txt_md_over_the_clicked_snapshot`
- `font_free_formats_preserve_all_source_unicode_without_loading_any_pdf_assets`
- `offered_language_scripts_request_only_their_lazy_fonts_and_roundtrip_real_pdf_unicode`
- `a_concrete_script_outside_the_offered_pack_is_reported_not_dropped_or_substituted`
- `independent_readers_preserve_nested_metadata_ratings_and_arabic_with_embedded_font_programs`
- `long_rtl_and_unbroken_text_wrap_without_losing_the_tail_and_every_page_has_a_footer`
- `ordinary_and_admin_invoice_pdf_keep_authoritative_facts_and_only_replace_the_bill_to_block`
- `invoice_font_requirements_follow_only_effective_bill_to_and_captured_facts`
- `generated_outputs_match_independently_captured_source_references` — explicitly ignored until real source captures exist, never a fabricated golden.

Lead-only commands, **not executed by W**:

```sh
cargo test --locked -p transgate-core documents::tests
cargo test --locked -p transgate-export --test documents
TZ=UTC TRANSGATE_DOCUMENT_REFERENCE_DIR="$PWD/.artifacts/p5-documents/source" node transgate/export/tests/reference/capture-source.mjs
TRANSGATE_DOCUMENT_REFERENCE_DIR="$PWD/.artifacts/p5-documents/source" cargo test --locked -p transgate-export --test documents generated_outputs_match_independently_captured_source_references -- --ignored --exact
```

The capture harness requires the original frontend's installed TypeScript/esbuild, docx, jsPDF, React PDF, React and date-fns packages. Native library modules remain native imports so their own font/resource paths survive; only original source TypeScript plus save/asset interception is bundled. The source tree is read-only, the output directory is lead-owned, and fixtures contain synthetic data only. The capture records source/package/library-entry/font hashes, UTC and click time. TXT/MD/HTML, real DOCX/PDF and ordinary/admin invoice PDF are saved through the actual source functions. Independent comparison normalizes extraction whitespace, preserves section order and permits only the named D15 string correction; it is not a visual-equivalence assertion. Arabic visual comparison, source Helvetica counterexample and non-UTC reference scenarios remain unobserved.

## Async delivery fence — consumer-owned, not a writer shortcut

The pure writer is deliberately auth-free. Immutable capture happens synchronously at the click; every asynchronous font completion must still pass the current **owner/session epoch, consent epoch and disposal guard immediately before clipboard/save**. Logout/login as the same owner, consent refusal/DELETE followed by regrant, and account switch must invalidate an older pending effect; checking only the current boolean/owner id is insufficient. Never replace the captured progress or timestamp with latest data when fonts arrive.

This interface and the meaningful held-font-load → revoke/session-switch → release → no-save/clipboard regression were handed to X/21/22 before the Claude hold. **22 owns the consumer implementation/regression; no saved selector or passing result has been received.** A positive held-load case should deliver the original click snapshot, not newer progress, when its guard remains valid. This is an existing integration gate, not permission for W to absorb suspended Claude code or introduce auth into the model.

## Frozen handoff and current hold

W's owned model/byte adapter/assets/regression source is coherent by source inspection, **not compiler or runtime evidence**. Manifest and module tree are saved; lock resolution and actual native/wasm/browser integration remain Y/X/22/lead-owned. No book, canonical ledger or unrelated Rust source was edited for this slice. No services, containers, smoke processes or scheduled work were started; the failed early scouts ended and were not restarted. Preserve the saved session and source. Notify lead only during the Claude hold; no direct peer prompts, new agents, retries/model transfers or next-phase work.
