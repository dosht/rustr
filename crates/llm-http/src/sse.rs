//! Incremental SSE framing; one record at a time avoids a per-chunk event queue.
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent { pub event: String, pub id: Option<String>, pub data: String }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError { Limit, Utf8, Incomplete, Json, Depth, Envelope, Event }
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "stream codec {self:?}") }
}
impl std::error::Error for CodecError {}

pub struct SseDecoder {
    line: Vec<u8>, data: String, event: String, id: Option<String>, record_bytes: usize,
    limit: usize, saw_data: bool, first_line: bool, failed: bool,
}
impl SseDecoder {
    pub fn new(record_bytes: usize) -> Self {
        Self { line: Vec::new(), data: String::new(), event: String::new(), id: None,
            record_bytes: 0, limit: record_bytes, saw_data: false, first_line: true, failed: false }
    }
    /// Consumes at most one dispatched record; caller loops over the unused suffix.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(usize, Option<SseEvent>), CodecError> {
        if self.failed { return Err(CodecError::Incomplete); }
        let result = self.consume(bytes);
        if result.is_err() { self.failed = true; }
        result
    }
    fn consume(&mut self, bytes: &[u8]) -> Result<(usize, Option<SseEvent>), CodecError> {
        let mut consumed = 0;
        while consumed < bytes.len() {
            let rest = &bytes[consumed..];
            let take = rest.iter().position(|b| *b == b'\n').map_or(rest.len(), |i| i + 1);
            if take > self.limit.saturating_sub(self.record_bytes) { return Err(CodecError::Limit); }
            self.record_bytes += take;
            self.line.extend_from_slice(&rest[..take]); consumed += take;
            if self.line.last() != Some(&b'\n') { break; }
            self.line.pop();
            if self.line.last() == Some(&b'\r') { self.line.pop(); }
            let mut line = std::str::from_utf8(&self.line).map_err(|_| CodecError::Utf8)?;
            if self.first_line { line = line.strip_prefix('\u{feff}').unwrap_or(line); self.first_line = false; }
            if line.is_empty() {
                self.record_bytes = 0; self.line.clear();
                if self.saw_data {
                    if self.data.ends_with('\n') { self.data.pop(); }
                    let event = SseEvent { event: std::mem::take(&mut self.event), id: self.id.take(), data: std::mem::take(&mut self.data) };
                    self.saw_data = false;
                    return Ok((consumed, Some(event)));
                }
                self.event.clear(); self.id = None;
                continue;
            }
            if !line.starts_with(':') {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "data" => { self.data.push_str(value); self.data.push('\n'); self.saw_data = true; }
                    "event" => { self.event.clear(); self.event.push_str(value); }
                    "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
                    _ => {}
                }
            }
            self.line.clear();
        }
        Ok((consumed, None))
    }
    /// EOF never dispatches an unterminated record.
    pub fn finish(&self) -> Result<(), CodecError> {
        if self.failed || !self.line.is_empty() || self.saw_data { Err(CodecError::Incomplete) } else { Ok(()) }
    }
}

/// Validate nesting before serde allocates a tree; braces in strings do not count.
pub(crate) fn check_json(bytes: &[u8], depth_limit: usize) -> Result<(), CodecError> {
    let mut depth = 0usize; let mut quoted = false; let mut escaped = false;
    for &b in bytes {
        if quoted {
            if escaped { escaped = false; } else if b == b'\\' { escaped = true; } else if b == b'"' { quoted = false; }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' | b'[' => { depth += 1; if depth > depth_limit { return Err(CodecError::Depth); } }
                b'}' | b']' => { depth = depth.checked_sub(1).ok_or(CodecError::Json)?; }
                _ => {}
            }
        }
    }
    if quoted || depth != 0 { Err(CodecError::Json) } else { Ok(()) }
}
