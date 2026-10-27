//! A4 analysis and invoice layout only. All PDF objects, embedded font subsets
//! and Unicode mappings are real Krilla output; no host/browser fallback.
use billing_core::presentation::invoice::{TaxKind, invoice_amount};
use krilla::{Document, geom::{PathBuilder, Point, Size, Transform}, metadata::Metadata, page::PageSettings, paint::Fill, color::rgb};
use transgate_core::documents::{AnalysisDocument, BlockRole, DocumentBlock, DocumentFormat, InvoiceDocument};
use super::{DocumentBytesError as Error, DocumentWriter};
use super::text::{ShapedLine, Style, Styled, TextLayout};

const MM: f32 = 72.0 / 25.4;
const W: f32 = 210.0 * MM;
const H: f32 = 297.0 * MM;
const MAX_PAGES: usize = 1024;
type Color = [u8; 3];
const BODY: Color = [50; 3];
const MUTED: Color = [120; 3];
const LIGHT: Color = [180; 3];
#[derive(Clone, Copy)]
enum Align { Left, Center, Right }
enum Command {
    Text { line: ShapedLine, x: f32, y: f32, size: f32, color: Color },
    Rect { x: f32, y: f32, width: f32, height: f32, color: Color },
    Bullet { x: f32, y: f32 },
    Logo { x: f32, y: f32, width: f32, height: f32 },
}
#[derive(Clone, Copy)]
struct Cursor { page: usize, y: f32 }
struct Plan { pages: Vec<Vec<Command>>, cursor: Cursor, top: f32, bottom: f32 }
impl Plan {
    fn new(top: f32, bottom: f32) -> Self { Self { pages: vec![vec![]], cursor: Cursor { page: 0, y: top }, top, bottom } }
    fn command(&mut self, command: Command) { self.pages[self.cursor.page].push(command); }
    fn ensure(&mut self, needed: f32) -> Result<(), Error> {
        if self.cursor.y + needed > self.bottom {
            self.cursor.page += 1; self.cursor.y = self.top;
            if self.cursor.page >= MAX_PAGES { return Err(Error::LimitExceeded); }
            if self.pages.len() <= self.cursor.page { self.pages.push(vec![]); }
        }
        Ok(())
    }
    fn spacing(&mut self, height: f32) { self.cursor.y += height; }
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) { self.command(Command::Rect { x, y, width, height, color }); }
    fn rule(&mut self, x: f32, width: f32, thickness: f32) -> Result<(), Error> {
        self.ensure(4.0 * MM)?; self.rect(x, self.cursor.y, width, thickness, [200; 3]); self.spacing(4.0 * MM); Ok(())
    }
    // Layout primitive: position, box and typography are independent inputs.
    #[allow(clippy::too_many_arguments)]
    fn lines(&mut self, lines: Vec<ShapedLine>, x: f32, width: f32, size: f32, leading: f32, color: Color, align: Align) -> Result<(), Error> {
        for line in lines {
            self.ensure(leading)?;
            let offset = match align { Align::Left => 0.0, Align::Center => (width - line.width) / 2.0, Align::Right => width - line.width };
            self.command(Command::Text { line, x: x + offset, y: self.cursor.y, size, color }); self.spacing(leading);
        }
        Ok(())
    }
    // Layout primitive: position, box and typography are independent inputs.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, layout: &TextLayout<'_>, text: impl Into<String>, x: f32, width: f32, size: f32, style: Style, color: Color, align: Align) -> Result<(), Error> {
        let lines = layout.wrap(Styled::plain(text, style), size, width, style)?;
        self.lines(lines, x, width, size, size * 1.6, color, align)
    }
}
fn bold() -> Style { Style { bold: true, italic: false } }
fn italic() -> Style { Style { bold: false, italic: true } }

pub(super) fn analysis(writer: &DocumentWriter, model: &AnalysisDocument) -> Result<Vec<u8>, Error> {
    let layout = TextLayout::new(writer)?;
    let left = 20.0 * MM; let width = W - 40.0 * MM;
    let mut plan = Plan::new(25.0 * MM, H - 25.0 * MM);
    plan.spacing(8.0 * MM);
    for block in model.blocks(DocumentFormat::Pdf) {
        match block.role {
            BlockRole::Title => {
                let lines = layout.wrap(Styled::block(&block), 20.0, width, bold())?;
                plan.lines(lines, left, width, 20.0, 26.0, [30; 3], Align::Center)?;
                plan.spacing(2.0 * MM);
                plan.rect((W - 40.0 * MM) / 2.0, plan.cursor.y, 40.0 * MM, 0.8 * MM, [60; 3]); plan.spacing(6.0 * MM);
            }
            BlockRole::Subtitle => { plan.text(&layout, block.plain_text(), left, width, 14.0, Style::default(), MUTED, Align::Center)?; plan.spacing(MM); }
            BlockRole::Metadata => { plan.text(&layout, block.plain_text(), left, width, 8.0, italic(), LIGHT, Align::Center)?; plan.spacing(8.0 * MM); }
            BlockRole::Divider => { plan.rule(left, width, 0.3 * MM)?; plan.spacing(4.0 * MM); }
            BlockRole::Heading(level) => {
                plan.ensure(if level <= 2 { 14.0 * MM } else { 10.0 * MM })?;
                if level <= 2 {
                    if block.plain_text() != "Summary" { plan.rule(left, width, 0.3 * MM)?; }
                    plan.spacing(4.0 * MM);
                    plan.rect(left, plan.cursor.y - 3.0 * MM, 1.5 * MM, 5.0 * MM, [60; 3]);
                    plan.text(&layout, block.plain_text(), left + 5.0 * MM, width - 5.0 * MM, 12.0, bold(), [30; 3], Align::Left)?;
                    plan.spacing(2.0 * MM);
                } else {
                    plan.spacing(2.0 * MM); plan.text(&layout, block.plain_text(), left + 2.0 * MM, width - 2.0 * MM, 10.5, bold(), [60; 3], Align::Left)?; plan.spacing(MM);
                }
            }
            _ => analysis_body(&mut plan, &layout, &block, left, width)?,
        }
    }
    // Add page furniture after pagination is known, including every overflow
    // page. Header text precedes body content; footers follow it in the PDF.
    let total = plan.pages.len();
    for (index, commands) in plan.pages.iter_mut().enumerate() {
        if index > 0 {
            let title = layout.line(model.metadata().title.clone(), 7.0, italic())?;
            let kind = layout.line(model.type_title(), 7.0, italic())?;
            let right = W - left - kind.width;
            let mut header = vec![
                Command::Text { line: title, x: left, y: 17.0 * MM, size: 7.0, color: LIGHT },
                Command::Text { line: kind, x: right, y: 17.0 * MM, size: 7.0, color: LIGHT },
                Command::Rect { x: left, y: 20.0 * MM, width, height: 0.2 * MM, color: [200; 3] },
            ];
            header.append(commands); *commands = header;
        }
        let number = layout.line(format!("{} / {total}", index + 1), 7.0, Style::default())?;
        let generated = layout.line(format!("Generated by Transgate AI  |  {}", model.export_date(false)), 7.0, Style::default())?;
        commands.push(Command::Text { x: (W - number.width) / 2.0, y: H - 10.0 * MM, line: number, size: 7.0, color: LIGHT });
        commands.push(Command::Text { x: (W - generated.width) / 2.0, y: H - 6.0 * MM, line: generated, size: 7.0, color: LIGHT });
    }
    encode(writer, plan, Metadata::new().title(model.metadata().title.clone()).description(format!("AI Analysis - {}", model.type_name())).creator("Transgate AI".into()))
}
fn analysis_body(plan: &mut Plan, layout: &TextLayout<'_>, block: &DocumentBlock, left: f32, width: f32) -> Result<(), Error> {
    let bullet = block.role == BlockRole::Bullet;
    let indent = if bullet { 9.0 * MM } else if matches!(block.role, BlockRole::Numbered(_)) { 3.0 * MM } else { 2.0 * MM };
    let last_meta = block.runs.last().filter(|r| r.italic && !r.text.trim().is_empty());
    if let Some(meta) = last_meta.filter(|_| block.runs.len() > 1) {
        let mut body = block.clone(); body.runs.pop();
        let lines = layout.wrap(Styled::block(&body), 9.5, width - indent, Style::default())?;
        plan.ensure(9.5 * 1.6)?;
        if bullet { plan.command(Command::Bullet { x: left + 5.0 * MM, y: plan.cursor.y - MM }); }
        plan.lines(lines, left + indent, width - indent, 9.5, 9.5 * 1.6, BODY, Align::Left)?;
        plan.text(layout, meta.text.trim(), left + indent, width - indent, 8.0, italic(), MUTED, Align::Left)?;
    } else {
        let small = block.runs.iter().all(|r| r.italic);
        let size = if small { 8.0 } else { 9.5 };
        let lines = layout.wrap(Styled::block(block), size, width - indent, Style::default())?;
        plan.ensure(size * 1.6)?;
        if bullet { plan.command(Command::Bullet { x: left + 5.0 * MM, y: plan.cursor.y - MM }); }
        plan.lines(lines, left + indent, width - indent, size, size * 1.6, if small { MUTED } else { BODY }, Align::Left)?;
    }
    if block.blank_after { plan.spacing(4.0 * MM); }
    Ok(())
}

pub(super) fn invoice(writer: &DocumentWriter, model: &InvoiceDocument) -> Result<Vec<u8>, Error> {
    let layout = TextLayout::new(writer)?; let facts = model.facts();
    let mut plan = Plan::new(40.0, H - 40.0); let left = 40.0; let width = W - 80.0;
    let (iw, ih) = writer.logo.size(); let scale = (160.0 / iw as f32).min(50.0 / ih as f32);
    plan.command(Command::Logo { x: left, y: 40.0 + (50.0 - ih as f32 * scale) / 2.0, width: iw as f32 * scale, height: ih as f32 * scale });
    plan.cursor.y = 60.0;
    plan.text(&layout, "INVOICE", W - 240.0, 200.0, 24.0, bold(), [2, 132, 199], Align::Right)?;
    plan.text(&layout, format!("#{}", facts.invoice_id), W - 260.0, 220.0, 14.0, Style::default(), [100, 116, 139], Align::Right)?;
    plan.cursor.y = 135.0;
    let start = plan.cursor; let column = width * 0.48;
    plan.text(&layout, "FROM", left, column, 10.0, Style::default(), [100, 116, 139], Align::Left)?;
    for line in ["Transgate.ai / Mustafa Abdelhamid", "D.dere Yalı Mah. Saniye Altuncu Cad.", "No: 14/B, 41950 Gölcük", "Kocaeli, Türkiye", "Tax ID: 6221748346", "admin@transgate.ai"] {
        plan.text(&layout, line, left, column, 10.0, Style::default(), BODY, Align::Left)?;
    }
    let seller_end = plan.cursor; plan.cursor = start;
    let right = left + width - column;
    plan.text(&layout, "BILL TO", right, column, 10.0, Style::default(), [100, 116, 139], Align::Left)?;
    for line in model.bill_to() { plan.text(&layout, line, right, column, 10.0, Style::default(), BODY, Align::Left)?; }
    plan.text(&layout, format!("Invoice Date: {}", facts.date), right, column, 10.0, Style::default(), BODY, Align::Left)?;
    if (seller_end.page, seller_end.y) > (plan.cursor.page, plan.cursor.y) { plan.cursor = seller_end; }
    plan.spacing(20.0); plan.ensure(44.0)?;
    plan.rect(left, plan.cursor.y - 11.0, width, 25.0, [248, 250, 252]);
    let table_start = plan.cursor;
    plan.text(&layout, "DESCRIPTION", left, width * 0.6, 10.0, bold(), BODY, Align::Left)?;
    plan.cursor = table_start; plan.text(&layout, "QTY", left + width * 0.6, width * 0.2, 10.0, bold(), BODY, Align::Center)?;
    plan.cursor = table_start; plan.text(&layout, "AMOUNT", left + width * 0.8, width * 0.2, 10.0, bold(), BODY, Align::Right)?;
    plan.rule(left, width, 0.7)?; plan.spacing(8.0);
    let row = plan.cursor;
    let mut description = format!("{} - Transcription Minutes Package", facts.package_name);
    if let Some(minutes) = facts.package_minutes { description.push_str(&format!("\n{minutes} minutes")); }
    plan.text(&layout, description, left, width * 0.6, 10.0, Style::default(), BODY, Align::Left)?;
    let description_end = plan.cursor;
    plan.cursor = row; plan.text(&layout, facts.quantity.to_string(), left + width * 0.6, width * 0.2, 10.0, Style::default(), BODY, Align::Center)?;
    let quantity_end = plan.cursor;
    plan.cursor = row; plan.text(&layout, format!("{} {}", facts.currency, invoice_amount(&facts.subtotal)), left + width * 0.8, width * 0.2, 10.0, Style::default(), BODY, Align::Right)?;
    for end in [description_end, quantity_end] { if (end.page, end.y) > (plan.cursor.page, plan.cursor.y) { plan.cursor = end; } }
    plan.spacing(12.0); plan.rule(left, width, 0.7)?; plan.spacing(16.0);
    for (label, value, total) in [
        ("Subtotal", &facts.subtotal, false),
        (if facts.tax_kind == TaxKind::Vat20 { "VAT (20%)" } else { "Tax (0%)" }, &facts.tax, false),
        ("Total ", &facts.total, true),
    ] {
        if total { plan.spacing(8.0); plan.rule(W - 248.0, 208.0, 0.7)?; }
        let at = plan.cursor; let size = if total { 14.0 } else { 10.0 }; let style = if total { bold() } else { Style::default() };
        plan.text(&layout, label, W - 248.0, 100.0, size, style, BODY, Align::Right)?;
        let label_end = plan.cursor; plan.cursor = at;
        plan.text(&layout, format!("{} {}", facts.currency, invoice_amount(value)), W - 140.0, 100.0, size, style, BODY, Align::Right)?;
        if (label_end.page, label_end.y) > (plan.cursor.page, plan.cursor.y) { plan.cursor = label_end; }
        plan.spacing(4.0);
    }
    plan.spacing(40.0); plan.rule(left, width, 0.7)?; plan.spacing(20.0);
    for line in [Some("Thank you for your Payment!".to_string()), Some(format!("Payment processed through {}", facts.gateway)), (facts.tax_kind == TaxKind::Vat20).then(|| "VAT Registration Number: 6221748346".to_string()), Some("For any questions, please contact support@transgate.ai".to_string())].into_iter().flatten() {
        plan.text(&layout, line, left, width, 10.0, Style::default(), [100, 116, 139], Align::Center)?;
    }
    encode(writer, plan, Metadata::new().title(format!("Invoice {}", facts.invoice_id)).creator("Transgate".into()))
}
fn fill(color: Color) -> Fill { Fill { paint: rgb::Color::new(color[0], color[1], color[2]).into(), ..Default::default() } }
fn encode(writer: &DocumentWriter, plan: Plan, metadata: Metadata) -> Result<Vec<u8>, Error> {
    let mut document = Document::new(); document.set_metadata(metadata);
    for commands in plan.pages {
        let mut page = document.start_page_with(PageSettings::from_wh(W, H).ok_or(Error::LimitExceeded)?);
        let mut surface = page.surface();
        for command in commands {
            match command {
                Command::Text { mut line, x, y, size, color } => {
                    surface.set_fill(Some(fill(color)));
                    // Positions were assigned in visual bidi order, but emit
                    // runs in logical order so independent text readers retain
                    // the original Unicode instead of a visual-order reversal.
                    line.runs.sort_by_key(|r| r.range.start);
                    for run in line.runs { surface.draw_glyphs(Point::from_xy(x + run.x, y), &run.glyphs, writer.font(run.font).font.clone(), &run.text[run.range], size, false); }
                }
                Command::Rect { x, y, width, height, color } => {
                    let mut path = PathBuilder::new(); path.move_to(x, y); path.line_to(x + width, y); path.line_to(x + width, y + height); path.line_to(x, y + height); path.close();
                    if let Some(path) = path.finish() { surface.set_fill(Some(fill(color))); surface.draw_path(&path); }
                }
                Command::Bullet { x, y } => {
                    let r = 0.8 * MM; let k = r * 0.552_284_8; let mut path = PathBuilder::new(); path.move_to(x + r, y);
                    path.cubic_to(x + r, y + k, x + k, y + r, x, y + r); path.cubic_to(x - k, y + r, x - r, y + k, x - r, y);
                    path.cubic_to(x - r, y - k, x - k, y - r, x, y - r); path.cubic_to(x + k, y - r, x + r, y - k, x + r, y); path.close();
                    if let Some(path) = path.finish() { surface.set_fill(Some(fill(MUTED))); surface.draw_path(&path); }
                }
                Command::Logo { x, y, width, height } => {
                    surface.push_transform(&Transform::from_translate(x, y));
                    surface.draw_image(writer.logo.clone(), Size::from_wh(width, height).ok_or(Error::LimitExceeded)?); surface.pop();
                }
            }
        }
        surface.finish(); page.finish();
    }
    document.finish().map_err(|error| Error::Encoding(format!("{error:?}")))
}
