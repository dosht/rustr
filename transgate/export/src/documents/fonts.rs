//! P5D18: bounded, named, same-origin fallback assets. This scans the captured
//! typed value without constructing a second JSON/string copy. It selects
//! assets, not content or provider policy. No network exists in this module.
use std::io;
use serde::{Deserialize, Serialize};
use transgate_core::documents::{AnalysisDocument, InvoiceDocument, MAX_CAPTURE_BYTES};
use super::DocumentBytesError as Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum FallbackFont { Hebrew, Armenian, Georgian, Devanagari, Kannada, Tamil, Thai, Cjk }
impl FallbackFont {
    pub const ALL: [Self; 8] = [Self::Hebrew, Self::Armenian, Self::Georgian, Self::Devanagari, Self::Kannada, Self::Tamil, Self::Thai, Self::Cjk];
    pub fn filename(self) -> &'static str { match self {
        Self::Hebrew => "NotoSansHebrew.ttf", Self::Armenian => "NotoSansArmenian.ttf", Self::Georgian => "NotoSansGeorgian.ttf",
        Self::Devanagari => "NotoSansDevanagari.ttf", Self::Kannada => "NotoSansKannada.ttf", Self::Tamil => "NotoSansTamil.ttf",
        Self::Thai => "NotoSansThai.ttf", Self::Cjk => "NotoSansCJKjp-Regular.otf",
    } }
    pub fn license_filename(self) -> &'static str { match self {
        Self::Hebrew => "NotoSansHebrew-OFL.txt", Self::Armenian => "NotoSansArmenian-OFL.txt", Self::Georgian => "NotoSansGeorgian-OFL.txt",
        Self::Devanagari => "NotoSansDevanagari-OFL.txt", Self::Kannada => "NotoSansKannada-OFL.txt", Self::Tamil => "NotoSansTamil-OFL.txt",
        Self::Thai => "NotoSansThai-OFL.txt", Self::Cjk => "NotoSansCJK-OFL.txt",
    } }
}
pub(super) fn family_for(c: char) -> Option<FallbackFont> {
    match u32::from(c) {
        0x0590..=0x05ff | 0xfb1d..=0xfb4f => Some(FallbackFont::Hebrew),
        0x0530..=0x058f | 0xfb13..=0xfb17 => Some(FallbackFont::Armenian),
        0x10a0..=0x10ff | 0x1c90..=0x1cbf | 0x2d00..=0x2d2f => Some(FallbackFont::Georgian),
        0x0900..=0x097f | 0xa8e0..=0xa8ff | 0x11b00..=0x11b5f => Some(FallbackFont::Devanagari),
        0x0c80..=0x0cff => Some(FallbackFont::Kannada),
        0x0b80..=0x0bff | 0x11fc0..=0x11fff => Some(FallbackFont::Tamil),
        0x0e00..=0x0e7f => Some(FallbackFont::Thai),
        0x1100..=0x11ff | 0x2e80..=0x33ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff |
        0xa960..=0xa97f | 0xac00..=0xd7ff | 0xf900..=0xfaff | 0xff01..=0xffef |
        0x1aff0..=0x1b16f | 0x20000..=0x323af => Some(FallbackFont::Cjk),
        _ => None,
    }
}
struct Required { fonts: [bool; 8], bytes: usize }
impl io::Write for Required {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > MAX_CAPTURE_BYTES { return Err(io::Error::other("document input limit")); }
        // serde_json writes string fragments at ASCII escape boundaries, so
        // each fragment preserves UTF-8 boundaries, including non-BMP text.
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        for c in text.chars() { if let Some(family) = family_for(c) { self.fonts[family as usize] = true; } }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
fn requirements(value: &impl Serialize) -> Result<Vec<FallbackFont>, Error> {
    let mut required = Required { fonts: [false; 8], bytes: 0 };
    serde_json::to_writer(&mut required, value).map_err(|e| Error::Encoding(e.to_string()))?;
    Ok(FallbackFont::ALL.into_iter().filter(|f| required.fonts[*f as usize]).collect())
}
pub fn analysis_fonts(document: &AnalysisDocument) -> Result<Vec<FallbackFont>, Error> { document.validate()?; requirements(document) }
pub fn invoice_fonts(document: &InvoiceDocument) -> Result<Vec<FallbackFont>, Error> { document.validate()?; requirements(document) }
