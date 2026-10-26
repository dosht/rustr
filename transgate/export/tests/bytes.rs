//! The byte adapters over the real core export model: the zip a user downloads and
//! the Word parts inside it, read back by readers that are not the writers.

use std::io::Read;

use transgate_core::export::{self, EntryContent, ExportBundle, ExportEntry, ExportLabels, WordDocument, WordParagraph, WordRun};
use transgate_core::transcript::{Line, TranslationStatus};
use transgate_core::{AccountId, JobId, LineId, Micros};
use transgate_export::{ExportBytesError, docx_bytes, zip_bytes};

fn line(id: i64, start: &str, end: &str, text: &str, speaker: Option<&str>) -> Line {
    Line {
        id: LineId(id),
        job: JobId(7),
        account: AccountId(uuid::Uuid::nil()),
        start_time: start.into(),
        end_time: end.into(),
        line: text.into(),
        speaker: speaker.map(str::to_string),
        corrected_text: None,
        split_on_speaker_change: None,
        language: None,
        translation_status: None::<TranslationStatus>,
        created_at: Micros(0),
    }
}

fn lines() -> Vec<Line> {
    vec![
        line(1, "00:00:01,000", "00:00:04,000", "Hello & welcome, <everyone>.", Some("SPEAKER_00")),
        line(2, "00:00:05,000", "00:00:09,000", "  second line with \"quotes\"  ", Some("SPEAKER_01")),
        line(3, "00:00:40,000", "00:00:44,000", "مرحبا بالعالم", Some("SPEAKER_00")),
    ]
}

fn names(bytes: &[u8]) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a readable zip");
    (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned()).collect()
}

fn member(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    archive.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

#[test]
fn the_zip_holds_the_folder_and_every_core_entry_as_utf8_text() {
    let bundle = export::all_formats(Some("Interview: part 1.mp4"), true, &lines(), &ExportLabels::default());
    let bytes = zip_bytes(&bundle).unwrap();

    let folder = &bundle.folder;
    assert_eq!(folder, "Interview part 1_transcript");
    let mut expected = vec![format!("{folder}/")];
    expected.extend(bundle.entries.iter().map(|e| format!("{folder}/{}", e.name)));
    assert_eq!(names(&bytes), expected);

    for entry in &bundle.entries {
        if let EntryContent::Text { text } = &entry.content {
            assert_eq!(member(&bytes, &format!("{folder}/{}", entry.name)), text.as_bytes(), "{} is the core text, byte for byte, with no BOM", entry.name);
        }
    }
    let srt = member(&bytes, &format!("{folder}/Interview part 1.srt"));
    assert!(String::from_utf8(srt).unwrap().starts_with("1\n"));
}

#[test]
fn the_same_bundle_always_gives_the_same_bytes() {
    let bundle = export::wysiwyg(Some("a.mp3"), true, &lines(), true, true, &ExportLabels::default());
    assert_eq!(zip_bytes(&bundle).unwrap(), zip_bytes(&bundle).unwrap());
}

fn text_entry(name: &str) -> ExportEntry {
    ExportEntry { name: name.into(), content: EntryContent::Text { text: "x".into() } }
}

#[test]
fn names_that_could_escape_the_folder_or_collide_are_refused() {
    let bundle = |folder: &str, entries: Vec<ExportEntry>| ExportBundle { zip_name: "z.zip".into(), folder: folder.into(), entries };
    for bad in ["../x.txt", "a/b.txt", "a\\b.txt", "", "..", ".", "x\0.txt"] {
        assert!(matches!(zip_bytes(&bundle("ok", vec![text_entry(bad)])), Err(ExportBytesError::Invalid(_))), "entry {bad:?}");
    }
    for bad in ["../up", "a/b", "", ".."] {
        assert!(matches!(zip_bytes(&bundle(bad, vec![text_entry("a.txt")])), Err(ExportBytesError::Invalid(_))), "folder {bad:?}");
    }
    assert!(matches!(zip_bytes(&bundle("ok", vec![text_entry("a.txt"), text_entry("a.txt")])), Err(ExportBytesError::Invalid(_))), "a duplicate name would shadow a file");
}

fn document_xml(docx: &[u8]) -> String {
    String::from_utf8(member(docx, "word/document.xml")).unwrap()
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[test]
fn a_word_document_is_well_formed_ooxml_that_keeps_text_fonts_bold_and_spacing() {
    let document = export::word_timestamped(&lines());
    let docx = docx_bytes(&document).unwrap();
    assert_eq!(names(&docx).first().map(String::as_str), Some("[Content_Types].xml"), "the content types part comes first");

    for part in ["[Content_Types].xml", "_rels/.rels", "docProps/core.xml", "word/document.xml"] {
        let xml = String::from_utf8(member(&docx, part)).unwrap();
        roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{part} is not well-formed: {e}"));
    }
    let xml = document_xml(&docx);
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let paragraphs: Vec<_> = parsed.descendants().filter(|n| n.has_tag_name((W, "p"))).collect();
    assert_eq!(paragraphs.len(), 3);

    let text_of = |p: &roxmltree::Node| -> String { p.descendants().filter(|n| n.has_tag_name((W, "t"))).filter_map(|n| n.text()).collect() };
    assert_eq!(text_of(&paragraphs[0]), "00:00:01 Hello & welcome, <everyone>.", "markup characters survive as text");
    assert_eq!(text_of(&paragraphs[1]), "00:00:05 second line with \"quotes\"", "the line is trimmed by core, the time keeps its trailing space");
    assert_eq!(text_of(&paragraphs[2]), "00:00:40 مرحبا بالعالم", "right-to-left text is written as it is, the reader's bidi rules apply");

    let runs: Vec<_> = paragraphs[0].children().filter(|n| n.has_tag_name((W, "r"))).collect();
    assert_eq!(runs.len(), 2);
    let font = |run: &roxmltree::Node| run.descendants().find(|n| n.has_tag_name((W, "rFonts"))).and_then(|n| n.attribute((W, "ascii"))).map(str::to_string);
    let bold = |run: &roxmltree::Node| run.descendants().any(|n| n.has_tag_name((W, "b")));
    assert_eq!((font(&runs[0]), bold(&runs[0])), (Some("Consolas".into()), true));
    assert_eq!((font(&runs[1]), bold(&runs[1])), (Some("Arial".into()), false));

    let spacing = paragraphs[0].descendants().find(|n| n.has_tag_name((W, "spacing"))).unwrap();
    assert_eq!((spacing.attribute((W, "after")), spacing.attribute((W, "before"))), (Some("120"), None));
    let time_run = runs[0].descendants().find(|n| n.has_tag_name((W, "t"))).unwrap();
    assert_eq!(time_run.attribute(("http://www.w3.org/XML/1998/namespace", "space")), Some("preserve"), "the trailing space after the time is kept");

    let core = String::from_utf8(member(&docx, "docProps/core.xml")).unwrap();
    let core = roxmltree::Document::parse(&core).unwrap();
    let creator = core.descendants().find(|n| n.tag_name().name() == "creator").and_then(|n| n.text());
    assert_eq!(creator, Some("Transgate"));
}

#[test]
fn the_speaker_document_keeps_its_headings_and_per_paragraph_spacing() {
    let document = export::word_speakers(&lines(), &ExportLabels::default());
    let xml = document_xml(&docx_bytes(&document).unwrap());
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let spacings: Vec<(Option<&str>, Option<&str>)> =
        parsed.descendants().filter(|n| n.has_tag_name((W, "spacing"))).map(|n| (n.attribute((W, "before")), n.attribute((W, "after")))).collect();
    assert_eq!(spacings[0], (Some("0"), Some("60")), "the first heading has no space before it");
    assert_eq!(spacings[1], (None, Some("200")));
    assert_eq!(spacings[2], (Some("200"), Some("60")));
}

#[test]
fn characters_xml_cannot_carry_are_dropped_and_line_breaks_and_tabs_stay_breaks_and_tabs() {
    let document = WordDocument {
        creator: "Transgate & Co".into(),
        paragraphs: vec![WordParagraph {
            runs: vec![WordRun { text: "a\u{0}b\u{1}c\u{b}d\u{fffe}e\r\nline two\tindented".into(), font: "Ar\"ial".into(), bold: false }],
            spacing_before: None,
            spacing_after: None,
        }],
    };
    let docx = docx_bytes(&document).unwrap();
    let xml = document_xml(&docx);
    let parsed = roxmltree::Document::parse(&xml).expect("still well-formed");
    let paragraph = parsed.descendants().find(|n| n.has_tag_name((W, "p"))).unwrap();
    assert!(paragraph.descendants().any(|n| n.has_tag_name((W, "br"))), "a newline is a Word line break");
    assert!(paragraph.descendants().any(|n| n.has_tag_name((W, "tab"))));
    let text: String = paragraph.descendants().filter(|n| n.has_tag_name((W, "t"))).filter_map(|n| n.text()).collect();
    assert_eq!(text, "abcdeline twoindented");
    let font = paragraph.descendants().find(|n| n.has_tag_name((W, "rFonts"))).and_then(|n| n.attribute((W, "ascii")));
    assert_eq!(font, Some("Ar\"ial"), "attribute values are escaped, not mangled");
    assert!(paragraph.descendants().all(|n| !n.has_tag_name((W, "pPr"))), "no spacing means no paragraph properties");
    let core = roxmltree::Document::parse(std::str::from_utf8(&member(&docx, "docProps/core.xml")).unwrap()).unwrap().descendants().find(|n| n.tag_name().name() == "creator").and_then(|n| n.text().map(str::to_string));
    assert_eq!(core.as_deref(), Some("Transgate & Co"));
}

#[test]
fn an_empty_document_is_still_a_valid_package() {
    let docx = docx_bytes(&WordDocument { creator: "Transgate".into(), paragraphs: vec![] }).unwrap();
    let xml = document_xml(&docx);
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    assert!(parsed.descendants().any(|n| n.has_tag_name((W, "body"))));
}

#[test]
fn a_word_entry_inside_the_zip_is_the_same_package_as_the_standalone_document() {
    let bundle = export::wysiwyg(Some("talk.mp3"), false, &lines(), false, false, &ExportLabels::default());
    let bytes = zip_bytes(&bundle).unwrap();
    let EntryContent::Word { document } = &bundle.entries.iter().find(|e| e.name == "talk.doc").unwrap().content else { panic!("the Word entry") };
    let inner = member(&bytes, "talk_transcript/talk.doc");
    assert_eq!(inner, docx_bytes(document).unwrap());
    assert_eq!(names(&inner).len(), 4, "content types, rels, core properties and the document");
}
