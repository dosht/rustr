//! The HTTP mechanics the Express service provided: `cors` with an origin
//! list, `express.json()`, the extended query parser, JSON and streamed file
//! responses, and the default 404 page.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use bytes::Bytes;
use futures_core::Stream;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use tokio_util::io::ReaderStream;
use transgate_core::tools::{Arg, Refusal, RefusalKind};

use crate::js::J;

/// The source error middleware's answer to anything unexpected (`index.ts:60-64`).
pub const SOMETHING_WENT_WRONG: &str = "Something went wrong while processing your file. Please try again.";

/// `express.json()` default limit.
const JSON_LIMIT: usize = 100 * 1024;

/// One request as a tool route sees it. The body is `!Sync`, so a handler
/// that awaits takes the request by value or borrows only `query`: holding
/// `&Incoming` across an await makes the route future `!Send`.
pub struct Incoming {
    pub method: Method,
    pub query: Query,
    pub headers: HeaderMap,
    /// The address the processing allowance is keyed by.
    pub ip: String,
    pub body: Body,
}

#[derive(Debug, Default)]
pub struct Query(HashMap<String, QueryValue>);

#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    One(String),
    /// Repeated key.
    Many(Vec<String>),
    /// Bracket notation (`a[b]=c`), an object to `qs`.
    Nested,
}

fn percent_decode(raw: &str) -> String {
    let spaced = raw.replace('+', " ");
    let bytes = spaced.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
            match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                // decodeURIComponent throws; qs then keeps the raw text.
                None => return spaced,
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or(spaced)
}

impl Query {
    pub fn parse(raw: Option<&str>) -> Query {
        let mut map: HashMap<String, QueryValue> = HashMap::new();
        for pair in raw.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let key = percent_decode(k);
            let value = percent_decode(v);
            if key.is_empty() {
                continue;
            }
            if let Some(open) = key.find('[').filter(|at| *at > 0) {
                map.insert(key[..open].to_string(), QueryValue::Nested);
                continue;
            }
            let next = match map.remove(&key) {
                None => QueryValue::One(value),
                Some(QueryValue::One(first)) => QueryValue::Many(vec![first, value]),
                Some(QueryValue::Many(mut all)) => {
                    all.push(value);
                    QueryValue::Many(all)
                }
                Some(QueryValue::Nested) => QueryValue::Nested,
            };
            map.insert(key, next);
        }
        Query(map)
    }

    pub fn get(&self, key: &str) -> Option<&QueryValue> {
        self.0.get(key)
    }

    /// The value when it is a single string (`typeof === 'string'`).
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.0.get(key) {
            Some(QueryValue::One(s)) => Some(s),
            _ => None,
        }
    }

    /// Truthiness of `req.query[key]`.
    pub fn truthy(&self, key: &str) -> bool {
        match self.0.get(key) {
            None => false,
            Some(QueryValue::One(s)) => !s.is_empty(),
            Some(_) => true,
        }
    }
}

#[derive(Debug)]
pub enum BodyError {
    TimedOut,
    /// body-parser rejected it; the error middleware answers 500.
    Rejected,
}

/// Decoded body bytes that refuse to grow past the limit, so a decoder can
/// never expand one compressed frame (or its final flush) beyond it.
struct Capped(Vec<u8>);

impl std::io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.0.len() + buf.len() > JSON_LIMIT {
            return Err(io::Error::other("request entity too large"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The decoded body as body-parser 1.20 reads it: `identity`, `gzip` or
/// `deflate` (zlib) content encoding, with the 100 kb limit on decoded bytes.
enum Decoded {
    Identity(Capped),
    Gzip(flate2::write::MultiGzDecoder<Capped>),
    Deflate(flate2::write::ZlibDecoder<Capped>),
}

impl Decoded {
    fn for_encoding(encoding: &str) -> Option<Decoded> {
        let sink = Capped(Vec::new());
        match encoding {
            "identity" => Some(Decoded::Identity(sink)),
            "gzip" => Some(Decoded::Gzip(flate2::write::MultiGzDecoder::new(sink))),
            "deflate" => Some(Decoded::Deflate(flate2::write::ZlibDecoder::new(sink))),
            _ => None,
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), BodyError> {
        use std::io::Write;
        match self {
            Decoded::Identity(sink) => sink.write_all(data),
            Decoded::Gzip(d) => d.write_all(data),
            Decoded::Deflate(d) => d.write_all(data),
        }
        .map_err(|_| BodyError::Rejected)
    }

    fn finish(self) -> Result<Vec<u8>, BodyError> {
        match self {
            Decoded::Identity(sink) => Ok(sink),
            Decoded::Gzip(d) => d.finish(),
            Decoded::Deflate(d) => d.finish(),
        }
        .map(|sink| sink.0)
        .map_err(|_| BodyError::Rejected)
    }
}

/// The `charset` parameter, lower-cased, as `content-type` parses it.
fn charset(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches('"').to_ascii_lowercase())
    })
}

/// UTF-16 code units of big- or little-endian bytes; an odd trailing byte is
/// dropped, as iconv-lite's decoders drop it.
fn utf16_units(bytes: &[u8], little: bool) -> impl Iterator<Item = u16> + '_ {
    bytes.as_chunks::<2>().0.iter().map(move |bytes| if little { u16::from_le_bytes(*bytes) } else { u16::from_be_bytes(*bytes) })
}

/// iconv-lite 0.4.24's UTF-16 decoder: a byte order mark, else more
/// ASCII-shaped code units in the first 64 bytes, else little-endian.
fn utf16_detect_little(bytes: &[u8]) -> bool {
    match bytes {
        [0xFE, 0xFF, ..] => false,
        [0xFF, 0xFE, ..] => true,
        _ => {
            let head = &bytes[..(bytes.len() - bytes.len() % 2).min(64)];
            let (mut le, mut be) = (0, 0);
            for pair in head.as_chunks::<2>().0 {
                be += usize::from(pair[0] == 0 && pair[1] != 0);
                le += usize::from(pair[0] != 0 && pair[1] == 0);
            }
            be <= le
        }
    }
}

/// iconv-lite 0.4.24's UTF-7 (RFC 2152) and UTF-7-IMAP (RFC 3501) decoders:
/// direct bytes as ASCII (high bytes as U+FFFD), `+`/`&` opening a modified
/// base64 run of UTF-16BE that ends at the first non-base64 byte (a `-` there
/// is absorbed; `+-`/`&-` is the shift character itself). Node's base64 decode
/// keeps the bits of an unpadded tail.
fn utf7_units(bytes: &[u8], imap: bool) -> Vec<u16> {
    let shift = if imap { b'&' } else { b'+' };
    let is_base64 = |b: u8| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || (imap && b == b',');
    let ascii = |run: &[u8], units: &mut Vec<u16>| units.extend(run.iter().map(|&b| if b < 0x80 { u16::from(b) } else { 0xFFFD }));
    let base64 = |run: &[u8], units: &mut Vec<u16>| {
        let (mut acc, mut bits, mut out) = (0u32, 0u32, Vec::new());
        for &b in run {
            let v = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                _ => 63, // '/' and IMAP ','
            };
            acc = (acc << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        units.extend(utf16_units(&out, false));
    };
    let mut units = Vec::new();
    let (mut in_base64, mut last, mut i) = (false, 0, 0);
    while i < bytes.len() {
        if !in_base64 {
            if bytes[i] == shift {
                ascii(&bytes[last..i], &mut units);
                last = i + 1;
                in_base64 = true;
            }
        } else if !is_base64(bytes[i]) {
            if i == last && bytes[i] == b'-' {
                units.push(u16::from(shift));
            } else {
                base64(&bytes[last..i], &mut units);
            }
            // A '-' closes the run; any other byte is read again as direct text.
            last = if bytes[i] == b'-' { i + 1 } else { i };
            in_base64 = false;
            if bytes[i] != b'-' {
                continue;
            }
        }
        i += 1;
    }
    if in_base64 { base64(&bytes[last..], &mut units) } else { ascii(&bytes[last..], &mut units) }
    units
}

/// The installed source's decode (iconv-lite 0.4.24, via raw-body) of the
/// charsets body-parser lets through (`utf-` prefixed): utf-8, utf-16le,
/// utf-16be, utf-16, utf-7 and utf-7-imap, after iconv's name canonicalization.
/// Bad input becomes U+FFFD (a lone surrogate, which a JS string would keep,
/// included) and a leading byte order mark is stripped. `None` for a charset
/// iconv-lite 0.4.24 does not have (it has no UTF-32), which body-parser
/// answers 415.
fn decode_text(bytes: &[u8], charset: &str) -> Option<String> {
    let lower = charset.to_lowercase();
    let name = match lower.rfind(':') {
        Some(at) if lower.len() - at == 5 && lower[at + 1..].bytes().all(|b| b.is_ascii_digit()) => &lower[..at],
        _ => &lower[..],
    };
    let canonical: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let text = match canonical.as_str() {
        "utf8" => String::from_utf8_lossy(bytes).into_owned(),
        "utf16le" => String::from_utf16_lossy(&utf16_units(bytes, true).collect::<Vec<_>>()),
        "utf16be" => String::from_utf16_lossy(&utf16_units(bytes, false).collect::<Vec<_>>()),
        "utf16" => String::from_utf16_lossy(&utf16_units(bytes, utf16_detect_little(bytes)).collect::<Vec<_>>()),
        "utf7" => String::from_utf16_lossy(&utf7_units(bytes, false)),
        "utf7imap" => String::from_utf16_lossy(&utf7_units(bytes, true)),
        _ => return None,
    };
    Some(text.strip_prefix('\u{FEFF}').map(str::to_string).unwrap_or(text))
}

/// `express.json()`: a request without a body, or not `application/json`,
/// leaves the body `{}`; an array body has no named fields. Unsupported
/// charsets and encodings, oversized or malformed bodies are rejected.
pub async fn json_body(headers: &HeaderMap, body: Body) -> Result<Map<String, Value>, BodyError> {
    let has_body = headers.contains_key(header::TRANSFER_ENCODING)
        || headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).is_some_and(|v| !crate::js::number(v).is_nan());
    let content_type = headers.get(header::CONTENT_TYPE).map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    let media_type = content_type.as_deref().map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase());
    if !has_body || media_type.as_deref() != Some("application/json") {
        return Ok(Map::new());
    }
    let charset = content_type.as_deref().and_then(charset).unwrap_or_else(|| "utf-8".into());
    if !charset.starts_with("utf-") {
        return Err(BodyError::Rejected);
    }
    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).to_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "identity".into());
    let mut decoded = Decoded::for_encoding(&encoding).ok_or(BodyError::Rejected)?;
    let mut body = body;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.map_err(|_| BodyError::Rejected)?.into_data() {
            decoded.write(&data)?;
        }
    }
    let bytes = decoded.finish()?;
    let text = decode_text(&bytes, &charset).ok_or(BodyError::Rejected)?;
    let first = text.trim_start_matches([' ', '\t', '\n', '\r']).chars().next();
    match first {
        None => Ok(Map::new()),
        Some('{') | Some('[') => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Ok(Map::new()),
            Err(_) => Err(BodyError::Rejected),
        },
        Some(_) => Err(BodyError::Rejected),
    }
}

/// A JSON body field as the core policy reads it.
pub fn arg<'a>(body: &'a Map<String, Value>, key: &str) -> Arg<'a> {
    match body.get(key) {
        None => Arg::Absent,
        Some(Value::Null) => Arg::Null,
        Some(Value::Bool(b)) => Arg::Bool(*b),
        Some(Value::Number(n)) => Arg::Number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => Arg::Text(s),
        Some(_) => Arg::Composite,
    }
}

/// `res.status(status).json(value)`.
pub fn json(status: u16, value: J) -> Response {
    let body = serde_json::to_vec(&value).expect("serializable response");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .expect("valid response")
}

pub fn error(status: u16, message: &str) -> Response {
    json(status, crate::obj! { "error" => message })
}

pub fn refusal(r: Refusal) -> Response {
    let status = match r.kind {
        RefusalKind::Invalid => 400,
        RefusalKind::Expired => 410,
        RefusalKind::Busy => 409,
        RefusalKind::Limited => 429,
    };
    error(status, r.message)
}

pub fn body_error(e: BodyError) -> Response {
    match e {
        BodyError::TimedOut => request_timeout(),
        BodyError::Rejected => error(500, SOMETHING_WENT_WRONG),
    }
}

/// Node's `server.requestTimeout` answer.
pub fn request_timeout() -> Response {
    Response::builder()
        .status(StatusCode::REQUEST_TIMEOUT)
        .header(header::CONNECTION, "close")
        .body(Body::empty())
        .expect("valid response")
}

/// Pieces of a streamed response body.
pub enum Segment {
    Bytes(Bytes),
    File(tokio::fs::File),
}

/// Streams segments in order; `on_end` runs once every byte was read (the
/// source's `fileStream.on('end')`), not when the client stops early. It fires
/// as the final byte is yielded: with a `Content-Length`, hyper stops polling
/// once that many bytes are written and may never ask for the end of stream.
/// A file that ends short of (or runs past) the announced length is an error,
/// and the body is never reported complete.
pub struct Streamed {
    segments: VecDeque<Segment>,
    current: Option<ReaderStream<tokio::fs::File>>,
    len: u64,
    remaining: u64,
    on_end: Option<Box<dyn FnOnce() + Send>>,
    failed: bool,
}

impl Streamed {
    /// `len` is the exact total of all segments.
    pub fn new(len: u64, segments: Vec<Segment>, on_end: Option<Box<dyn FnOnce() + Send>>) -> Self {
        Streamed { segments: segments.into(), current: None, len, remaining: len, on_end, failed: false }
    }

    fn end(&mut self) {
        if let Some(done) = self.on_end.take() {
            done();
        }
    }

    fn fail(&mut self, kind: io::ErrorKind, message: &str) -> Poll<Option<io::Result<Bytes>>> {
        self.failed = true;
        self.on_end = None;
        Poll::Ready(Some(Err(io::Error::new(kind, message.to_string()))))
    }

    fn emit(&mut self, chunk: Bytes) -> Poll<Option<io::Result<Bytes>>> {
        let Some(remaining) = self.remaining.checked_sub(chunk.len() as u64) else {
            return self.fail(io::ErrorKind::InvalidData, "file is longer than its announced length");
        };
        self.remaining = remaining;
        if remaining == 0 {
            self.end();
        }
        Poll::Ready(Some(Ok(chunk)))
    }
}

impl Stream for Streamed {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }
        loop {
            if let Some(current) = self.current.as_mut() {
                match Pin::new(current).poll_next(cx) {
                    Poll::Ready(None) => self.current = None,
                    Poll::Ready(Some(Ok(chunk))) => return self.emit(chunk),
                    Poll::Ready(Some(Err(e))) => {
                        let message = e.to_string();
                        return self.fail(e.kind(), &message);
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }
            match self.segments.pop_front() {
                Some(Segment::Bytes(b)) => return self.emit(b),
                Some(Segment::File(f)) => self.current = Some(ReaderStream::with_capacity(f, 64 * 1024)),
                None if self.remaining == 0 => return Poll::Ready(None),
                None => return self.fail(io::ErrorKind::UnexpectedEof, "file ended before its announced length"),
            }
        }
    }
}

/// A file response. `headers` are set as the source sets them.
pub fn stream(headers: &[(header::HeaderName, String)], mut body: Streamed) -> Response {
    // Nothing will be polled from an empty body: it has been read in full now.
    if body.len == 0 {
        body.end();
    }
    let mut builder = Response::builder().status(200);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    builder.header(header::CONTENT_LENGTH, body.len).body(Body::from_stream(body)).expect("valid response")
}

/// `getClientIp`: the entry the trusted proxy appended, counted from the right
/// of `X-Forwarded-For`; the socket peer without one.
pub fn client_ip(headers: &HeaderMap, peer: Option<std::net::IpAddr>, trusted_proxies: usize) -> String {
    let joined: Vec<String> = headers
        .get_all("x-forwarded-for")
        .iter()
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .collect();
    let entries: Vec<&str> = joined.iter().flat_map(|v| v.split(',')).map(str::trim).filter(|e| !e.is_empty()).collect();
    if !entries.is_empty() {
        return entries[entries.len().saturating_sub(trusted_proxies)].to_string();
    }
    peer.map_or_else(|| "unknown".to_string(), |ip| ip.to_string())
}

/// `cors({ origin: ALLOWED_ORIGINS, methods })`: answers every OPTIONS.
pub fn preflight(headers: &HeaderMap, origins: &[String]) -> Response {
    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    if let Some(origin) = allowed_origin(headers, origins) {
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    builder = builder.header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET,POST,DELETE,OPTIONS");
    if let Some(requested) = headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_HEADERS, requested.clone());
    }
    builder
        .header(header::VARY, "Origin, Access-Control-Request-Headers")
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .expect("valid response")
}

pub fn add_cors(response: &mut Response, request_headers: &HeaderMap, origins: &[String]) {
    if let Some(origin) = allowed_origin(request_headers, origins) {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response.headers_mut().append(header::VARY, HeaderValue::from_static("Origin"));
}

fn allowed_origin(headers: &HeaderMap, origins: &[String]) -> Option<HeaderValue> {
    let origin = headers.get(header::ORIGIN)?;
    origins.iter().any(|o| o.as_bytes() == origin.as_bytes()).then(|| origin.clone())
}

/// Express's default 404 for an unmatched method and path.
pub fn not_found(method: &Method, path: &str) -> Response {
    let escaped = path.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;");
    let body = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Error</title>\n</head>\n<body>\n<pre>Cannot {method} {escaped}</pre>\n</body>\n</html>\n"
    );
    Response::builder()
        .status(404)
        .header("content-security-policy", "default-src 'none'")
        .header("x-content-type-options", "nosniff")
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_ip_is_the_trusted_proxys_entry_not_the_leftmost() {
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", "6.6.6.6, 1.2.3.4".parse().unwrap());
        let peer = Some("10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&h, peer, 1), "1.2.3.4");
        assert_eq!(client_ip(&h, peer, 2), "6.6.6.6");
        assert_eq!(client_ip(&h, peer, 5), "6.6.6.6");
        assert_eq!(client_ip(&HeaderMap::new(), peer, 1), "10.0.0.1");
    }

    /// A known-length body is complete when its last byte is handed over; hyper
    /// may drop it then without polling for the end of the stream.
    #[tokio::test]
    async fn completion_fires_with_the_final_byte_not_a_later_end_of_stream_poll() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let path = std::env::temp_dir().join(format!("tg-streamed-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, vec![7u8; 200_000]).unwrap();
        let file = tokio::fs::File::open(&path).await.unwrap();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        let segments = vec![Segment::Bytes(Bytes::from_static(b"head")), Segment::File(file), Segment::Bytes(Bytes::from_static(b"tail"))];
        let mut body = Streamed::new(200_008, segments, Some(Box::new(move || flag.store(true, Ordering::SeqCst))));
        let mut received = 0u64;
        while received < 200_008 {
            assert!(!fired.load(Ordering::SeqCst), "fired before the last byte, at {received}");
            let chunk = std::future::poll_fn(|cx| Pin::new(&mut body).poll_next(cx)).await.unwrap().unwrap();
            received += chunk.len() as u64;
        }
        assert!(fired.load(Ordering::SeqCst), "the final chunk completes the body without a further poll");
        drop(body);
        std::fs::remove_file(&path).unwrap();

        let empty_fired = Arc::new(AtomicBool::new(false));
        let flag = empty_fired.clone();
        let response = stream(&[], Streamed::new(0, vec![], Some(Box::new(move || flag.store(true, Ordering::SeqCst)))));
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
        assert!(empty_fired.load(Ordering::SeqCst), "an empty body is read in full when it is answered");
    }

    /// A file truncated after its length was announced is an error, and the
    /// download is never treated as complete (so nothing is cleaned up).
    #[tokio::test]
    async fn a_file_shorter_than_announced_fails_the_body_without_completing_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let path = std::env::temp_dir().join(format!("tg-streamed-short-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, vec![1u8; 1000]).unwrap();
        let file = tokio::fs::File::open(&path).await.unwrap();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        let mut body = Streamed::new(5000, vec![Segment::File(file)], Some(Box::new(move || flag.store(true, Ordering::SeqCst))));
        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_next(cx)).await {
            items.push(item);
        }
        let last = items.pop().unwrap();
        assert_eq!(last.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(items.iter().map(|i| i.as_ref().unwrap().len()).sum::<usize>(), 1000);
        assert!(!fired.load(Ordering::SeqCst), "a truncated download is not complete");
        std::fs::remove_file(&path).unwrap();
    }

    /// The charsets iconv-lite 0.4.24 decodes for body-parser, and the one it lacks.
    #[test]
    fn json_charsets_decode_as_the_installed_iconv_lite_does() {
        // RFC 2152 and RFC 3501 examples.
        assert_eq!(decode_text(b"Hi Mom -+Jjo--!", "utf-7").as_deref(), Some("Hi Mom -\u{263A}-!"));
        assert_eq!(decode_text(b"1 +- 1", "utf-7").as_deref(), Some("1 + 1"));
        assert_eq!(decode_text(b"+AHsAfQ", "utf-7").as_deref(), Some("{}"), "an open run ends at the end of input");
        assert_eq!(decode_text(b"~peter/mail/&U,BTFw-/&ZeVnLIqe-", "utf-7-imap").as_deref(), Some("~peter/mail/\u{53F0}\u{5317}/\u{65E5}\u{672C}\u{8A9E}"));
        assert_eq!(decode_text(b"&-x\xFF", "utf-7-imap").as_deref(), Some("&x\u{FFFD}"));
        let le: Vec<u8> = "{\"a\":1}".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = "{\"a\":1}".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode_text(&le, "utf-16").as_deref(), Some("{\"a\":1}"));
        assert_eq!(decode_text(&be, "utf-16").as_deref(), Some("{\"a\":1}"), "ASCII-shaped units choose big-endian");
        let mut bom_be = vec![0xFE, 0xFF];
        bom_be.extend(&be);
        bom_be.push(0x41);
        assert_eq!(decode_text(&bom_be, "UTF-16").as_deref(), Some("{\"a\":1}"), "BOM chosen and stripped, odd byte dropped");
        assert_eq!(decode_text(&le, "utf-16le").as_deref(), Some("{\"a\":1}"));
        assert_eq!(decode_text("\u{FEFF}{}".as_bytes(), "utf-8").as_deref(), Some("{}"));
        assert_eq!(decode_text(b"{}", "utf-8:2000").as_deref(), Some("{}"), "iconv-lite drops a :NNNN suffix");
        assert_eq!(decode_text(b"{}", "utf-32"), None, "iconv-lite 0.4.24 has no UTF-32");
        assert_eq!(decode_text(b"{}", "utf-9"), None);
    }

    /// One small compressed frame that inflates far past the limit is refused
    /// while decoding; the decoded bytes never exceed the limit.
    #[test]
    fn a_compressed_frame_cannot_expand_past_the_decoded_limit() {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&vec![b' '; 50 * 1024 * 1024]).unwrap();
        let bomb = encoder.finish().unwrap();
        assert!(bomb.len() < 100 * 1024, "one frame of {} bytes", bomb.len());
        let mut decoded = Decoded::for_encoding("gzip").unwrap();
        assert!(decoded.write(&bomb).is_err());
        let Decoded::Gzip(d) = &decoded else { unreachable!() };
        assert!(d.get_ref().0.len() <= JSON_LIMIT);
        let mut at_limit = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        at_limit.write_all(&vec![b' '; JSON_LIMIT]).unwrap();
        let mut decoded = Decoded::for_encoding("deflate").unwrap();
        decoded.write(&at_limit.finish().unwrap()).unwrap();
        assert_eq!(decoded.finish().ok().map(|b| b.len()), Some(JSON_LIMIT), "exactly the limit is accepted");
    }

    #[test]
    fn repeated_and_bracketed_query_keys_are_not_strings() {
        let q = Query::parse(Some("sessionId=a&sessionId=b&part=1&x[y]=2&name=a%20b+c&bad=%zz"));
        assert_eq!(q.text("sessionId"), None);
        assert_eq!(q.get("sessionId"), Some(&QueryValue::Many(vec!["a".into(), "b".into()])));
        assert_eq!(q.text("part"), Some("1"));
        assert_eq!(q.get("x"), Some(&QueryValue::Nested));
        assert_eq!(q.text("name"), Some("a b c"));
        assert_eq!(q.text("bad"), Some("%zz"));
        assert!(!Query::parse(Some("sessionId=")).truthy("sessionId"));
    }
}
