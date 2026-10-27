//! P5 FINAL-A: real format bytes read by independent XML/PDF readers.
//! Source-reference capture is a separate, explicit lead-run selector below.
#![cfg(not(target_arch = "wasm32"))]
use std::io::Read;
use std::sync::LazyLock;
use billing_core::decimal::Decimal;
use billing_core::presentation::invoice::{BillTo, InvoiceFacts, TaxKind};
use serde::Deserialize;
use transgate_core::ai::analysis::AnalysisProgress;
use transgate_core::ai::ports::GenerationStatus;
use transgate_core::documents::{AnalysisDocument, AnalysisMetadata, DocumentFormat, InvoiceDocument};
use transgate_core::{JobId, Micros};
use transgate_export::documents::{DocumentAssets, DocumentWriter, DocumentBytesError, FontBytes, FallbackFont, analysis_bytes, analysis_fonts, invoice_fonts};

#[derive(Deserialize)]
struct Case { name: String, progress: AnalysisProgress }
fn cases() -> Vec<Case> { serde_json::from_str(include_str!("../../core/src/documents/fixtures/analysis.json")).unwrap() }
fn assets() -> DocumentAssets {
    DocumentAssets {
        fonts: FontBytes {
            latin: include_bytes!("../fonts/NotoSans.ttf").to_vec(),
            latin_italic: include_bytes!("../fonts/NotoSans-Italic.ttf").to_vec(),
            arabic: include_bytes!("../fonts/NotoSansArabic.ttf").to_vec(),
            symbols: include_bytes!("../fonts/NotoSansSymbols2-Regular.ttf").to_vec(),
            arrows: include_bytes!("../fonts/NotoSansSymbols.ttf").to_vec(),
        },
        invoice_logo: include_bytes!("../assets/invoice-logo.jpg").to_vec(),
    }
}
fn writer() -> &'static DocumentWriter {
    static WRITER: LazyLock<DocumentWriter> = LazyLock::new(|| DocumentWriter::new(assets()).unwrap());
    &WRITER
}
fn document(case: &Case) -> AnalysisDocument {
    AnalysisDocument::capture(&case.progress, if case.name == "partial" { GenerationStatus::Interrupted } else { GenerationStatus::Completed },
        AnalysisMetadata { title: format!("Fixture {}", case.name), job_id: Some(JobId(7)), exported_at: Micros(0), utc_offset_minutes: 0 }).unwrap()
}
fn member(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = String::new(); archive.by_name(name).unwrap().read_to_string(&mut out).unwrap(); out
}
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
fn docx_text(bytes: &[u8]) -> String {
    let xml = member(bytes, "word/document.xml");
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    parsed.descendants().filter(|n| n.has_tag_name((W, "p")))
        .map(|p| p.descendants().filter(|n| n.has_tag_name((W, "t"))).filter_map(|n| n.text()).collect::<String>()).collect::<Vec<_>>().join("\n")
}
fn pdf_text(bytes: &[u8]) -> (lopdf::Document, String) {
    let pdf = lopdf::Document::load_mem(bytes).unwrap();
    let pages = pdf.get_pages().keys().copied().collect::<Vec<_>>();
    let text = pdf.extract_text(&pages).unwrap(); (pdf, text)
}

/// Independent, bidi-aware PDF reader: Poppler `pdftotext` (package
/// poppler-utils). The tests FAIL, never skip, when it is not installed, so a
/// gate image without it cannot silently drop the logical-Unicode checks.
/// Bidi embedding/isolate controls Poppler may add around RTL runs are removed.
fn poppler_text(bytes: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("transgate-export-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).unwrap();
    let output = std::process::Command::new("pdftotext").arg("-enc").arg("UTF-8").arg(&path).arg("-").output()
        .expect("poppler-utils `pdftotext` is required on PATH by the document PDF tests (install poppler-utils)");
    let _ = std::fs::remove_file(&path);
    assert!(output.status.success(), "pdftotext failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().chars().filter(|c| !matches!(u32::from(*c), 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)).collect()
}

/// Logical words of a page, read structurally instead of via `extract_text`. The
/// content stream is in visual order (and glyph-reordered for Indic and cursive
/// scripts), so ToUnicode of single glyphs cannot give exact words there. The
/// text units are an `/ActualText` span's string (the spec mechanism for the
/// logical text of a cluster drawn as several positioned glyphs), else each
/// glyph's ToUnicode character, in stream order; right-to-left text is reversed
/// from visual to logical order. `ET` separates text objects.
fn logical_words(bytes: &[u8], rtl: bool) -> Vec<String> {
    use lopdf::{content::Content, Object};
    let pdf = lopdf::Document::load_mem(bytes).unwrap();
    let mut units: Vec<String> = vec![];
    for page_id in pdf.get_pages().values() {
        let encodings: std::collections::BTreeMap<Vec<u8>, lopdf::Encoding> = pdf.get_page_fonts(*page_id).unwrap().into_iter()
            .map(|(name, font)| (name, font.get_font_encoding(&pdf).unwrap())).collect();
        let content = Content::decode(&pdf.get_page_content(*page_id)).unwrap();
        let (mut encoding, mut depth) = (None, 0usize);
        for op in &content.operations {
            match op.operator.as_str() {
                "Tf" => encoding = op.operands.first().and_then(|o| o.as_name().ok()).and_then(|n| encodings.get(n)),
                "BDC" => {
                    if depth > 0 { depth += 1; }
                    else if let Some(Object::Dictionary(properties)) = op.operands.get(1) {
                        if let Ok(actual) = properties.get(b"ActualText") { units.push(lopdf::decode_text_string(actual).unwrap()); depth = 1; }
                    }
                }
                "EMC" => depth = depth.saturating_sub(1),
                "Tj" | "TJ" if depth == 0 => {
                    fn shown(units: &mut Vec<String>, encoding: &lopdf::Encoding, operands: &[Object]) {
                        for operand in operands {
                            match operand {
                                Object::String(raw, _) => units.extend(encoding.bytes_to_string(raw).unwrap().chars().map(String::from)),
                                Object::Array(items) => shown(units, encoding, items),
                                _ => {}
                            }
                        }
                    }
                    shown(&mut units, encoding.expect("a font is selected before text"), &op.operands);
                }
                "ET" => units.push(" ".into()),
                _ => {}
            }
        }
    }
    let ordered: Vec<&str> = if rtl { units.iter().rev().map(String::as_str).collect() } else { units.iter().map(String::as_str).collect() };
    ordered.concat().split_whitespace().map(str::to_owned).collect()
}
fn compact(text: &str) -> String { text.split_whitespace().collect::<Vec<_>>().join(" ") }

#[test]
fn six_analysis_shapes_are_real_docx_pdf_and_identical_txt_md_over_the_clicked_snapshot() {
    for case in cases() {
        let model = document(&case);
        let docx = analysis_bytes(&model, DocumentFormat::Docx, None).unwrap();
        assert_eq!(docx.content_type, "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        let xml = member(&docx.bytes, "word/document.xml");
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        assert!(parsed.descendants().any(|n| n.has_tag_name((W, "pStyle")) && n.attribute((W, "val")) == Some("Title")));
        let word = docx_text(&docx.bytes);
        let pdf = analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())).unwrap();
        let (_, extracted) = pdf_text(&pdf.bytes);
        let summary = case.progress.summary.as_deref().unwrap();
        assert!(compact(&word).contains(summary), "{} DOCX {word}", case.name);
        assert!(compact(&extracted).contains(summary), "{} PDF {extracted}", case.name);
        let txt = analysis_bytes(&model, DocumentFormat::Txt, None).unwrap();
        let md = analysis_bytes(&model, DocumentFormat::Md, None).unwrap();
        assert_eq!(txt.bytes, md.bytes, "source TXT and MD are identical downloads");
        assert!(std::str::from_utf8(&txt.bytes).unwrap().contains(summary));
        assert_eq!(txt.filename, format!("fixture-{}-analysis.txt", case.name));
        assert_eq!(md.filename, format!("fixture-{}-analysis.md", case.name));
        if case.name == "partial" {
            assert!(word.contains("Only the delivered topic")); assert!(extracted.contains("Only the delivered topic"));
            assert!(!word.contains("Review Questions")); assert!(!extracted.contains("Review Questions"));
        }
    }
}

#[test]
fn font_free_formats_preserve_all_source_unicode_without_loading_any_pdf_assets() {
    let mut case = cases().into_iter().find(|c| c.name == "generic").unwrap();
    let text = "Հայերեն ქართული עברית हिन्दी ಕನ್ನಡ தமிழ் ไทย 中文 日本語 한국어 😀";
    case.progress.summary = Some(text.into());
    let model = document(&case);
    for format in [DocumentFormat::Txt, DocumentFormat::Md] {
        let bytes = analysis_bytes(&model, format, None).unwrap();
        assert!(String::from_utf8(bytes.bytes).unwrap().contains(text));
    }
    let bytes = analysis_bytes(&model, DocumentFormat::Docx, None).unwrap();
    assert!(docx_text(&bytes.bytes).contains(text), "OOXML retains Unicode independently of installed or PDF fonts");
    assert!(model.html().contains(text));
}

fn fallback_bytes(family: FallbackFont) -> &'static [u8] {
    match family {
        FallbackFont::Hebrew => include_bytes!("../fonts/NotoSansHebrew.ttf"),
        FallbackFont::Armenian => include_bytes!("../fonts/NotoSansArmenian.ttf"),
        FallbackFont::Georgian => include_bytes!("../fonts/NotoSansGeorgian.ttf"),
        FallbackFont::Devanagari => include_bytes!("../fonts/NotoSansDevanagari.ttf"),
        FallbackFont::Kannada => include_bytes!("../fonts/NotoSansKannada.ttf"),
        FallbackFont::Tamil => include_bytes!("../fonts/NotoSansTamil.ttf"),
        FallbackFont::Thai => include_bytes!("../fonts/NotoSansThai.ttf"),
        FallbackFont::Cjk => include_bytes!("../fonts/NotoSansCJKjp-Regular.otf"),
    }
}

#[test]
fn offered_language_scripts_request_only_their_lazy_fonts_and_roundtrip_real_pdf_unicode() {
    #[derive(Deserialize)]
    struct ScriptCase { script: String, text: String, required: Vec<FallbackFont> }
    let scripts: Vec<ScriptCase> = serde_json::from_str(include_str!("fixtures/offered-scripts.json")).unwrap();
    let mut writer = DocumentWriter::new(assets()).unwrap();
    assert!(FallbackFont::ALL.into_iter().all(|family| !writer.has_fallback(family)), "base PDF assets do not parse the lazy pack");
    for script in scripts {
        let mut case = cases().into_iter().find(|c| c.name == "generic").unwrap();
        case.progress.summary = Some(script.text.clone());
        let model = document(&case);
        assert_eq!(analysis_fonts(&model).unwrap(), script.required, "{}", script.script);
        for family in script.required {
            if !writer.has_fallback(family) { writer.add_fallback(family, fallback_bytes(family).to_vec()).unwrap(); }
        }
        let bytes = analysis_bytes(&model, DocumentFormat::Pdf, Some(&writer)).unwrap();
        let rtl = script.text.chars().any(|c| matches!(u32::from(c), 0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfeff));
        // Exact logical words from the ActualText spans plus the ToUnicode of the
        // remaining glyphs; not extract_text, which reads glyphs in visual order.
        let logical = logical_words(&bytes.bytes, rtl);
        for word in script.text.split_whitespace() {
            assert!(logical.iter().any(|w| w == word), "{} word {word:?} is not carried exactly; PDF logical words: {logical:?}", script.script);
        }
        // Independent reader: Poppler's page text contains each word once whitespace
        // (which it inserts between joined glyph chunks) is removed.
        let page = poppler_text(&bytes.bytes).split_whitespace().collect::<String>();
        for word in script.text.split_whitespace() {
            assert!(page.contains(word), "{} Poppler page text lacks {word:?}: {page}", script.script);
        }
    }
    assert!(FallbackFont::ALL.into_iter().all(|family| writer.has_fallback(family)));
}

#[test]
fn a_concrete_script_outside_the_offered_pack_is_reported_not_dropped_or_substituted() {
    let mut case = cases().into_iter().find(|c| c.name == "generic").unwrap();
    case.progress.summary = Some("বাংলা".into()); // Bengali is not in the offered 61-language table.
    let model = document(&case);
    assert!(analysis_fonts(&model).unwrap().is_empty());
    assert!(matches!(analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())), Err(DocumentBytesError::MissingGlyph('ব'))));
    assert!(String::from_utf8(analysis_bytes(&model, DocumentFormat::Txt, None).unwrap().bytes).unwrap().contains("বাংলা"));
    assert!(docx_text(&analysis_bytes(&model, DocumentFormat::Docx, None).unwrap().bytes).contains("বাংলা"));
}

#[test]
fn sentiment_progression_renders_the_arrow_in_pdf() {
    // Source quirk kept: only the PDF flavor prints "initial → final"; TXT, MD and DOCX
    // list Initial:/Final: fields instead.
    let case = cases().into_iter().find(|c| c.name == "support").unwrap();
    let model = document(&case);
    assert!(model.text().contains("frustrated") && model.text().contains("relieved") && !model.text().contains("→"));
    let pdf = analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())).expect("a sentiment progression must not make PDF export fail");
    let (_, text) = pdf_text(&pdf.bytes);
    assert!(compact(&text).contains(" → "), "arrow extracted from the PDF: {text}");
    assert!(compact(&poppler_text(&pdf.bytes)).contains(" → "), "independent Poppler extraction");
}

#[test]
fn independent_readers_preserve_nested_metadata_ratings_and_arabic_with_embedded_font_programs() {
    for case in cases().into_iter().filter(|c| ["generic", "meeting", "interview"].contains(&c.name.as_str())) {
        let model = document(&case);
        let docx = analysis_bytes(&model, DocumentFormat::Docx, None).unwrap();
        let word = docx_text(&docx.bytes);
        let bytes = analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())).unwrap().bytes;
        let (pdf, text) = pdf_text(&bytes);
        assert!(pdf.objects.values().any(|o| o.as_dict().is_ok_and(|d| d.has(b"FontFile2") || d.has(b"FontFile3"))), "fonts must be embedded, not replaced by a viewer default");
        match case.name.as_str() {
            "generic" => { assert!(word.contains("مرحبا بالعالم")); let logical = poppler_text(&bytes); assert!(compact(&logical).contains("مرحبا بالعالم"), "logical Unicode extraction: {logical}"); }
            "meeting" => { assert!(word.contains("Ana & Bob")); assert!(text.contains("Owner: Ana & Bob")); assert!(text.contains("Friday")); assert!(text.contains("1:05")); }
            "interview" => { assert!(word.contains("★★★☆ (3.5/5)")); assert!(text.contains("Overall Rating: 3.5/5")); assert!(text.contains("Priority: high")); assert!(text.contains("Check the generation before publication.")); }
            _ => unreachable!(),
        }
    }
}

#[test]
fn long_rtl_and_unbroken_text_wrap_without_losing_the_tail_and_every_page_has_a_footer() {
    let mut case = cases().into_iter().find(|c| c.name == "generic").unwrap();
    case.progress.summary = Some(format!("{}\n{} END_OF_CAPTURE", "مرحبا بالعالم. ".repeat(900), "longword".repeat(150)));
    let model = document(&case);
    let bytes = analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())).unwrap().bytes;
    let (pdf, text) = pdf_text(&bytes);
    let pages = pdf.get_pages(); assert!(pages.len() > 1);
    assert!(text.contains("END_OF_CAPTURE"));
    for number in pages.keys() {
        let page = pdf.extract_text(&[*number]).unwrap();
        assert!(page.contains(&format!("{number} / {}", pages.len())));
        assert!(page.contains("Generated by Transgate AI"));
    }
}

fn facts(tax_kind: TaxKind) -> InvoiceFacts {
    InvoiceFacts { payment_id: 42, invoice_id: "TRAN00000042".into(), date: "25/10/2026".into(), package_name: "Professional".into(), package_minutes: Some(600), quantity: 1, currency: if tax_kind == TaxKind::Vat20 { "TRY" } else { "USD" }.into(), subtotal: Decimal::parse("100.00").unwrap(), tax: Decimal::parse(if tax_kind == TaxKind::Vat20 { "20.00" } else { "0.00" }).unwrap(), total: Decimal::parse(if tax_kind == TaxKind::Vat20 { "120.00" } else { "100.00" }).unwrap(), tax_kind, gateway: "iyzico".into() }
}
fn customer() -> BillTo { BillTo { name: Some("Owner".into()), company: None, address_lines: vec!["Stored address".into()], country: None, tax_id: None } }

#[test]
fn ordinary_and_admin_invoice_pdf_keep_authoritative_facts_and_only_replace_the_bill_to_block() {
    for tax in [TaxKind::Vat20, TaxKind::None] {
        for custom in ["", "Temporary Company\nمرحبا بالعالم\nTax 456"] {
            let facts = facts(tax);
            let model = InvoiceDocument::capture(&facts, &customer(), custom).unwrap();
            let bytes = writer().invoice(&model).unwrap();
            assert_eq!(bytes.filename, "invoice-TRAN00000042.pdf");
            let (pdf, text) = pdf_text(&bytes.bytes);
            for expected in ["INVOICE", "#TRAN00000042", "25/10/2026", "Transgate.ai / Mustafa Abdelhamid", "600 minutes", "100.00", "Payment processed through iyzico", "support@transgate.ai"] { assert!(text.contains(expected), "missing {expected}: {text}"); }
            assert!(pdf.objects.values().any(|o| o.as_stream().is_ok_and(|s| s.dict.get(b"Subtype").is_ok_and(|v| v.as_name().ok() == Some(b"Image")))), "real source logo is an embedded image");
            if custom.is_empty() { assert!(text.contains("Owner")); assert!(text.contains("Stored address")); }
            else { assert!(text.contains("Temporary Company")); assert!(!text.contains("Stored address")); assert!(!text.contains("Owner")); }
            if tax == TaxKind::Vat20 { assert!(text.contains("VAT (20%)")); assert!(text.contains("VAT Registration Number: 6221748346")); assert!(text.contains("120.00")); }
            else { assert!(text.contains("Tax (0%)")); assert!(!text.contains("VAT Registration Number:")); }
        }
    }
}

#[test]
fn invoice_font_requirements_follow_only_effective_bill_to_and_captured_facts() {
    let mut owner = customer(); owner.name = Some("客户".into());
    let facts = facts(TaxKind::Vat20);
    let ordinary = InvoiceDocument::capture(&facts, &owner, "").unwrap();
    assert_eq!(invoice_fonts(&ordinary).unwrap(), vec![FallbackFont::Cjk]);
    let custom = InvoiceDocument::capture(&facts, &owner, "Temporary Company").unwrap();
    assert!(invoice_fonts(&custom).unwrap().is_empty(), "replaced identity must not trigger a private or unnecessary CJK asset dependency");
    assert_eq!(custom.facts(), ordinary.facts());
}

#[test]
#[ignore = "Lead captures source outputs with reference/capture-source.mjs, then sets TRANSGATE_DOCUMENT_REFERENCE_DIR; no fabricated goldens"]
fn generated_outputs_match_independently_captured_source_references() {
    let directory = std::path::PathBuf::from(std::env::var_os("TRANSGATE_DOCUMENT_REFERENCE_DIR").expect("source capture directory is required"));
    for case in cases() {
        let model = document(&case);
        let stem = directory.join(&case.name);
        let mut source_text = std::fs::read_to_string(stem.with_extension("txt")).unwrap();
        if case.name == "interview" {
            // Lead-approved D15 only: original JS object interpolation is wrong.
            source_text = source_text.replacen("1. [object Object]", "1. Arrange pairing (Priority: high)", 1);
        }
        assert_eq!(compact(&model.text()), compact(&source_text), "{} source TXT/MD content/order, with only D15 content correction", case.name);
        let source_word = docx_text(&std::fs::read(stem.with_extension("docx")).unwrap());
        let ours = docx_text(&analysis_bytes(&model, DocumentFormat::Docx, None).unwrap().bytes);
        assert_eq!(compact(&ours), compact(&source_word), "{} source DOCX text/order", case.name);
        let (_, source_pdf) = pdf_text(&std::fs::read(stem.with_extension("pdf")).unwrap());
        let (_, our_pdf) = pdf_text(&analysis_bytes(&model, DocumentFormat::Pdf, Some(writer())).unwrap().bytes);
        assert_eq!(compact(&our_pdf), compact(&source_pdf), "{} source PDF text/order; visual comparison is separate", case.name);
    }
    for (name, custom) in [("invoice", ""), ("admin-invoice", "Temporary Company\nمرحبا بالعالم\nTax 456")] {
        let model = InvoiceDocument::capture(&facts(TaxKind::Vat20), &customer(), custom).unwrap();
        let (_, original) = pdf_text(&std::fs::read(directory.join(format!("{name}.pdf"))).unwrap());
        let (_, generated) = pdf_text(&writer().invoice(&model).unwrap().bytes);
        assert_eq!(compact(&generated), compact(&original), "{name} source invoice facts/text/order");
    }
}
