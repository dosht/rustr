//! P5-DOCS-FINAL-A. Pure, bounded documents captured from the displayed value
//! at the export click, never loaded from a job or a latest generation.
//! Analysis layout follows the source's format-specific ordering. Invoice
//! financial facts are already computed by billing; this module never taxes,
//! rounds, authorizes, persists, loads fonts or saves bytes.
use std::fmt::{self, Write as _};
use std::io;
use billing_core::presentation::invoice::{BillTo, InvoiceFacts};
use serde::{Deserialize, Serialize};
use crate::ai::analysis::*;
use crate::ai::ports::GenerationStatus;
use crate::{JobId, Micros};

pub const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentFormat { Docx, Pdf, Txt, Md }
impl DocumentFormat {
    pub fn extension(self) -> &'static str { match self { Self::Docx => "docx", Self::Pdf => "pdf", Self::Txt => "txt", Self::Md => "md" } }
    pub fn content_type(self) -> &'static str { match self {
        Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Self::Pdf => "application/pdf", Self::Txt => "text/plain", Self::Md => "text/markdown",
    } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentError { NoContent, InvalidMetadata, LimitExceeded }
impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(match self {
        Self::NoContent => "There is no displayed analysis to export.",
        Self::InvalidMetadata => "The captured document metadata is invalid.",
        Self::LimitExceeded => "The captured document exceeds the in-memory limit.",
    }) }
}
impl std::error::Error for DocumentError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisMetadata {
    pub title: String,
    pub job_id: Option<JobId>,
    pub exported_at: Micros,
    /// Offset east of UTC at the click instant, not at the end of font loading.
    pub utc_offset_minutes: i16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisDocument {
    displayed: AnalysisProgress,
    status: GenerationStatus,
    metadata: AnalysisMetadata,
}

// Count serialized input without allocating a second copy of the displayed
// text. This writer is memory-only; it has no connection to an I/O port.
struct BoundedCount(usize);
impl io::Write for BoundedCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_CAPTURE_BYTES { return Err(io::Error::other("document input limit")); }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
fn bounded(value: &impl Serialize) -> Result<(), DocumentError> {
    serde_json::to_writer(BoundedCount(0), value).map_err(|_| DocumentError::LimitExceeded)
}
impl AnalysisDocument {
    pub fn capture(displayed: &AnalysisProgress, status: GenerationStatus, metadata: AnalysisMetadata) -> Result<Self, DocumentError> {
        validate_metadata(&metadata)?;
        if displayed.summary.as_deref().is_none_or(str::is_empty) && displayed.analysis.is_none() {
            return Err(DocumentError::NoContent);
        }
        bounded(&(displayed, &metadata))?;
        Ok(Self { displayed: displayed.clone(), status, metadata })
    }
    /// Recheck a value that crossed a serialization boundary before encoding.
    pub fn validate(&self) -> Result<(), DocumentError> {
        validate_metadata(&self.metadata)?;
        bounded(self)
    }
    pub fn status(&self) -> GenerationStatus { self.status }
    pub fn metadata(&self) -> &AnalysisMetadata { &self.metadata }
    pub fn analysis_type(&self) -> AnalysisType { self.displayed.detected.map_or(AnalysisType::Generic, |d| d.kind) }
    pub fn type_name(&self) -> &'static str { match self.analysis_type() {
        AnalysisType::Generic => "generic", AnalysisType::Lecture => "lecture", AnalysisType::Meeting => "meeting",
        AnalysisType::Sales => "sales", AnalysisType::Support => "support", AnalysisType::Interview => "interview",
    } }
    pub fn type_title(&self) -> &'static str { match self.analysis_type() {
        AnalysisType::Generic => "General Analysis", AnalysisType::Lecture => "Lecture Analysis", AnalysisType::Meeting => "Meeting Analysis",
        AnalysisType::Sales => "Sales Call Analysis", AnalysisType::Support => "Support Call Analysis", AnalysisType::Interview => "Interview Analysis",
    } }
    pub fn filename(&self, format: DocumentFormat) -> String {
        format!("{}-analysis.{}", filename_stem(&self.metadata.title), format.extension())
    }
    pub fn export_date(&self, weekday: bool) -> String {
        let days = (self.metadata.exported_at.0.div_euclid(1_000_000) + i64::from(self.metadata.utc_offset_minutes) * 60).div_euclid(86_400);
        let (year, month, day) = civil_date(days);
        const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
        const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        let date = format!("{} {day}, {year}", MONTHS[(month - 1) as usize]);
        if weekday { format!("{}, {date}", DAYS[(days + 4).rem_euclid(7) as usize]) } else { date }
    }
    /// TXT and MD intentionally share exactly the same source Markdown text.
    pub fn text(&self) -> String { markdown(&self.layout(Flavor::Plain)) }
    /// Escaped semantic rich clipboard content, from the same captured value.
    pub fn html(&self) -> String { clipboard_html(&self.layout(Flavor::Html)) }
    /// A small, nonrecursive layout vocabulary for these two document families,
    /// not a document editor, markup interpreter or arbitrary template engine.
    pub fn blocks(&self, format: DocumentFormat) -> Vec<DocumentBlock> {
        self.layout(match format { DocumentFormat::Docx => Flavor::Docx, DocumentFormat::Pdf => Flavor::Pdf, _ => Flavor::Plain })
    }
    fn layout(&self, flavor: Flavor) -> Vec<DocumentBlock> {
        let mut b = Blocks::new(flavor);
        if flavor.rich_file() {
            b.push(BlockRole::Title, &self.metadata.title);
            if flavor == Flavor::Docx {
                b.push(BlockRole::Metadata, format!("Analysis Type: {} | Date: {}", self.type_name(), self.export_date(false)));
            } else {
                b.push(BlockRole::Subtitle, self.type_title());
                b.push(BlockRole::Metadata, self.export_date(true));
            }
            b.push(BlockRole::Divider, "");
        } else { b.push(BlockRole::Title, "AI Analysis"); }
        if let Some(summary) = self.displayed.summary.as_deref().filter(|s| !s.is_empty()) {
            b.heading("Summary"); b.push(BlockRole::Paragraph, summary); b.end();
        }
        if let Some(Analysis::Generic(g)) = &self.displayed.analysis
            && let Some(highlights) = g.highlights.as_ref().filter(|h| !h.is_empty())
        {
            b.heading("Key Highlights");
            let category = if flavor == Flavor::Pdf { "Highlights" } else { "Highlight" };
            if flavor == Flavor::Docx { b.push(BlockRole::Heading(2), category); } else { b.subheading(category); }
            b.list(highlights);
        }
        if let Some(analysis) = &self.displayed.analysis { b.analysis(analysis); }
        if flavor == Flavor::Docx {
            b.push(BlockRole::Divider, "");
            b.push(BlockRole::Footer, format!("Generated by Transgate AI on {}", self.export_date(false)));
        }
        b.out
    }
}
fn validate_metadata(meta: &AnalysisMetadata) -> Result<(), DocumentError> {
    if !(-1439..=1439).contains(&meta.utc_offset_minutes) { return Err(DocumentError::InvalidMetadata); }
    let days = (meta.exported_at.0.div_euclid(1_000_000) + i64::from(meta.utc_offset_minutes) * 60).div_euclid(86_400);
    if !(1..=9999).contains(&civil_date(days).0) { return Err(DocumentError::InvalidMetadata); }
    Ok(())
}
// Proleptic Gregorian date, epoch 1970-01-01, with Euclidean eras for pre-epoch dates.
fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}
fn filename_stem(title: &str) -> String {
    let mut stem = String::new(); let mut whitespace = false;
    for c in title.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')) {
        if c.is_whitespace() || c == '\u{feff}' { if !whitespace { stem.push('-'); } whitespace = true; }
        else { stem.push(c); whitespace = false; }
    }
    // Source .slice(0,50) counts UTF-16 units, including a cut surrogate.
    let mut lower = stem.to_lowercase();
    let mut units = 0; let mut end = 0; let mut cut_surrogate = false;
    for (offset, c) in lower.char_indices() {
        if units + c.len_utf16() > 50 { end = offset; cut_surrogate = units == 49; break; }
        units += c.len_utf16(); end = offset + c.len_utf8();
    }
    lower.truncate(end);
    if cut_surrogate { lower.push('\u{fffd}'); }
    lower
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvoiceDocument { facts: InvoiceFacts, bill_to: Vec<String> }
impl InvoiceDocument {
    pub fn capture(facts: &InvoiceFacts, bill_to: &BillTo, temporary_bill_to: &str) -> Result<Self, DocumentError> {
        bounded(&(facts, bill_to, temporary_bill_to))?;
        let custom: Vec<_> = temporary_bill_to.split('\n').filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
        let lines = if custom.is_empty() {
            let mut lines = vec![bill_to.name.clone().unwrap_or_else(|| "Valued Customer".into())];
            if let Some(company) = &bill_to.company { lines.push(company.clone()); }
            lines.extend(bill_to.address_lines.iter().cloned());
            if let Some(country) = &bill_to.country { lines.push(country.clone()); }
            if let Some(tax_id) = &bill_to.tax_id { lines.push(format!("Tax ID: {tax_id}")); }
            lines
        } else { custom };
        Ok(Self { facts: facts.clone(), bill_to: lines })
    }
    pub fn facts(&self) -> &InvoiceFacts { &self.facts }
    pub fn bill_to(&self) -> &[String] { &self.bill_to }
    pub fn filename(&self) -> String { format!("invoice-{}.pdf", filename_stem(&self.facts.invoice_id).to_uppercase()) }
    pub fn validate(&self) -> Result<(), DocumentError> { bounded(self) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockRole { Title, Subtitle, Metadata, Heading(u8), Paragraph, Bullet, Numbered(u32), Divider, Footer }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentTone { Standard, Muted, Positive, Caution, Negative }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRun { pub text: String, pub bold: bool, pub italic: bool, pub tone: DocumentTone }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentBlock { pub role: BlockRole, pub runs: Vec<DocumentRun>, pub blank_after: bool }
impl DocumentBlock { pub fn plain_text(&self) -> String { self.runs.iter().map(|r| r.text.as_str()).collect() } }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor { Plain, Html, Docx, Pdf }
impl Flavor { fn rich_file(self) -> bool { matches!(self, Self::Docx | Self::Pdf) } }
struct Blocks { flavor: Flavor, out: Vec<DocumentBlock> }
fn run(text: impl Into<String>) -> DocumentRun { DocumentRun { text: text.into(), bold: false, italic: false, tone: DocumentTone::Standard } }
fn strong(text: impl Into<String>) -> DocumentRun { DocumentRun { text: text.into(), bold: true, italic: false, tone: DocumentTone::Standard } }
fn meta(text: impl Into<String>) -> DocumentRun { DocumentRun { text: text.into(), bold: false, italic: true, tone: DocumentTone::Muted } }
fn toned(mut run: DocumentRun, tone: DocumentTone) -> DocumentRun { run.tone = tone; run }
fn ts(seconds: f64) -> String { format!("{}:{:02}", (seconds / 60.0).floor(), (seconds % 60.0).floor() as i64) }
fn some(text: &Option<String>) -> Option<&str> { text.as_deref().filter(|t| !t.is_empty()) }
fn nonempty(items: &Option<Vec<String>>) -> Option<&[String]> { items.as_deref().filter(|v| !v.is_empty()) }
trait SourceName { fn source_name(&self) -> &'static str; }
impl SourceName for Priority { fn source_name(&self) -> &'static str { match self { Self::High => "high", Self::Medium => "medium", Self::Low => "low" } } }
impl SourceName for SkillLevel { fn source_name(&self) -> &'static str { match self { Self::Beginner => "Beginner", Self::Intermediate => "Intermediate", Self::Advanced => "Advanced", Self::Expert => "Expert" } } }
impl SourceName for ResponseAssessment { fn source_name(&self) -> &'static str { match self { Self::Positive => "positive", Self::Neutral => "neutral", Self::Negative => "negative" } } }
impl SourceName for ResolutionStatus { fn source_name(&self) -> &'static str { match self { Self::Resolved => "resolved", Self::Unresolved => "unresolved", Self::Escalated => "escalated" } } }
impl SourceName for SentimentKind { fn source_name(&self) -> &'static str { match self { Self::Frustrated => "frustrated", Self::Neutral => "neutral", Self::Satisfied => "satisfied", Self::Relieved => "relieved" } } }
fn enum_text(value: &impl SourceName) -> &'static str { value.source_name() }
impl Blocks {
    fn new(flavor: Flavor) -> Self { Self { flavor, out: vec![] } }
    fn push(&mut self, role: BlockRole, text: impl Into<String>) { self.runs(role, vec![run(text)]); }
    fn runs(&mut self, role: BlockRole, runs: Vec<DocumentRun>) { self.out.push(DocumentBlock { role, runs, blank_after: matches!(role, BlockRole::Title | BlockRole::Heading(_)) }); }
    fn heading(&mut self, title: &str) { self.push(BlockRole::Heading(if self.flavor == Flavor::Docx && matches!(title, "Summary" | "Key Highlights") { 1 } else { 2 }), title); }
    fn subheading(&mut self, title: &str) {
        if self.flavor == Flavor::Docx { self.runs(BlockRole::Paragraph, vec![strong(title)]); }
        else { self.push(BlockRole::Heading(3), title); }
    }
    fn advice_heading(&mut self, title: &str, positive: bool) {
        self.subheading(title);
        if self.flavor == Flavor::Docx
            && let Some(block) = self.out.last_mut()
        {
            for run in &mut block.runs { run.tone = if positive { DocumentTone::Positive } else { DocumentTone::Caution }; }
        }
    }
    fn advice_item(&mut self, text: &str, positive: bool) {
        if self.flavor == Flavor::Docx {
            self.runs(BlockRole::Bullet, vec![toned(run(if positive { "✓ " } else { "⚠ " }), if positive { DocumentTone::Positive } else { DocumentTone::Caution }), run(text)]);
        } else { self.push(BlockRole::Bullet, text); }
    }
    fn end(&mut self) { if let Some(last) = self.out.last_mut() { last.blank_after = true; } }
    fn list(&mut self, items: &[String]) { for item in items { self.push(BlockRole::Bullet, item); } self.end(); }
    fn strings(&mut self, title: &str, items: &Option<Vec<String>>, numbered: bool) {
        if let Some(items) = nonempty(items) { self.heading(title); for (i, item) in items.iter().enumerate() {
            self.push(if numbered { BlockRole::Numbered(i as u32 + 1) } else { BlockRole::Bullet }, item);
        } self.end(); }
    }
    fn field(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        let runs = if self.flavor == Flavor::Pdf { vec![run(format!("{label}: ")), run(value)] }
            else if self.flavor == Flavor::Docx { vec![strong(format!("{label}: ")), run(value)] }
            else { vec![strong(label), run(": "), run(value)] };
        self.runs(if self.flavor.rich_file() { BlockRole::Paragraph } else { BlockRole::Bullet }, runs);
    }
    fn analysis(&mut self, analysis: &Analysis) {
        match analysis {
            Analysis::Generic(g) => {
                if self.flavor == Flavor::Docx
                    && let Some(items) = nonempty(&g.highlights)
                {
                    self.push(BlockRole::Heading(2), "Key Highlights"); self.list(items);
                }
            }
            Analysis::Meeting(m) => {
                if let Some(items) = m.action_items.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Action Items");
                    for item in items {
                        if self.flavor == Flavor::Pdf {
                            self.push(BlockRole::Paragraph, match item.status { ActionStatus::Done => "[Done]", ActionStatus::InProgress => "[In Progress]", ActionStatus::Pending => "[Pending]" });
                            self.runs(BlockRole::Paragraph, vec![strong(&item.task)]);
                            if let Some(owner) = some(&item.owner) { self.push(BlockRole::Paragraph, format!("Owner: {owner}")); }
                            if let Some(deadline) = some(&item.deadline) { self.push(BlockRole::Paragraph, format!("Due: {deadline}")); }
                        } else {
                            let icon = match (self.flavor, item.status) {
                                (Flavor::Plain, ActionStatus::Done) => "[x]", (Flavor::Plain, ActionStatus::InProgress) => "[-]", (Flavor::Plain, _) => "[ ]",
                                (_, ActionStatus::Done) => "☑", (Flavor::Docx, ActionStatus::InProgress) => "◐", _ => "☐",
                            };
                            let mut runs = vec![run(format!("{icon} "))];
                            if let Some(owner) = some(&item.owner) { runs.extend([strong(owner), run(": ")]); }
                            runs.push(run(&item.task));
                            if let Some(deadline) = some(&item.deadline) { runs.push(meta(format!(" (Due: {deadline})"))); }
                            self.runs(if self.flavor == Flavor::Docx { BlockRole::Paragraph } else { BlockRole::Bullet }, runs);
                        }
                    } self.end();
                }
                if let Some(items) = m.decisions.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Decisions Made");
                    for (i, item) in items.iter().enumerate() {
                        let participants = item.participants.as_ref().filter(|p| !p.is_empty()).map(|p| p.join(", "));
                        let mut runs = vec![run(&item.decision)];
                        if self.flavor == Flavor::Plain {
                            if let Some(p) = participants { runs.push(run(format!(" ({p})"))); }
                            if let Some(at) = item.timestamp { runs.push(run(format!(" [{}]", ts(at)))); }
                        } else {
                            let mut details = vec![];
                            if let Some(p) = participants { details.push(if self.flavor == Flavor::Html { p } else { format!("Participants: {p}") }); }
                            if let Some(at) = item.timestamp { details.push(if self.flavor == Flavor::Html { ts(at) } else { format!("at {}", ts(at)) }); }
                            if !details.is_empty() { runs.push(meta(format!("  {}", details.join(if self.flavor == Flavor::Pdf { "  |  " } else { " | " })))); }
                        }
                        self.runs(if self.flavor == Flavor::Plain { BlockRole::Bullet } else { BlockRole::Numbered(i as u32 + 1) }, runs);
                    } self.end();
                }
                self.strings("Discussion Points", &m.discussion_points, false);
                self.strings("Next Steps", &m.next_steps, true);
            }
            Analysis::Lecture(l) => {
                if let Some(topics) = l.topics.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Topics Covered");
                    for (i, topic) in topics.iter().enumerate() {
                        let mut runs = vec![run(&topic.title)]; let mut details = vec![];
                        if let Some(at) = topic.timestamp { details.push(match self.flavor { Flavor::Plain | Flavor::Html => format!("[{}]", ts(at)), Flavor::Docx => format!("at {}", ts(at)), Flavor::Pdf => format!("Starts at {}", ts(at)) }); }
                        if self.flavor.rich_file()
                            && let Some(duration) = topic.duration.filter(|d| *d != 0.0)
                        {
                            let minutes = (duration / 60.0 + 0.5).floor();
                            details.push(if self.flavor == Flavor::Pdf { format!("Duration: {minutes} min") } else { format!("{minutes} min") });
                        }
                        if !details.is_empty() { runs.push(meta(format!(" {}", details.join(if self.flavor == Flavor::Pdf { "  |  " } else { " | " })))); }
                        self.runs(BlockRole::Numbered(i as u32 + 1), runs);
                    } self.end();
                }
                if self.flavor != Flavor::Docx { self.strings("Study Guide", &l.study_guide, false); }
                if let Some(concepts) = l.concepts.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Key Concepts");
                    for c in concepts {
                        let mut runs = vec![strong(&c.concept)];
                        if !c.explanation.is_empty() { runs.push(run(format!("{}{}", if self.flavor.rich_file() { " — " } else { ": " }, c.explanation))); }
                        self.runs(BlockRole::Bullet, runs);
                        if self.flavor != Flavor::Html
                            && let Some(examples) = nonempty(&c.examples)
                        {
                            self.runs(BlockRole::Paragraph, vec![meta(format!("{}Examples: {}", if self.flavor == Flavor::Plain { "  " } else { "" }, examples.join(", ")))]);
                        }
                    } self.end();
                }
                self.strings("Review Questions", &l.review_questions, true);
                if self.flavor == Flavor::Docx { self.strings("Study Guide", &l.study_guide, false); }
            }
            Analysis::Sales(s) => {
                self.strings("Next Steps", &s.next_steps, true);
                if self.flavor.rich_file() { self.metrics(s); }
                if self.flavor == Flavor::Docx { self.coaching(s); self.customer(s); }
                else { self.customer(s); self.coaching(s); }
                if !self.flavor.rich_file() { self.metrics(s); }
            }
            Analysis::Support(s) => {
                if !self.flavor.rich_file() { self.support_insights(s); }
                self.issue(s);
                if self.flavor.rich_file() || self.flavor == Flavor::Html { self.sentiment(s); }
                if self.flavor == Flavor::Docx { self.support_insights(s); }
                self.strings("Knowledge Base Updates", &s.kb_updates, false);
                if self.flavor == Flavor::Pdf { self.support_insights(s); }
                self.strings("Follow-up Actions", &s.follow_up_actions, true);
                if self.flavor == Flavor::Plain { self.sentiment(s); }
            }
            Analysis::Interview(i) => {
                if let Some(steps) = i.next_steps.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Next Steps");
                    for (n, step) in steps.iter().enumerate() {
                        // D15 approved correction: source plain interpolation produced
                        // [object Object]; render action and Priority, never a fake id.
                        let priority = enum_text(&step.priority);
                        let detail = if self.flavor == Flavor::Html { format!(" {priority}") }
                            else if self.flavor.rich_file() { format!("  Priority: {priority}") }
                            else { format!(" (Priority: {priority})") };
                        self.runs(BlockRole::Numbered(n as u32 + 1), vec![run(&step.action), meta(detail)]);
                    } self.end();
                }
                if self.flavor.rich_file() { self.assessment(i); }
                if let Some(skills) = i.skills_assessment.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Skills Assessment");
                    for skill in skills {
                        let details = match self.flavor {
                            Flavor::Docx => format!("  {}  {}", stars(skill.rating.value()), enum_text(&skill.level)),
                            Flavor::Pdf => format!("  Rating: {}/5  |  Level: {}", skill.rating.value(), enum_text(&skill.level)),
                            _ => format!(" [{}] ({}/5)", enum_text(&skill.level), skill.rating.value()),
                        };
                        if self.flavor == Flavor::Docx { self.runs(BlockRole::Bullet, vec![strong(&skill.skill), toned(run(details), DocumentTone::Muted)]); }
                        else { self.runs(BlockRole::Bullet, vec![run(&skill.skill), meta(details)]); }
                    } self.end();
                }
                if !self.flavor.rich_file() { self.assessment(i); }
                if let Some(responses) = i.notable_responses.as_ref().filter(|v| !v.is_empty()) {
                    self.heading("Notable Responses");
                    for r in responses {
                        if self.flavor == Flavor::Docx {
                            let (icon, tone) = match r.assessment { ResponseAssessment::Positive => ("✓ ", DocumentTone::Positive), ResponseAssessment::Negative => ("✗ ", DocumentTone::Negative), ResponseAssessment::Neutral => ("— ", DocumentTone::Muted) };
                            self.runs(BlockRole::Paragraph, vec![toned(strong(icon), tone), strong(format!("Q: {}", r.question))]);
                        } else if self.flavor.rich_file() || !r.question.is_empty() { self.runs(BlockRole::Paragraph, vec![strong(format!("Q: {}", r.question))]); }
                        if self.flavor.rich_file() || !r.answer.is_empty() { self.push(BlockRole::Paragraph, format!("{}A: {}", if self.flavor == Flavor::Docx { "    " } else { "" }, r.answer)); }
                        self.runs(BlockRole::Paragraph, vec![meta(format!("{}Assessment: {}", if self.flavor == Flavor::Docx { "    " } else { "" }, enum_text(&r.assessment)))]); self.end();
                    }
                }
            }
        }
    }
    fn metrics(&mut self, s: &SalesAnalysis) {
        if let Some(m) = &s.call_metrics {
            self.heading(if self.flavor.rich_file() { "Call Performance" } else { "Call Metrics" });
            let role = if self.flavor == Flavor::Pdf { BlockRole::Paragraph } else { BlockRole::Bullet };
            if let Some(ratio) = some(&m.talk_listen_ratio) { self.push(role, format!("Talk/Listen Ratio: {ratio}")); }
            if let Some(n) = m.questions_asked { self.push(role, format!("Questions Asked: {n}")); }
            if let Some(n) = m.objections_handled { self.push(role, format!("Objections Handled: {n}")); }
            if let Some(yes) = m.next_steps_confirmed { self.push(role, format!("Next Steps Confirmed: {}", if yes { "Yes" } else { "No" })); }
            self.end();
        }
    }
    fn coaching(&mut self, s: &SalesAnalysis) {
        if let Some(tips) = s.coaching_tips.as_ref().filter(|v| !v.is_empty()) {
            self.heading("Sales Coaching");
            for (category, title) in [(CoachingCategory::Strength, "Strengths"), (CoachingCategory::Improvement, "Areas for Improvement")] {
                if tips.iter().any(|t| t.category == category) { self.advice_heading(title, category == CoachingCategory::Strength); for tip in tips.iter().filter(|t| t.category == category) {
                    self.advice_item(&tip.tip, category == CoachingCategory::Strength);
                } self.end(); }
            } self.end();
        }
    }
    fn customer(&mut self, s: &SalesAnalysis) {
        if let Some(c) = &s.customer_insights {
            let fields = [("Pain Points", &c.pain_points), ("Budget Signals", &c.budget_signals), ("Objections", &c.objections), ("Buying Signals", &c.buying_signals)];
            if fields.iter().any(|(_, items)| nonempty(items).is_some()) {
                self.heading("Customer Insights"); for (title, items) in fields { if let Some(items) = nonempty(items) { self.subheading(title); self.list(items); } }
            }
        }
    }
    fn assessment(&mut self, i: &InterviewAnalysis) {
        if let Some(a) = &i.assessment {
            self.heading("Candidate Assessment");
            // Source plain/HTML omit position; preserve that difference.
            if self.flavor.rich_file()
                && let Some(position) = some(&a.position)
            {
                if self.flavor == Flavor::Docx { self.runs(BlockRole::Paragraph, vec![toned(run("Position: "), DocumentTone::Muted), strong(position)]); }
                else { self.field("Position", position); }
            }
            if let Some(rating) = a.overall_rating {
                if self.flavor == Flavor::Docx { self.push(BlockRole::Paragraph, format!("Overall Rating: {} ({}/5)", stars(rating.value()), rating.value())); }
                else { self.field("Overall Rating", format!("{}/5", rating.value())); }
            }
            for (title, items) in [("Strengths", &a.strengths), ("Concerns", &a.concerns)] {
                if let Some(items) = items {
                    if self.flavor.rich_file() {
                        if !items.is_empty() { self.advice_heading(title, title == "Strengths"); for item in items { self.advice_item(item, title == "Strengths"); } self.end(); }
                    } else { self.field(title, items.join(", ")); }
                }
            } self.end();
        }
    }
    fn issue(&mut self, s: &SupportAnalysis) {
        if let Some(i) = &s.issue_resolution {
            self.heading("Issue Resolution");
            let status = enum_text(&i.status); self.field("Status", if self.flavor.rich_file() { status.to_uppercase() } else { status.to_string() });
            if !i.problem.is_empty() { self.field("Problem", &i.problem); }
            if let Some(cause) = some(&i.root_cause) { self.field("Root Cause", cause); }
            if let Some(resolution) = some(&i.resolution) { self.field("Resolution", resolution); }
            if let Some(minutes) = i.resolution_time_minutes.filter(|n| self.flavor == Flavor::Pdf || *n != 0.0) {
                self.field("Resolution Time", format!("{minutes} {}", if self.flavor == Flavor::Pdf { "minutes" } else { "min" }));
            } self.end();
        }
    }
    fn support_insights(&mut self, s: &SupportAnalysis) {
        if let Some(insights) = &s.insights
            && (nonempty(&insights.strengths).is_some() || nonempty(&insights.improvements).is_some())
        {
            self.heading("Support Insights");
            for (title, items) in [("Strengths", &insights.strengths), ("Areas for Improvement", &insights.improvements)] {
                if let Some(items) = nonempty(items) { self.advice_heading(title, title == "Strengths"); for item in items {
                    self.advice_item(item, title == "Strengths");
                } self.end(); }
            }
        }
    }
    fn sentiment(&mut self, s: &SupportAnalysis) {
        if let Some(sentiment) = &s.sentiment {
            self.heading(if self.flavor == Flavor::Pdf { "Customer Sentiment" } else { "Sentiment Analysis" });
            if self.flavor == Flavor::Pdf {
                if !sentiment.initial.is_empty() && !sentiment.r#final.is_empty() { self.push(BlockRole::Paragraph, format!("{} → {}", sentiment.initial, sentiment.r#final)); }
            } else {
                if !sentiment.initial.is_empty() { if self.flavor == Flavor::Docx { self.push(BlockRole::Bullet, format!("Initial: {}", sentiment.initial)); } else { self.field("Initial", &sentiment.initial); } }
                if !sentiment.r#final.is_empty() { if self.flavor == Flavor::Docx { self.push(BlockRole::Bullet, format!("Final: {}", sentiment.r#final)); } else { self.field("Final", &sentiment.r#final); } }
            }
            if let Some(moments) = sentiment.key_moments.as_ref().filter(|m| !m.is_empty()) {
                self.end(); self.subheading("Key Moments"); for m in moments {
                    self.push(BlockRole::Bullet, format!("[{}] {}{}", ts(m.timestamp), enum_text(&m.sentiment), some(&m.reason).map_or_else(String::new, |r| format!(" — {r}"))));
                }
            } self.end();
        }
    }
}
fn stars(rating: f64) -> String { format!("{}{}", "★".repeat(rating.floor() as usize), "☆".repeat((5.0 - rating).floor() as usize)) }

fn markdown(blocks: &[DocumentBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block.role {
            BlockRole::Title => out.push_str("# "), BlockRole::Heading(level) => { for _ in 0..level { out.push('#'); } out.push(' '); }
            BlockRole::Bullet => out.push_str("- "), BlockRole::Numbered(n) => { let _ = write!(out, "{n}. "); }
            BlockRole::Divider => continue, _ => {}
        }
        for r in &block.runs { if r.bold { out.push_str("**"); } out.push_str(&r.text); if r.bold { out.push_str("**"); } }
        out.push('\n'); if block.blank_after { out.push('\n'); }
    }
    out
}
fn escape_html(out: &mut String, text: &str) {
    for c in text.chars() { match c { '&' => out.push_str("&amp;"), '<' => out.push_str("&lt;"), '>' => out.push_str("&gt;"), '"' => out.push_str("&quot;"), '\'' => out.push_str("&#39;"), _ => out.push(c) } }
}
fn html_style(tag: &str) -> &'static str {
    match tag {
        "h1" => "font-size: 1.4em; margin: 0 0 0.5em; color: #1a1a1a; border-bottom: 2px solid #e5e5e5; padding-bottom: 0.3em;",
        "h2" => "font-size: 1.15em; margin: 1.2em 0 0.4em; color: #222; border-bottom: 1px solid #eee; padding-bottom: 0.2em;",
        "h3" => "font-size: 1em; margin: 0.8em 0 0.3em; color: #444;",
        "ul" | "ol" => "margin: 0 0 0.8em; padding-left: 1.5em;",
        "li" => "margin-bottom: 0.3em;",
        _ => "margin: 0 0 0.8em; color: #444; line-height: 1.7;",
    }
}
fn clipboard_html(blocks: &[DocumentBlock]) -> String {
    let mut out = String::from("<div style=\"font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333;\">");
    let mut list = None;
    for block in blocks {
        let next = match block.role { BlockRole::Bullet => Some("ul"), BlockRole::Numbered(_) => Some("ol"), _ => None };
        if list != next {
            if let Some(tag) = list { let _ = write!(out, "</{tag}>"); }
            if let Some(tag) = next { let _ = write!(out, "<{tag} style=\"{}\">", html_style(tag)); }
            list = next;
        }
        let tag = match block.role { BlockRole::Title => "h1", BlockRole::Heading(1) => "h1", BlockRole::Heading(2) => "h2", BlockRole::Heading(_) => "h3", BlockRole::Bullet | BlockRole::Numbered(_) => "li", BlockRole::Divider => continue, _ => "p" };
        let _ = write!(out, "<{tag} style=\"{}\">", html_style(tag));
        for r in &block.runs {
            if r.bold { out.push_str("<strong>"); }
            if r.italic { out.push_str("<span style=\"font-size: 0.85em; color: #888; font-style: italic;\">"); }
            escape_html(&mut out, &r.text);
            if r.italic { out.push_str("</span>"); }
            if r.bold { out.push_str("</strong>"); }
        }
        let _ = write!(out, "</{tag}>");
    }
    if let Some(tag) = list { let _ = write!(out, "</{tag}>"); } out.push_str("</div>"); out
}

#[cfg(test)]
mod tests;
