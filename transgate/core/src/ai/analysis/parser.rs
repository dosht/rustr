use super::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
use super::super::sanitize::js_trim;

/// Accepts decoded UTF-8 completion text, not transport SSE. One unfinished
/// document and bounded pre-type sections; no per-token transcript reparse/copy.
#[derive(Default)]
pub struct AnalysisParser {
    document: Vec<u8>,
    marker_bytes: u8,
    depth: usize,
    quoted: bool,
    escaped: bool,
    bytes: usize,
    documents: usize,
    expected_documents: Option<usize>,
    deferred: Vec<(String, Value)>,
    progress: AnalysisProgress,
    error: Option<AnalysisError>,
    finalized: bool,
}

impl AnalysisParser {
    pub fn new() -> Self { Self::default() }
    pub fn progress(&self) -> &AnalysisProgress { &self.progress }

    /// Move the retained typed partial to its owner without cloning or boxing.
    pub fn into_progress(self) -> AnalysisProgress { self.progress }

    pub fn push(&mut self, text: &str) -> Result<(), AnalysisError> {
        if let Some(error) = self.error { return Err(error); }
        if self.finalized { return Err(AnalysisError::AlreadyFinished); }
        if text.len() > MAX_OUTPUT_BYTES.saturating_sub(self.bytes) {
            return self.fail(AnalysisError::OutputTooLarge);
        }
        self.bytes += text.len();
        for byte in text.bytes() {
            // D8: separator markers need not occupy an otherwise empty line.
            // Hold at most two hyphens so split/adjacent markers work without
            // making delimiters part of the document's byte allowance.
            if !self.quoted && self.depth == 0 && byte == b'-' {
                self.marker_bytes += 1;
                if self.marker_bytes == 3 {
                    self.marker_bytes = 0;
                    if let Err(error) = self.consume_document() { return self.fail(error); }
                    self.document.clear();
                }
                continue;
            }
            for _ in 0..self.marker_bytes { self.document.push(b'-'); }
            self.marker_bytes = 0;
            self.document.push(byte);
            if self.document.len() > MAX_DOCUMENT_BYTES {
                return self.fail(AnalysisError::DocumentTooLarge);
            }
            if self.quoted {
                if self.escaped { self.escaped = false; }
                else if byte == b'\\' { self.escaped = true; }
                else if byte == b'"' { self.quoted = false; }
            } else {
                match byte {
                    b'"' => self.quoted = true,
                    b'{' | b'[' => {
                        self.depth += 1;
                        if self.depth > MAX_JSON_DEPTH { return self.fail(AnalysisError::TooDeep); }
                    }
                    b'}' | b']' => {
                        if self.depth == 0 { return self.fail(AnalysisError::MalformedDocument); }
                        self.depth -= 1;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn fail<T>(&mut self, error: AnalysisError) -> Result<T, AnalysisError> {
        self.error = Some(error);
        Err(error)
    }

    /// `successful_stop` MUST mean an actual recognized successful provider stop,
    /// not transport EOF, [DONE], length, filtering, tool calls or cancellation.
    /// Success moves the typed state into the result. Failure retains the final
    /// validated partial in progress()/into_progress(); either outcome is terminal.
    pub fn finish(&mut self, successful_stop: bool) -> Result<AnalysisResult, AnalysisError> {
        if self.finalized { return Err(self.error.unwrap_or(AnalysisError::AlreadyFinished)); }
        self.finalized = true;
        if self.error.is_none() {
            for _ in 0..self.marker_bytes { self.document.push(b'-'); }
            if let Err(error) = self.consume_document() { self.error = Some(error); }
        }
        if self.error.is_none() && !successful_stop { self.error = Some(AnalysisError::UnsuccessfulFinish); }
        if self.error.is_none() && self.progress.detected.is_none() { self.error = Some(AnalysisError::MissingDetectedType); }
        if self.error.is_none() && self.progress.summary.as_deref().is_none_or(|s| js_trim(s).is_empty()) {
            self.error = Some(AnalysisError::MissingSummary);
        }
        if let Some(expected) = self.expected_documents && expected != self.documents {
            self.progress.warnings.push(AnalysisWarning::DocumentCountMismatch { expected, actual: self.documents });
        }
        if let Some(error) = self.error { return Err(error); }
        self.document.clear();
        let progress = std::mem::take(&mut self.progress);
        // Presence checked above; keep failure paths explicit, not invented defaults.
        match (progress.detected, progress.summary, progress.analysis) {
            (Some(detected), Some(summary), Some(analysis)) => Ok(AnalysisResult {
                detected, summary, analysis, warnings: progress.warnings,
            }),
            _ => unreachable!("validated detected type initializes typed analysis"),
        }
    }

    fn consume_document(&mut self) -> Result<(), AnalysisError> {
        if self.document.len() > MAX_DOCUMENT_BYTES { return Err(AnalysisError::DocumentTooLarge); }
        let text = js_trim(std::str::from_utf8(&self.document).map_err(|_| AnalysisError::MalformedDocument)?);
        if text.is_empty() { return Ok(()); }
        // Only the first document may be the restricted legacy YAML count header.
        if self.documents == 0 && let Some(number) = text.strip_prefix("documentCount:") {
            if text.bytes().any(|b| matches!(b, b'\r' | b'\n')) { return Err(AnalysisError::MalformedDocument); }
            let number = js_trim(number);
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) { return Err(AnalysisError::MalformedDocument); }
            let n = number.parse::<usize>().map_err(|_| AnalysisError::InvalidSection)?;
            self.header(n)?;
            self.documents += 1;
            return Ok(());
        }
        let value: Value = serde_json::from_str(text).map_err(|_| AnalysisError::MalformedDocument)?;
        let Value::Object(mut object) = value else { return Err(AnalysisError::MalformedDocument); };
        if let Some(number) = object.get("documentCount") {
            if self.documents != 0 || object.len() != 1 { return Err(AnalysisError::InvalidSection); }
            // JS JSON numbers have no integer representation distinction (5.0 is 5).
            let n = number.as_f64().filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < usize::MAX as f64)
                .ok_or(AnalysisError::InvalidSection)? as usize;
            self.header(n)?;
        } else {
            let section = object.remove("type").and_then(|v| v.as_str().map(str::to_owned)).ok_or(AnalysisError::InvalidSection)?;
            if section == "detectedType" {
                let kind: AnalysisType = serde_json::from_value(object.remove("value").ok_or(AnalysisError::InvalidSection)?)
                    .map_err(|_| AnalysisError::InvalidSection)?;
                let confidence = object.get("confidence").and_then(Value::as_f64)
                    .filter(|n| n.is_finite()).unwrap_or(0.0);
                let detected = DetectedType { kind, confidence };
                match self.progress.detected {
                    Some(previous) if previous != detected => return Err(AnalysisError::ConflictingDuplicate),
                    Some(_) => {}
                    None => {
                        let mut analysis = Analysis::empty(kind);
                        if let Analysis::Generic(g) = &mut analysis { g.summary = self.progress.summary.clone(); }
                        self.progress.detected = Some(detected);
                        self.progress.analysis = Some(analysis);
                        // Applicability is unknowable before type detection.
                        // Raw deferred values share the total generated bound;
                        // apply in arrival order and retain valid predecessors.
                        for (section, data) in std::mem::take(&mut self.deferred) {
                            let added = apply(self.progress.analysis.as_mut().expect("type initialized"), &section, data)?;
                            self.progress.content_sections += usize::from(added);
                        }
                    }
                }
            } else {
                // Source JavaScript `data || items || content`: [] and {} are
                // truthy, unlike null, false, zero and an empty string.
                let data = object.remove("data").filter(truthy)
                    .or_else(|| object.remove("items").filter(truthy))
                    .or_else(|| object.remove("content").filter(truthy))
                    .ok_or(AnalysisError::InvalidSection)?;
                let added = if section == "summary" {
                    let added = assign(&mut self.progress.summary, data)?;
                    if let Some(Analysis::Generic(g)) = &mut self.progress.analysis { g.summary = self.progress.summary.clone(); }
                    added
                } else if let Some(analysis) = self.progress.analysis.as_mut() {
                    apply(analysis, &section, data)?
                } else {
                    self.deferred.push((section, data));
                    false
                };
                self.progress.content_sections += usize::from(added);
            }
        }
        self.documents += 1;
        Ok(())
    }

    fn header(&mut self, n: usize) -> Result<(), AnalysisError> {
        if self.expected_documents.is_some() { return Err(AnalysisError::ConflictingDuplicate); }
        self.expected_documents = Some(n);
        self.progress.expected_content_sections = Some(n.saturating_sub(2));
        Ok(())
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn assign<T: DeserializeOwned + PartialEq>(slot: &mut Option<T>, value: Value) -> Result<bool, AnalysisError> {
    let next = serde_json::from_value(value).map_err(|_| AnalysisError::InvalidSection)?;
    match slot {
        Some(previous) if *previous != next => Err(AnalysisError::ConflictingDuplicate),
        Some(_) => Ok(false),
        None => { *slot = Some(next); Ok(true) }
    }
}

fn apply(analysis: &mut Analysis, section: &str, value: Value) -> Result<bool, AnalysisError> {
    match (analysis, section) {
        (Analysis::Meeting(m), "actionItems") => assign(&mut m.action_items, value),
        (Analysis::Meeting(m), "decisions") => assign(&mut m.decisions, value),
        (Analysis::Meeting(m), "discussionPoints") => assign(&mut m.discussion_points, value),
        (Analysis::Meeting(m), "nextSteps") => assign(&mut m.next_steps, value),
        (Analysis::Lecture(l), "topics") => assign(&mut l.topics, value),
        (Analysis::Lecture(l), "concepts") => assign(&mut l.concepts, value),
        (Analysis::Lecture(l), "reviewQuestions") => assign(&mut l.review_questions, value),
        (Analysis::Lecture(l), "studyGuide") => assign(&mut l.study_guide, value),
        (Analysis::Sales(s), "callMetrics") => assign(&mut s.call_metrics, value),
        (Analysis::Sales(s), "coachingTips") => assign(&mut s.coaching_tips, value),
        (Analysis::Sales(s), "customerInsights") => assign(&mut s.customer_insights, value),
        (Analysis::Sales(s), "nextSteps") => assign(&mut s.next_steps, value),
        (Analysis::Support(s), "issueResolution") => assign(&mut s.issue_resolution, value),
        (Analysis::Support(s), "sentiment") => assign(&mut s.sentiment, value),
        (Analysis::Support(s), "insights") => assign(&mut s.insights, value),
        (Analysis::Support(s), "kbUpdates") => assign(&mut s.kb_updates, value),
        (Analysis::Support(s), "followUpActions") => assign(&mut s.follow_up_actions, value),
        (Analysis::Interview(i), "assessment") => assign(&mut i.assessment, value),
        (Analysis::Interview(i), "skillsAssessment") => assign(&mut i.skills_assessment, value),
        (Analysis::Interview(i), "notableResponses") => assign(&mut i.notable_responses, value),
        (Analysis::Interview(i), "nextSteps") => assign(&mut i.next_steps, value),
        (Analysis::Generic(g), "highlights") => assign(&mut g.highlights, value),
        _ => Err(AnalysisError::InvalidSection),
    }
}
