//! Consumer side of the P5-DOCS-FINAL-A document adapter.
//!
//! The pure writer is auth-free, and the document is captured once at the click
//! (`AnalysisDocument::capture`). This module adds what only a consumer can:
//! - one reusable PDF writer whose licensed assets are fetched lazily, only for
//!   PDF and only the fallback families the captured document needs (D18);
//! - the delivery fence: after every await, and immediately before bytes reach
//!   save/share/clipboard, the caller's current owner/session/consent epoch and
//!   disposal are checked. A stale capture is dropped, never re-rendered from
//!   newer data. Fonts stay cached: they are public assets, not private output.
use std::sync::Arc;

use futures_util::lock::Mutex;
use transgate_core::documents::{AnalysisDocument, DocumentFormat, InvoiceDocument};
use transgate_core::presentation::ai::analysis::ExportFailure;
use transgate_export::documents::{
    DocumentAssets, DocumentBytes, DocumentBytesError, DocumentWriter, FallbackFont, analysis_bytes, analysis_fonts, invoice_fonts,
};

use crate::future::{BoxFuture, MaybeSend, MaybeSync};

/// The bundled same-origin font/logo assets (browser `fetch`, native bundle).
pub trait DocumentAssetSource: MaybeSend + MaybeSync {
    /// The four base fonts and the invoice logo, needed by any PDF.
    fn base(&self) -> BoxFuture<'_, Result<DocumentAssets, String>>;
    /// One extra script family (`FallbackFont::filename`).
    fn fallback(&self, family: FallbackFont) -> BoxFuture<'_, Result<Vec<u8>, String>>;
}

/// Why bytes could not be produced, before any delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// An asset could not be loaded or parsed.
    Fonts,
    /// The adapter refused the captured model (limits, a glyph outside the pack).
    Document,
}

impl From<RenderError> for ExportFailure {
    fn from(e: RenderError) -> Self {
        match e {
            RenderError::Fonts => ExportFailure::Fonts,
            RenderError::Document => ExportFailure::Document,
        }
    }
}

fn adapter(e: DocumentBytesError) -> RenderError {
    match e {
        DocumentBytesError::InvalidFont(_) | DocumentBytesError::InvalidLogo(_) | DocumentBytesError::MissingPdfAssets => RenderError::Fonts,
        _ => RenderError::Document,
    }
}

/// One parsed PDF writer per tab, created on the first PDF and reused.
pub struct DocumentFonts {
    source: Arc<dyn DocumentAssetSource>,
    writer: Mutex<Option<DocumentWriter>>,
}

impl DocumentFonts {
    pub fn new(source: Arc<dyn DocumentAssetSource>) -> Self {
        Self { source, writer: Mutex::new(None) }
    }

    /// Ensures the base writer and the given fallback families, then runs `encode`.
    /// Concurrent exports wait for one load instead of fetching twice; a failed
    /// load leaves nothing cached, so a later export tries again.
    async fn with_writer<T>(
        &self,
        families: Vec<FallbackFont>,
        encode: impl FnOnce(&DocumentWriter) -> Result<T, DocumentBytesError>,
    ) -> Result<T, RenderError> {
        let mut slot = self.writer.lock().await;
        if slot.is_none() {
            let assets = self.source.base().await.map_err(|_| RenderError::Fonts)?;
            *slot = Some(DocumentWriter::new(assets).map_err(adapter)?);
        }
        let writer = slot.as_mut().expect("set above");
        for family in families {
            if !writer.has_fallback(family) {
                let bytes = self.source.fallback(family).await.map_err(|_| RenderError::Fonts)?;
                writer.add_fallback(family, bytes).map_err(adapter)?;
            }
        }
        encode(writer).map_err(adapter)
    }

    /// DOCX/TXT/MD need no asset; PDF loads exactly what the capture requires.
    pub async fn analysis(&self, document: &AnalysisDocument, format: DocumentFormat) -> Result<DocumentBytes, RenderError> {
        if format != DocumentFormat::Pdf {
            return analysis_bytes(document, format, None).map_err(adapter);
        }
        let families = analysis_fonts(document).map_err(adapter)?;
        self.with_writer(families, |writer| analysis_bytes(document, DocumentFormat::Pdf, Some(writer))).await
    }

    pub async fn invoice(&self, document: &InvoiceDocument) -> Result<DocumentBytes, RenderError> {
        let families = invoice_fonts(document).map_err(adapter)?;
        self.with_writer(families, |writer| writer.invoice(document)).await
    }
}

/// What happened to one export effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The bytes reached `save`; `Err` when the save itself failed.
    Delivered(Result<(), ExportFailure>),
    /// Rendering failed while the capture was still current.
    Failed(ExportFailure),
    /// The owner, session, consent epoch or presenter changed while bytes were
    /// produced: nothing was saved, shared or copied, and no result applies.
    Stale,
}

/// Renders the captured analysis and hands it to `save` only while `current()`
/// still holds. `current` must compare the epochs captured with the effect, not
/// only the present owner id: a same-owner relogin or a consent refusal then
/// regrant makes an older capture stale.
pub async fn export_analysis(
    fonts: &DocumentFonts,
    document: &AnalysisDocument,
    format: DocumentFormat,
    current: impl Fn() -> bool,
    save: impl FnOnce(DocumentBytes) -> bool,
) -> Delivery {
    if !current() {
        return Delivery::Stale;
    }
    let rendered = fonts.analysis(document, format).await;
    if !current() {
        return Delivery::Stale;
    }
    match rendered {
        Ok(bytes) => Delivery::Delivered(if save(bytes) { Ok(()) } else { Err(ExportFailure::Save) }),
        Err(e) => Delivery::Failed(e.into()),
    }
}

/// Renders one captured invoice and delivers it only while the session/disposal
/// fence still holds. Preview callers additionally fence their core-issued
/// request ticket so closed, replaced or old-context previews cannot reappear.
pub async fn export_invoice(
    fonts: &DocumentFonts,
    document: &InvoiceDocument,
    current: impl Fn() -> bool,
    deliver: impl FnOnce(DocumentBytes) -> bool,
) -> Delivery {
    if !current() {
        return Delivery::Stale;
    }
    let rendered = fonts.invoice(document).await;
    if !current() {
        return Delivery::Stale;
    }
    match rendered {
        Ok(bytes) => Delivery::Delivered(if deliver(bytes) { Ok(()) } else { Err(ExportFailure::Save) }),
        Err(e) => Delivery::Failed(e.into()),
    }
}
