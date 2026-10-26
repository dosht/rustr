use std::ops::Range;
use std::sync::Arc;
use krilla::{Data, text::{Font, GlyphId, KrillaGlyph, Tag}};
use rustybuzz::{Direction, Face, UnicodeBuffer};
use unicode_bidi::{BidiInfo, ParagraphInfo};
use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;
use transgate_core::documents::{BlockRole, DocumentBlock};
use super::{DocumentBytesError as Error, DocumentWriter, MAX_FONT_BYTES};
use super::fonts::family_for;

#[derive(Clone)]
pub(super) struct LoadedFont { pub data: Data, pub font: Font, weight: f32 }
impl LoadedFont {
    pub fn data(bytes: Vec<u8>, name: &'static str) -> Result<Data, Error> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES { return Err(Error::InvalidFont(name)); }
        if Face::from_slice(&bytes, 0).is_none() { return Err(Error::InvalidFont(name)); }
        Ok(bytes.into())
    }
    pub fn new(data: Data, weight: f32) -> Result<Self, Error> {
        let font = Font::new_variable(data.clone(), 0, &[(Tag::new(b"wght"), weight)])
            .ok_or(Error::InvalidFont("unreadable OpenType instance"))?;
        Ok(Self { data, font, weight })
    }
    fn face(&self) -> Result<Face<'_>, Error> {
        let mut face = Face::from_slice(self.data.as_ref(), 0).ok_or(Error::InvalidFont("unreadable shaping face"))?;
        face.set_variation(rustybuzz::ttf_parser::Tag::from_bytes(b"wght"), self.weight);
        Ok(face)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Style { pub bold: bool, pub italic: bool }
pub(super) struct Styled { text: Arc<str>, spans: Vec<(Range<usize>, Style)> }
impl Styled {
    pub fn plain(text: impl Into<String>, style: Style) -> Self {
        let text: Arc<str> = Arc::from(text.into()); let len = text.len();
        Self { text, spans: vec![(0..len, style)] }
    }
    pub fn block(block: &DocumentBlock) -> Self {
        let mut text = String::new(); let mut spans = vec![];
        if let BlockRole::Numbered(n) = block.role { text.push_str(&format!("{n}. ")); spans.push((0..text.len(), Style::default())); }
        for run in &block.runs { let start = text.len(); text.push_str(&run.text); spans.push((start..text.len(), Style { bold: run.bold, italic: run.italic })); }
        Self { text: Arc::from(text), spans }
    }
    fn style(&self, at: usize, base: Style) -> Style {
        let own = self.spans.iter().find(|(r, _)| r.contains(&at)).map_or(Style::default(), |(_, s)| *s);
        Style { bold: own.bold || base.bold, italic: own.italic || base.italic }
    }
}
pub(super) struct ShapedRun {
    pub text: Arc<str>, pub range: Range<usize>, pub glyphs: Vec<KrillaGlyph>, pub font: usize,
    pub x: f32,
}
pub(super) struct ShapedLine { pub runs: Vec<ShapedRun>, pub width: f32 }
struct FontRun { range: Range<usize>, font: usize }
pub(super) struct TextLayout<'a> { faces: [Option<Face<'a>>; 23] }
impl<'a> TextLayout<'a> {
    pub fn new(writer: &'a DocumentWriter) -> Result<Self, Error> {
        let mut faces = std::array::from_fn(|_| None);
        for (index, font) in writer.fonts.iter().enumerate() { faces[index] = Some(font.face()?); }
        for (index, pair) in writer.fallbacks.iter().enumerate() {
            if let Some(pair) = pair { faces[7 + 2 * index] = Some(pair.regular.face()?); faces[8 + 2 * index] = Some(pair.bold.face()?); }
        }
        Ok(Self { faces })
    }
    fn face(&self, index: usize) -> &Face<'a> { self.faces[index].as_ref().expect("font selection checks the loaded slot") }
    fn font_for(&self, grapheme: &str, style: Style) -> Result<usize, Error> {
        let latin = usize::from(style.bold) + if style.italic { 2 } else { 0 };
        let arabic = 4 + usize::from(style.bold);
        if let Some(family) = grapheme.chars().find_map(family_for) {
            let index = 7 + family as usize * 2 + usize::from(style.bold);
            let face = self.faces[index].as_ref().ok_or(Error::MissingFallback(family))?;
            if grapheme.chars().all(|c| ignorable(c) || face.glyph_index(c).is_some()) { return Ok(index); }
        }
        let contains_arabic = grapheme.chars().any(|c| matches!(u32::from(c), 0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff));
        let candidates = if contains_arabic { [arabic, latin, 6] } else { [latin, arabic, 6] };
        for index in candidates {
            if grapheme.chars().all(|c| ignorable(c) || self.face(index).glyph_index(c).is_some()) { return Ok(index); }
        }
        let missing = grapheme.chars().find(|c| !ignorable(*c) && self.faces.iter().flatten().all(|face| face.glyph_index(*c).is_none())).unwrap_or_else(|| grapheme.chars().next().unwrap_or('\u{fffd}'));
        Err(Error::MissingGlyph(missing))
    }
    fn font_runs(&self, source: &Styled, range: Range<usize>, base: Style) -> Result<Vec<FontRun>, Error> {
        let mut runs: Vec<FontRun> = vec![];
        for (offset, g) in source.text[range.clone()].grapheme_indices(true) {
            let start = range.start + offset; let end = start + g.len();
            let font = self.font_for(g, source.style(start, base))?;
            if let Some(last) = runs.last_mut().filter(|r| r.font == font) { last.range.end = end; }
            else { runs.push(FontRun { range: start..end, font }); }
        }
        Ok(runs)
    }
    fn glyphs(&self, text: &str, font: usize, rtl: bool) -> Vec<KrillaGlyph> {
        let face = self.face(font);
        let mut input = UnicodeBuffer::new(); input.push_str(text); input.guess_segment_properties();
        input.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        let output = rustybuzz::shape(face, &[], input);
        let infos = output.glyph_infos();
        let mut ends: Vec<_> = infos.iter().map(|i| i.cluster as usize).collect(); ends.push(text.len()); ends.sort_unstable(); ends.dedup();
        let units = face.units_per_em() as f32;
        infos.iter().zip(output.glyph_positions()).map(|(info, pos)| {
            let start = info.cluster as usize;
            let end = ends[ends.binary_search(&start).expect("shaper returned its own cluster") + 1];
            KrillaGlyph::new(GlyphId::new(info.glyph_id), pos.x_advance as f32 / units, pos.x_offset as f32 / units,
                pos.y_offset as f32 / units, pos.y_advance as f32 / units, start..end, None)
        }).collect()
    }
    fn shape(&self, source: &Styled, bidi: &BidiInfo<'_>, range: Range<usize>, size: f32, base: Style) -> Result<ShapedLine, Error> {
        if range.is_empty() { return Ok(ShapedLine { runs: vec![], width: 0.0 }); }
        let paragraph = bidi.paragraphs.iter().find(|p| p.range.contains(&range.start)).expect("range belongs to the bidi paragraph");
        // unicode-bidi's L1 helper clones the entire levels vector. Preserve
        // paragraph-resolved levels, but give L1/L2 only this line/measurement,
        // not a fresh copy of a potentially megabyte-long paragraph per word.
        let offset = range.start;
        let sliced = BidiInfo {
            text: &source.text[range.clone()],
            original_classes: bidi.original_classes[range.clone()].to_vec(),
            levels: bidi.levels[range].to_vec(),
            paragraphs: Vec::new(),
        };
        let local = ParagraphInfo { range: 0..sliced.text.len(), level: paragraph.level };
        let (levels, visual) = sliced.visual_runs(&local, local.range.clone());
        let mut runs = vec![]; let mut x = 0.0;
        for level_run in visual {
            let rtl = levels[level_run.start].is_rtl();
            let mut fonts = self.font_runs(source, level_run.start + offset..level_run.end + offset, base)?;
            if rtl { fonts.reverse(); }
            for font_run in fonts {
                let glyphs = self.glyphs(&source.text[font_run.range.clone()], font_run.font, rtl);
                let width = glyphs.iter().map(|g| g.x_advance * size).sum::<f32>();
                runs.push(ShapedRun { text: source.text.clone(), range: font_run.range, glyphs, font: font_run.font, x });
                x += width;
            }
        }
        Ok(ShapedLine { runs, width: x })
    }
    pub fn line(&self, text: impl Into<String>, size: f32, style: Style) -> Result<ShapedLine, Error> {
        let source = Styled::plain(text, style); let bidi = BidiInfo::new(&source.text, None);
        self.shape(&source, &bidi, 0..source.text.len(), size, style)
    }
    pub fn wrap(&self, source: Styled, size: f32, width: f32, base: Style) -> Result<Vec<ShapedLine>, Error> {
        let bidi = BidiInfo::new(&source.text, None); let mut lines = vec![];
        if source.text.is_empty() { return Ok(lines); }
        for paragraph in &bidi.paragraphs {
            let full = paragraph.range.clone();
            let paragraph_end = full.start + source.text[full.clone()].trim_end_matches(['\n', '\r']).len();
            if paragraph_end == full.start { lines.push(ShapedLine { runs: vec![], width: 0.0 }); continue; }
            let mut start = full.start; let mut segment_start = start; let mut advance = 0.0;
            for (local_end, _) in linebreaks(&source.text[full.start..paragraph_end]) {
                let end = full.start + local_end;
                let segment = self.shape(&source, &bidi, segment_start..end, size, base)?;
                if advance + segment.width > width && segment_start > start {
                    lines.push(self.shape(&source, &bidi, trimmed(&source.text, start..segment_start), size, base)?);
                    start = segment_start; advance = 0.0;
                }
                if segment.width > width {
                    let boundaries: Vec<_> = source.text[start..end].grapheme_indices(true).map(|(i, g)| start + i + g.len()).collect();
                    let mut from = 0;
                    while start < end {
                        // Exponentially bracket the next line, then binary
                        // search only that bracket. A long unbroken word is
                        // never reshaped at half its total size on every line.
                        let mut lo = from; let mut hi = (from + 1).min(boundaries.len());
                        loop {
                            let measured = self.shape(&source, &bidi, start..boundaries[hi - 1], size, base)?.width;
                            if measured > width { break; }
                            lo = hi;
                            if hi == boundaries.len() { break; }
                            hi = (from + 2 * (hi - from)).min(boundaries.len());
                        }
                        while lo < hi {
                            let mid = lo + (hi - lo).div_ceil(2);
                            let measured = self.shape(&source, &bidi, start..boundaries[mid - 1], size, base)?.width;
                            if measured <= width { lo = mid; } else { hi = mid - 1; }
                        }
                        if lo == from { return Err(Error::LimitExceeded); }
                        let cut = boundaries[lo - 1]; from = lo;
                        if cut == end { advance = self.shape(&source, &bidi, start..end, size, base)?.width; break; }
                        lines.push(self.shape(&source, &bidi, trimmed(&source.text, start..cut), size, base)?); start = cut;
                    }
                } else { advance += segment.width; }
                segment_start = end;
            }
            if start < paragraph_end { lines.push(self.shape(&source, &bidi, trimmed(&source.text, start..paragraph_end), size, base)?); }
        }
        Ok(lines)
    }
}
fn trimmed(text: &str, range: Range<usize>) -> Range<usize> { range.start..range.start + text[range.clone()].trim_end().len() }
fn ignorable(c: char) -> bool { matches!(u32::from(c), 0x0000..=0x001f | 0x007f..=0x009f | 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xfeff | 0xe0100..=0xe01ef) }
