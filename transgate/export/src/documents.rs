//! In-memory P5 document encoders over the captured pure model (FINAL-A).
//! Assets enter once through owned bytes. No I/O, host clock, system font,
//! document fetch, browser encoder or implicit fallback exists here.
mod docx;
mod pdf;
mod text;
mod fonts;
pub use fonts::{FallbackFont, analysis_fonts, invoice_fonts};

use std::fmt;
use krilla::image::Image;
use transgate_core::documents::{AnalysisDocument, DocumentError, DocumentFormat, InvoiceDocument};
use text::LoadedFont;

pub const MAX_FONT_BYTES: usize = 24 * 1024 * 1024;
pub const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;
/// `symbols` is Noto Sans Symbols 2 (ratings, checks); `arrows` is Noto Sans
/// Symbols, the smallest bundled face that covers the generated U+2192 arrow.
pub struct FontBytes { pub latin: Vec<u8>, pub latin_italic: Vec<u8>, pub arabic: Vec<u8>, pub symbols: Vec<u8>, pub arrows: Vec<u8> }
/// Base faces precede the lazily loaded fallback pairs in every font index.
pub(super) const BASE_FONTS: usize = 8;
pub struct DocumentAssets { pub fonts: FontBytes, pub invoice_logo: Vec<u8> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentBytes { pub filename: String, pub content_type: String, pub bytes: Vec<u8> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentBytesError {
    Model(DocumentError), InvalidFont(&'static str), InvalidLogo(String), MissingGlyph(char),
    MissingPdfAssets, MissingFallback(FallbackFont), LimitExceeded, Encoding(String),
}
impl fmt::Display for DocumentBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { match self {
        Self::Model(error) => error.fmt(f), Self::InvalidFont(name) => write!(f, "Invalid document font: {name}"),
        Self::InvalidLogo(error) => write!(f, "Invalid invoice logo: {error}"),
        Self::MissingGlyph(c) => write!(f, "Bundled document fonts do not support U+{:04X}", u32::from(*c)),
        Self::MissingPdfAssets => f.write_str("PDF font assets have not been loaded"),
        Self::MissingFallback(family) => write!(f, "PDF font asset has not been loaded: {}", family.filename()),
        Self::LimitExceeded => f.write_str("Document layout exceeds its in-memory limit"),
        Self::Encoding(error) => write!(f, "Document encoding failed: {error}"),
    } }
}
impl std::error::Error for DocumentBytesError {}
impl From<DocumentError> for DocumentBytesError { fn from(value: DocumentError) -> Self { Self::Model(value) } }

struct FontPair { regular: LoadedFont, bold: LoadedFont }
pub struct DocumentWriter { fonts: [LoadedFont; BASE_FONTS], fallbacks: [Option<FontPair>; 8], logo: Image }
impl DocumentWriter {
    pub fn new(assets: DocumentAssets) -> Result<Self, DocumentBytesError> {
        let FontBytes { latin, latin_italic, arabic, symbols, arrows } = assets.fonts;
        let regular = LoadedFont::data(latin, "Noto Sans")?;
        let italic = LoadedFont::data(latin_italic, "Noto Sans Italic")?;
        let arabic = LoadedFont::data(arabic, "Noto Sans Arabic")?;
        let symbols = LoadedFont::data(symbols, "Noto Sans Symbols 2")?;
        let arrows = LoadedFont::data(arrows, "Noto Sans Symbols")?;
        let fonts = [
            LoadedFont::new(regular.clone(), 400.0)?, LoadedFont::new(regular, 700.0)?,
            LoadedFont::new(italic.clone(), 400.0)?, LoadedFont::new(italic, 700.0)?,
            LoadedFont::new(arabic.clone(), 400.0)?, LoadedFont::new(arabic, 700.0)?,
            LoadedFont::new(symbols, 400.0)?, LoadedFont::new(arrows, 400.0)?,
        ];
        if assets.invoice_logo.len() > MAX_LOGO_BYTES { return Err(DocumentBytesError::LimitExceeded); }
        let logo = Image::from_jpeg(assets.invoice_logo.into(), true).map_err(DocumentBytesError::InvalidLogo)?;
        Ok(Self { fonts, fallbacks: std::array::from_fn(|_| None), logo })
    }
    /// Parse each named family once, after the shell lazily loads its bundled
    /// same-origin asset. Repeated calls keep the existing parsed instance.
    pub fn add_fallback(&mut self, family: FallbackFont, bytes: Vec<u8>) -> Result<(), DocumentBytesError> {
        if self.has_fallback(family) { return Ok(()); }
        let data = LoadedFont::data(bytes, family.filename())?;
        let regular = LoadedFont::new(data.clone(), 400.0)?;
        // The bounded pan-CJK asset is a real regular face, not synthesized bold.
        let bold = if family == FallbackFont::Cjk { regular.clone() } else { LoadedFont::new(data, 700.0)? };
        self.fallbacks[family as usize] = Some(FontPair { regular, bold });
        Ok(())
    }
    pub fn has_fallback(&self, family: FallbackFont) -> bool { self.fallbacks[family as usize].is_some() }
    fn font(&self, index: usize) -> &LoadedFont {
        if index < BASE_FONTS { &self.fonts[index] }
        else {
            let pair = self.fallbacks[(index - BASE_FONTS) / 2].as_ref().expect("a shaped run refers only to a loaded font");
            if (index - BASE_FONTS).is_multiple_of(2) { &pair.regular } else { &pair.bold }
        }
    }
    pub fn invoice(&self, document: &InvoiceDocument) -> Result<DocumentBytes, DocumentBytesError> {
        document.validate()?;
        Ok(DocumentBytes { filename: document.filename(), content_type: "application/pdf".into(), bytes: pdf::invoice(self, document)? })
    }
}

/// Font-free formats never construct a PDF writer or check glyph coverage.
/// Only PDF consumes the reusable shell-supplied font/assets context.
pub fn analysis_bytes(document: &AnalysisDocument, format: DocumentFormat, pdf_writer: Option<&DocumentWriter>) -> Result<DocumentBytes, DocumentBytesError> {
    document.validate()?;
    let bytes = match format {
        DocumentFormat::Txt | DocumentFormat::Md => document.text().into_bytes(),
        DocumentFormat::Docx => docx::encode(document)?,
        DocumentFormat::Pdf => pdf::analysis(pdf_writer.ok_or(DocumentBytesError::MissingPdfAssets)?, document)?,
    };
    Ok(DocumentBytes { filename: document.filename(format), content_type: format.content_type().into(), bytes })
}
