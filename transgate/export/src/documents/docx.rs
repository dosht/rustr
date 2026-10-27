//! Analysis-specific OOXML over the existing export ZIP/XML primitives.
//! This is not the old transcript WordDocument: headings, lists, styled
//! metadata, document properties and script-aware runs are written explicitly.
use std::io::Cursor;
use transgate_core::documents::{AnalysisDocument, BlockRole, DocumentBlock, DocumentFormat, DocumentTone};
use unicode_bidi::BidiInfo;
use super::DocumentBytesError as Error;

const TYPES: &str = concat!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#);
const ROOT_RELS: &str = concat!(r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#);
const DOCUMENT_RELS: &str = concat!(r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdFonts" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/></Relationships>"#);
const FONTS: &str = concat!(r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:font w:name="Noto Sans"><w:family w:val="swiss"/></w:font><w:font w:name="Noto Sans Arabic"><w:family w:val="swiss"/></w:font><w:font w:name="Noto Sans Symbols 2"><w:family w:val="swiss"/></w:font></w:fonts>"#);
const STYLES: &str = concat!(r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Noto Sans" w:hAnsi="Noto Sans" w:cs="Noto Sans Arabic"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
    r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:rPr><w:b/><w:bCs/><w:sz w:val="52"/><w:szCs w:val="52"/></w:rPr></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:bCs/></w:rPr></w:style></w:styles>"#);
fn xml(text: &str) -> String { let mut out = String::new(); crate::escape_into(&mut out, text); out }
fn is_arabic(c: char) -> bool { matches!(u32::from(c), 0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff) }
fn family(c: char) -> &'static str {
    if is_arabic(c) { "Noto Sans Arabic" }
    else if matches!(u32::from(c), 0x2600..=0x27ff) { "Noto Sans Symbols 2" }
    else { "Noto Sans" }
}
fn text_run(out: &mut String, text: &str, font: &str, bold: bool, italic: bool, tone: DocumentTone, role: BlockRole) {
    out.push_str("<w:r><w:rPr>");
    out.push_str(&format!(r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="Noto Sans Arabic"/>"#));
    if bold { out.push_str("<w:b/><w:bCs/>"); }
    if italic { out.push_str("<w:i/><w:iCs/>"); }
    let color = match tone { DocumentTone::Muted => Some("888888"), DocumentTone::Positive => Some("2E7D32"), DocumentTone::Caution => Some("E65100"), DocumentTone::Negative => Some("C62828"), DocumentTone::Standard => match role { BlockRole::Metadata => Some("888888"), BlockRole::Footer => Some("999999"), _ => None } };
    if let Some(color) = color { out.push_str(&format!(r#"<w:color w:val="{color}"/>"#)); }
    if matches!(role, BlockRole::Metadata | BlockRole::Footer) && !italic { out.push_str("<w:i/><w:iCs/>"); }
    if role == BlockRole::Footer { out.push_str(r#"<w:sz w:val="18"/><w:szCs w:val="18"/>"#); }
    if font == "Noto Sans Arabic" { out.push_str("<w:rtl/>"); }
    out.push_str("</w:rPr>"); crate::run_content(out, text); out.push_str("</w:r>");
}
fn paragraph(out: &mut String, block: &DocumentBlock) {
    out.push_str("<w:p><w:pPr>");
    match block.role {
        BlockRole::Title => out.push_str(r#"<w:pStyle w:val="Title"/><w:jc w:val="center"/><w:spacing w:after="200"/>"#),
        BlockRole::Heading(level) => out.push_str(&format!(r#"<w:pStyle w:val="Heading{level}"/><w:keepNext/><w:spacing w:before="240" w:after="120"/>"#)),
        BlockRole::Metadata => out.push_str(r#"<w:jc w:val="center"/><w:spacing w:after="400"/>"#),
        BlockRole::Footer => out.push_str(r#"<w:jc w:val="center"/><w:spacing w:after="200"/>"#),
        BlockRole::Divider => out.push_str(r#"<w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="CCCCCC"/></w:pBdr><w:spacing w:before="120" w:after="120"/>"#),
        _ => out.push_str(if block.blank_after { r#"<w:spacing w:after="200"/>"# } else { r#"<w:spacing w:after="60"/>"# }),
    }
    let text = block.plain_text();
    if BidiInfo::new(&text, None).paragraphs.first().is_some_and(|p| p.level.is_rtl()) { out.push_str("<w:bidi/>"); }
    out.push_str("</w:pPr>");
    match block.role {
        BlockRole::Bullet => text_run(out, "  ", "Noto Sans", false, false, DocumentTone::Standard, block.role),
        BlockRole::Numbered(n) => text_run(out, &format!("{n}. "), "Noto Sans", false, false, DocumentTone::Standard, block.role), _ => {}
    }
    for run in &block.runs {
        let mut start = 0; let mut current = None;
        for (offset, c) in run.text.char_indices() {
            let next = family(c);
            if let Some(font) = current
                && font != next
            {
                text_run(out, &run.text[start..offset], font, run.bold, run.italic, run.tone, block.role);
                start = offset;
            }
            current = Some(next);
        }
        if let Some(font) = current { text_run(out, &run.text[start..], font, run.bold, run.italic, run.tone, block.role); }
    }
    out.push_str("</w:p>");
}
pub(super) fn encode(document: &AnalysisDocument) -> Result<Vec<u8>, Error> {
    let mut body = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#);
    for block in document.blocks(DocumentFormat::Docx) { paragraph(&mut body, &block); }
    body.push_str(r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#);
    let core = format!(r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Transgate AI</dc:creator><dc:title>{}</dc:title><dc:description>AI Analysis - {}</dc:description></cp:coreProperties>"#, xml(&document.metadata().title), document.type_name());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [("[Content_Types].xml", TYPES), ("_rels/.rels", ROOT_RELS), ("docProps/core.xml", &core), ("word/document.xml", &body), ("word/styles.xml", STYLES), ("word/fontTable.xml", FONTS), ("word/_rels/document.xml.rels", DOCUMENT_RELS)] {
        crate::write_part(&mut writer, name, content.as_bytes()).map_err(|e| Error::Encoding(e.to_string()))?;
    }
    Ok(writer.finish().map_err(|e| Error::Encoding(e.to_string()))?.into_inner())
}
