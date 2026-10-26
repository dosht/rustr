use super::*;
use crate::ai::analysis::{Analysis, GenericAnalysis, DetectedType};
use billing_core::decimal::Decimal;
use billing_core::presentation::invoice::{BillTo, InvoiceFacts, TaxKind};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case { name: String, progress: AnalysisProgress }
fn cases() -> Vec<Case> { serde_json::from_str(include_str!("fixtures/analysis.json")).unwrap() }
fn metadata() -> AnalysisMetadata {
    AnalysisMetadata { title: "Review: A/B <final>".into(), job_id: Some(JobId(7)), exported_at: Micros(0), utc_offset_minutes: -60 }
}
fn capture(progress: &AnalysisProgress) -> AnalysisDocument {
    AnalysisDocument::capture(progress, GenerationStatus::Completed, metadata()).unwrap()
}

#[test]
fn a_clicked_partial_is_owned_before_later_sections_arrive() {
    let mut progress = cases().pop().unwrap().progress;
    let doc = AnalysisDocument::capture(&progress, GenerationStatus::Interrupted, metadata()).unwrap();
    progress.summary = Some("A later saved result that must not replace the clicked partial".into());
    progress.analysis = Some(Analysis::Generic(GenericAnalysis { summary: progress.summary.clone(), highlights: Some(vec!["later".into()]) }));
    assert!(doc.text().contains("Visible partial, not a later result."));
    assert!(doc.text().contains("Only the delivered topic"));
    assert!(!doc.text().contains("later saved result"));
    assert!(!doc.text().contains("Review Questions"));
    assert_eq!(doc.filename(DocumentFormat::Pdf), "review-ab-final-analysis.pdf");
    assert_eq!(doc.export_date(false), "December 31, 1969", "click instant is formatted with its captured local offset");
    assert_eq!(doc.export_date(true), "Wednesday, December 31, 1969");
}

#[test]
fn a_visible_streaming_summary_does_not_require_type_or_complete_sections() {
    let only_summary = AnalysisProgress { summary: Some("Already visible".into()), ..Default::default() };
    let doc = AnalysisDocument::capture(&only_summary, GenerationStatus::Running, metadata()).unwrap();
    assert!(doc.text().contains("Already visible"));
    assert_eq!(AnalysisDocument::capture(&AnalysisProgress::default(), GenerationStatus::Running, metadata()), Err(DocumentError::NoContent));
}

#[test]
fn six_shapes_keep_nested_sections_zero_false_and_non_latin_content() {
    let expected = [
        ("generic", vec!["Captured common summary & <markup>.", "First highlight", "مرحبا بالعالم"]),
        ("meeting", vec!["Action Items", "Ana & Bob", "Friday", "Decisions Made", "1:05", "Risk and recovery", "Send the review"]),
        ("lecture", vec!["Topics Covered", "2:05", "Study Guide", "Fence", "session epoch, generation id", "What protects the boundary?"]),
        ("sales", vec!["Schedule the demo", "Manual work", "Budget approved", "Migration effort", "Requested a trial", "Listened carefully", "Ask about procurement", "Questions Asked: 0", "Next Steps Confirmed: No"]),
        ("support", vec!["Clear explanation", "Explain session expiry", "Expired session", "Sign in again", "Add expiry guidance", "Check the next export", "Recovered the document"]),
        ("interview", vec!["Rust", "Advanced", "3.5/5", "Precise reasoning", "Needs deployment practice", "How do you fence a late result?", "Only with storage evidence."]),
    ];
    for (name, needles) in expected {
        let case = cases().into_iter().find(|c| c.name == name).unwrap();
        let doc = capture(&case.progress);
        let text = doc.text();
        for needle in needles { assert!(text.contains(needle), "{name}: missing {needle:?} in {text}"); }
        let html = doc.html();
        assert!(!html.contains("<markup>"), "model output is text, never executable clipboard markup");
        assert!(!html.contains("<script"));
    }
}

#[test]
fn format_specific_source_order_and_rating_representation_are_preserved() {
    let all = cases();
    let lecture = capture(&all.iter().find(|c| c.name == "lecture").unwrap().progress);
    let docx = lecture.blocks(DocumentFormat::Docx).iter().map(DocumentBlock::plain_text).collect::<Vec<_>>().join("\n");
    let pdf = lecture.blocks(DocumentFormat::Pdf).iter().map(DocumentBlock::plain_text).collect::<Vec<_>>().join("\n");
    assert!(docx.find("Key Concepts").unwrap() < docx.find("Study Guide").unwrap());
    assert!(pdf.find("Study Guide").unwrap() < pdf.find("Key Concepts").unwrap());
    let interview = capture(&all.iter().find(|c| c.name == "interview").unwrap().progress);
    let docx = interview.blocks(DocumentFormat::Docx).iter().map(DocumentBlock::plain_text).collect::<Vec<_>>().join("\n");
    let pdf = interview.blocks(DocumentFormat::Pdf).iter().map(DocumentBlock::plain_text).collect::<Vec<_>>().join("\n");
    assert!(docx.contains("★★★☆ (3.5/5)"), "source String.repeat floors both star counts");
    assert!(pdf.contains("Overall Rating: 3.5/5"));
    assert!(docx.contains("Priority: high"));
}

#[test]
fn d15_interview_plain_next_steps_have_an_explicit_before_after_differential() {
    #[derive(Deserialize)]
    struct Difference { analysis: Analysis, before: String, after: String }
    let diff: Difference = serde_json::from_str(include_str!("fixtures/d15-nextsteps.json")).unwrap();
    let progress = AnalysisProgress { analysis: Some(diff.analysis), ..Default::default() };
    let doc = capture(&progress);
    assert_eq!(doc.text(), diff.after);
    assert_ne!(doc.text(), diff.before, "D15 is an approved changed output, not a parity claim");
    assert!(doc.html().contains("Arrange pairing"));
    assert!(!doc.html().contains("Priority: high"), "HTML already rendered a priority badge; do not normalize it to plain text");
}

#[test]
fn source_format_specific_field_omissions_are_not_broadly_normalized() {
    let all = cases();
    let lecture = capture(&all.iter().find(|c| c.name == "lecture").unwrap().progress);
    assert!(lecture.text().contains("session epoch, generation id"));
    assert!(!lecture.html().contains("session epoch"));
    let interview = capture(&all.iter().find(|c| c.name == "interview").unwrap().progress);
    assert!(!interview.text().contains("Rust engineer"));
    assert!(!interview.html().contains("Rust engineer"));
    assert!(interview.blocks(DocumentFormat::Pdf).iter().any(|b| b.plain_text().contains("Rust engineer")));
}

fn invoice() -> InvoiceFacts {
    InvoiceFacts { payment_id: 42, invoice_id: "TRAN00000042".into(), date: "25/10/2026".into(), package_name: "Professional".into(), package_minutes: Some(600), quantity: 1, currency: "TRY".into(), subtotal: Decimal::parse("100.00").unwrap(), tax: Decimal::parse("20.00").unwrap(), total: Decimal::parse("120.00").unwrap(), tax_kind: TaxKind::Vat20, gateway: "iyzico".into() }
}
fn bill_to() -> BillTo {
    BillTo { name: Some("Owner".into()), company: Some("Owner Company".into()), address_lines: vec!["Stored address".into()], country: Some("Türkiye".into()), tax_id: Some("123".into()) }
}

#[test]
fn temporary_bill_to_replaces_only_identity_not_financial_facts() {
    let facts = invoice();
    let doc = InvoiceDocument::capture(&facts, &bill_to(), "\n  Temp & Co  \n\nمرحبا\n").unwrap();
    assert_eq!(doc.bill_to(), &["  Temp & Co  ".to_string(), "مرحبا".to_string()]);
    assert_eq!(doc.facts(), &facts);
    assert_eq!(doc.filename(), "invoice-TRAN00000042.pdf");
    let ordinary = InvoiceDocument::capture(&facts, &bill_to(), " \n\t").unwrap();
    assert_eq!(ordinary.bill_to(), &["Owner", "Owner Company", "Stored address", "Türkiye", "Tax ID: 123"]);
    let mut anonymous = bill_to(); anonymous.name = None;
    assert_eq!(InvoiceDocument::capture(&facts, &anonymous, "").unwrap().bill_to()[0], "Valued Customer");
    assert_eq!(facts.total, Decimal::parse("120.00").unwrap(), "no document-side recomputation");
}

#[test]
fn oversized_snapshots_and_invalid_offsets_fail_explicitly_without_truncation() {
    let mut progress = AnalysisProgress { detected: Some(DetectedType { kind: AnalysisType::Generic, confidence: 1.0 }), summary: Some("x".repeat(MAX_CAPTURE_BYTES + 1)), ..Default::default() };
    assert_eq!(AnalysisDocument::capture(&progress, GenerationStatus::Completed, metadata()), Err(DocumentError::LimitExceeded));
    progress.summary = Some("valid".into());
    let mut meta = metadata(); meta.utc_offset_minutes = 1440;
    assert_eq!(AnalysisDocument::capture(&progress, GenerationStatus::Completed, meta), Err(DocumentError::InvalidMetadata));
}
