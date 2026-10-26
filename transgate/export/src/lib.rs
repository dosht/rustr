//! Byte adapters for Transgate exports. Core decides every file, name and text
//! (`transgate_core::export`); this crate only writes them: the zip container
//! and the OOXML parts of a `WordDocument`.
//!
//! Output is deterministic (fixed entry timestamps, fixed order) so the same
//! bundle always gives the same bytes. Names are checked here too: a name that
//! could leave the folder when extracted is refused, whatever produced it.

/// Captured analysis and authoritative invoice document bytes (P5 FINAL-A).
pub mod documents;

use std::fmt;
use std::io::{Cursor, Write};

use transgate_core::export::{EntryContent, ExportBundle, WordDocument, WordParagraph, WordRun};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// `application/zip`, for the download's content type.
pub const ZIP_CONTENT_TYPE: &str = "application/zip";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportBytesError {
    /// A name that could escape the folder, collide with another, or is empty.
    Invalid(String),
    /// The container could not be written.
    Zip(String),
}

impl fmt::Display for ExportBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportBytesError::Invalid(why) => write!(f, "invalid export: {why}"),
            ExportBytesError::Zip(why) => write!(f, "the export could not be written: {why}"),
        }
    }
}

impl std::error::Error for ExportBytesError {}

fn zip_error(e: impl fmt::Display) -> ExportBytesError {
    ExportBytesError::Zip(e.to_string())
}

/// One path component of the archive: not empty, not `.` or `..`, no separator or control character.
fn check_component(what: &str, name: &str) -> Result<(), ExportBytesError> {
    let bad = name.is_empty() || name == "." || name == ".." || name.chars().any(|c| c == '/' || c == '\\' || c.is_control());
    if bad { Err(ExportBytesError::Invalid(format!("{what} {name:?} is not a safe file name"))) } else { Ok(()) }
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn write_part(writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) -> Result<(), ExportBytesError> {
    writer.start_file(name, options()).map_err(zip_error)?;
    writer.write_all(bytes).map_err(zip_error)
}

/// The zip a download delivers: the folder entry, then every core entry under it, in core's order.
/// A Word entry is written as its OOXML package.
pub fn zip_bytes(bundle: &ExportBundle) -> Result<Vec<u8>, ExportBytesError> {
    check_component("the folder", &bundle.folder)?;
    let mut seen: Vec<&str> = Vec::new();
    for entry in &bundle.entries {
        check_component("the entry", &entry.name)?;
        if seen.contains(&entry.name.as_str()) {
            return Err(ExportBytesError::Invalid(format!("two entries are named {:?}", entry.name)));
        }
        seen.push(&entry.name);
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_directory(format!("{}/", bundle.folder), options()).map_err(zip_error)?;
    for entry in &bundle.entries {
        let path = format!("{}/{}", bundle.folder, entry.name);
        match &entry.content {
            EntryContent::Text { text } => write_part(&mut writer, &path, text.as_bytes())?,
            EntryContent::Word { document } => write_part(&mut writer, &path, &docx_bytes(document)?)?,
        }
    }
    Ok(writer.finish().map_err(zip_error)?.into_inner())
}

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    r#"<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#,
    r#"</Types>"#
);

const PACKAGE_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>"#,
    r#"</Relationships>"#
);

/// XML 1.0 `Char`: tab, line feed, carriage return and everything from space up except the
/// noncharacters U+FFFE and U+FFFF. Surrogates cannot occur in a Rust `char`.
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars().filter(|c| xml_char(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// A run's content: text pieces with a Word line break for each newline and a tab for each tab.
/// Characters XML cannot carry are dropped, never written as something else.
fn run_content(out: &mut String, text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut piece = String::new();
    let flush = |out: &mut String, piece: &mut String| {
        if !piece.is_empty() {
            out.push_str(r#"<w:t xml:space="preserve">"#);
            escape_into(out, piece);
            out.push_str("</w:t>");
            piece.clear();
        }
    };
    for c in normalized.chars() {
        match c {
            '\n' => {
                flush(out, &mut piece);
                out.push_str("<w:br/>");
            }
            '\t' => {
                flush(out, &mut piece);
                out.push_str("<w:tab/>");
            }
            c => piece.push(c),
        }
    }
    flush(out, &mut piece);
}

fn write_run(out: &mut String, run: &WordRun) {
    out.push_str("<w:r><w:rPr>");
    let mut font = String::new();
    escape_into(&mut font, &run.font);
    if !font.is_empty() {
        out.push_str(&format!(r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:eastAsia="{font}" w:cs="{font}"/>"#));
    }
    if run.bold {
        out.push_str("<w:b/><w:bCs/>");
    }
    out.push_str("</w:rPr>");
    run_content(out, &run.text);
    out.push_str("</w:r>");
}

fn write_paragraph(out: &mut String, paragraph: &WordParagraph) {
    out.push_str("<w:p>");
    if paragraph.spacing_before.is_some() || paragraph.spacing_after.is_some() {
        out.push_str("<w:pPr><w:spacing");
        if let Some(before) = paragraph.spacing_before {
            out.push_str(&format!(r#" w:before="{before}""#));
        }
        if let Some(after) = paragraph.spacing_after {
            out.push_str(&format!(r#" w:after="{after}""#));
        }
        out.push_str("/></w:pPr>");
    }
    for run in &paragraph.runs {
        write_run(out, run);
    }
    out.push_str("</w:p>");
}

fn document_xml(document: &WordDocument) -> String {
    let mut out = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#
    ));
    for paragraph in &document.paragraphs {
        write_paragraph(&mut out, paragraph);
    }
    // The page the `docx` package writes by default: A4, one-inch margins.
    out.push_str(concat!(
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>"#,
        "</w:body></w:document>"
    ));
    out
}

fn core_xml(document: &WordDocument) -> String {
    let mut creator = String::new();
    escape_into(&mut creator, &document.creator);
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" "#,
            r#"xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            "<dc:creator>{}</dc:creator></cp:coreProperties>"
        ),
        creator
    )
}

/// A `.docx` package for a core `WordDocument`: one part per concern, each paragraph with its
/// own spacing, each run with its font and weight. Text direction is not forced: the source
/// sets none, so right-to-left text is written as it is and the reader's bidi rules apply.
pub fn docx_bytes(document: &WordDocument) -> Result<Vec<u8>, ExportBytesError> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_part(&mut writer, "[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    write_part(&mut writer, "_rels/.rels", PACKAGE_RELS.as_bytes())?;
    write_part(&mut writer, "docProps/core.xml", core_xml(document).as_bytes())?;
    write_part(&mut writer, "word/document.xml", document_xml(document).as_bytes())?;
    Ok(writer.finish().map_err(zip_error)?.into_inner())
}
