//! Upload intake with formidable 2.1's observable rules (`multiples: false`,
//! `keepExtensions`, `maxFileSize`): a part with a `Content-Type` is a file,
//! the last file named `file` is the upload, every received file byte counts
//! against the limit, and a request with no body has no file. The body is
//! streamed to disk under the family prefix, so a crash mid-upload leaves an
//! orphan that the periodic purge removes.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderMap, header};
use bytes::{Buf, BytesMut};
use http_body_util::BodyExt;
use tokio::io::AsyncWriteExt;

/// Formidable's defaults for non-file fields.
const MAX_FIELDS: usize = 1000;
const MAX_FIELDS_BYTES: u64 = 20 * 1024 * 1024;
const MAX_HEADER_BLOCK: usize = 64 * 1024;

#[derive(Debug)]
pub struct Received {
    pub path: PathBuf,
    pub original_filename: Option<String>,
    pub mime: String,
    pub size: u64,
}

#[derive(Debug)]
pub enum UploadError {
    /// More file bytes than the family accepts.
    TooLarge,
    /// Not readable as an upload; the source answers 500 "Upload failed".
    Failed(String),
}

/// Removes received files unless they were handed over.
struct TempFiles(Vec<PathBuf>);

impl TempFiles {
    fn forget(&mut self, path: &Path) {
        self.0.retain(|p| p != path);
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn temp_path(dir: &Path, prefix: &str) -> PathBuf {
    dir.join(format!("{prefix}incoming_{}", uuid::Uuid::new_v4()))
}

/// Receives the request body. `Ok(None)` when it carries no file.
pub async fn receive(
    headers: &HeaderMap,
    body: Body,
    dir: &Path,
    prefix: &str,
    max_file_bytes: u64,
) -> Result<Option<Received>, UploadError> {
    let content_type = headers.get(header::CONTENT_TYPE).map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    if headers.get(header::CONTENT_LENGTH).is_some_and(|v| v.as_bytes() == b"0") {
        return Ok(None);
    }
    let Some(content_type) = content_type else {
        return Err(UploadError::Failed("bad content-type header, no content-type".into()));
    };
    let lower = content_type.to_ascii_lowercase();
    let mut temps = TempFiles(Vec::new());
    if lower.contains("multipart") {
        let boundary = boundary(&content_type)
            .ok_or_else(|| UploadError::Failed("bad content-type header, no multipart boundary".into()))?;
        let received = Multipart::new(&boundary, dir, prefix, max_file_bytes).run(body, &mut temps).await?;
        if let Some(r) = &received {
            temps.forget(&r.path);
        }
        return Ok(received);
    }
    if lower.contains("octet-stream") {
        // The whole body is the file, named by X-File-Name. The source applied
        // no size limit on this path; the family limit applies here too.
        let path = temp_path(dir, prefix);
        let mut file = tokio::fs::File::create(&path).await.map_err(|e| UploadError::Failed(e.to_string()))?;
        temps.0.push(path.clone());
        let mut size = 0u64;
        let mut body = body;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|e| UploadError::Failed(e.to_string()))?;
            let Ok(data) = frame.into_data() else { continue };
            size += data.len() as u64;
            if size > max_file_bytes {
                return Err(UploadError::TooLarge);
            }
            file.write_all(&data).await.map_err(|e| UploadError::Failed(e.to_string()))?;
        }
        file.flush().await.map_err(|e| UploadError::Failed(e.to_string()))?;
        temps.forget(&path);
        let original_filename = headers.get("x-file-name").map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        return Ok(Some(Received { path, original_filename, mime: content_type, size }));
    }
    if lower.contains("urlencoded") || lower.contains("json") {
        // Fields only: drain and report no file.
        let mut body = body;
        while let Some(frame) = body.frame().await {
            frame.map_err(|e| UploadError::Failed(e.to_string()))?;
        }
        return Ok(None);
    }
    Err(UploadError::Failed("uninitialized parser".into()))
}

/// `/boundary=(?:"([^"]+)"|([^;]+))/i`.
fn boundary(content_type: &str) -> Option<String> {
    let at = content_type.to_ascii_lowercase().find("boundary=")?;
    let rest = &content_type[at + "boundary=".len()..];
    let value = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().filter(|_| quoted.contains('"'))?,
        None => rest.split(';').next()?,
    };
    (!value.is_empty()).then(|| value.to_string())
}

/// `\bname=("([^"]*)"|token)` of a Content-Disposition value.
fn disposition_name(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("name=") {
        let at = from + pos;
        let boundary_before = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric() && lower.as_bytes()[at - 1] != b'_';
        if boundary_before {
            let rest = &value[at + 5..];
            if let Some(quoted) = rest.strip_prefix('"') {
                return quoted.find('"').map(|end| quoted[..end].to_string());
            }
            let token: String =
                rest.chars().take_while(|c| !"()<>@,;:\\\"/[]?={} \t".contains(*c)).collect();
            return (!token.is_empty()).then_some(token);
        }
        from = at + 5;
    }
    None
}

/// Formidable's `_getFileName`: `\bfilename=("(.*?)"|token)($|;\s)`, then
/// everything after the last backslash, `%22` as `"` and `&#NNNN;` decoded.
fn disposition_filename(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("filename=") {
        let at = from + pos;
        from = at + 9;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric() && lower.as_bytes()[at - 1] != b'_';
        if !before_ok {
            continue;
        }
        let rest = &value[at + 9..];
        let ends_ok = |tail: &str| tail.is_empty() || tail.starts_with("; ") || tail.starts_with(";\t");
        let matched = if let Some(quoted) = rest.strip_prefix('"') {
            // Lazy `.*?` followed by `"` and then end or `;\s`.
            let mut found = None;
            for (i, _) in quoted.match_indices('"') {
                if ends_ok(&quoted[i + 1..]) {
                    found = Some(&quoted[..i]);
                    break;
                }
            }
            found
        } else {
            let len = rest.find(|c: char| "()<>{}[]@,;:\"?=/ \t".contains(c)).unwrap_or(rest.len());
            (len > 0 && ends_ok(&rest[len..])).then(|| &rest[..len])
        };
        let Some(matched) = matched else { continue };
        let name = &matched[matched.rfind('\\').map_or(0, |i| i + 1)..];
        let name = name.replace("%22", "\"");
        return Some(decode_numeric_entities(&name));
    }
    None
}

fn decode_numeric_entities(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("&#") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let digits = tail.bytes().take(4).take_while(u8::is_ascii_digit).count();
        if digits == 4 && tail.as_bytes().get(4) == Some(&b';') {
            let code: u32 = tail[..4].parse().unwrap_or(0);
            // String.fromCharCode of one UTF-16 unit; lone surrogates become U+FFFD.
            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            rest = &tail[5..];
        } else {
            out.push_str("&#");
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

struct FileSink {
    file: tokio::fs::File,
    path: PathBuf,
    name: Option<String>,
    filename: Option<String>,
    mime: String,
    size: u64,
}

type Sink = Option<FileSink>;

enum State {
    /// The body must open with `--boundary` followed by CRLF or `--`.
    Start,
    Headers,
    Body,
    End,
}

struct Multipart<'a> {
    dash_boundary: Vec<u8>,
    delimiter: Vec<u8>,
    dir: &'a Path,
    prefix: &'a str,
    max_file_bytes: u64,
    file_bytes: u64,
    field_bytes: u64,
    fields: usize,
    upload: Option<Received>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn failed(e: impl std::fmt::Display) -> UploadError {
    UploadError::Failed(e.to_string())
}

impl<'a> Multipart<'a> {
    fn new(boundary: &str, dir: &'a Path, prefix: &'a str, max_file_bytes: u64) -> Self {
        let dash_boundary = [b"--", boundary.as_bytes()].concat();
        let delimiter = [b"\r\n", dash_boundary.as_slice()].concat();
        Multipart {
            dash_boundary,
            delimiter,
            dir,
            prefix,
            max_file_bytes,
            file_bytes: 0,
            field_bytes: 0,
            fields: 0,
            upload: None,
        }
    }

    async fn run(mut self, mut body: Body, temps: &mut TempFiles) -> Result<Option<Received>, UploadError> {
        let mut buf = BytesMut::new();
        let mut state = State::Start;
        let mut sink = None;
        let mut done = false;
        loop {
            let progressed = self.step(&mut buf, &mut state, &mut sink, temps).await?;
            if matches!(state, State::End) {
                if let Some(upload) = &self.upload {
                    temps.forget(&upload.path);
                }
                return Ok(self.upload);
            }
            if progressed {
                continue;
            }
            if done {
                return Err(failed("MultipartParser.end(): stream ended unexpectedly"));
            }
            match body.frame().await {
                Some(frame) => {
                    if let Ok(data) = frame.map_err(failed)?.into_data() {
                        buf.extend_from_slice(&data);
                    }
                }
                None => done = true,
            }
        }
    }

    /// Consumes what it can from `buf`; `false` when it needs more input.
    ///
    /// As in formidable's parser, `CRLF--boundary` ends a part only when CRLF
    /// or `--` follows it; anything else after it is part data.
    async fn step(
        &mut self,
        buf: &mut BytesMut,
        state: &mut State,
        sink: &mut Sink,
        temps: &mut TempFiles,
    ) -> Result<bool, UploadError> {
        match state {
            State::Start => {
                let dl = self.dash_boundary.len();
                let seen = buf.len().min(dl);
                if buf[..seen] != self.dash_boundary[..seen] {
                    return Err(failed("MultipartParser: the body does not start with its boundary"));
                }
                if buf.len() < dl + 2 {
                    return Ok(false);
                }
                *state = match &buf[dl..dl + 2] {
                    b"\r\n" => State::Headers,
                    b"--" => State::End,
                    _ => return Err(failed("MultipartParser: malformed first boundary")),
                };
                buf.advance(dl + 2);
                Ok(true)
            }
            State::Headers => {
                let (block_len, skip) = if buf.starts_with(b"\r\n") {
                    (0, 2)
                } else {
                    match find(buf, b"\r\n\r\n") {
                        Some(at) => (at, at + 4),
                        None if buf.len() > MAX_HEADER_BLOCK => return Err(failed("part headers too large")),
                        None => return Ok(false),
                    }
                };
                let block = String::from_utf8_lossy(&buf[..block_len]).into_owned();
                buf.advance(skip);
                let (mut name, mut filename, mut mime) = (None, None, None);
                for line in block.split("\r\n") {
                    let Some((field, value)) = line.split_once(':') else { continue };
                    let value = value.trim_start_matches(' ');
                    match field.to_ascii_lowercase().as_str() {
                        "content-disposition" => {
                            name = disposition_name(value);
                            filename = disposition_filename(value);
                        }
                        "content-type" => mime = Some(value.to_string()),
                        _ => {}
                    }
                }
                *sink = match mime.filter(|m| !m.is_empty()) {
                    Some(mime) => {
                        let path = temp_path(self.dir, self.prefix);
                        let file = tokio::fs::File::create(&path).await.map_err(failed)?;
                        temps.0.push(path.clone());
                        Some(FileSink { file, path, name, filename, mime, size: 0 })
                    }
                    None => None,
                };
                *state = State::Body;
                Ok(true)
            }
            State::Body => {
                let dl = self.delimiter.len();
                // Everything before `from` is known to be part data.
                let mut from = 0;
                let (data_len, next) = loop {
                    match find(&buf[from..], &self.delimiter) {
                        None => break (buf.len().saturating_sub(dl - 1).max(from), None),
                        Some(rel) => {
                            let at = from + rel;
                            if buf.len() < at + dl + 2 {
                                // Undecided until the two bytes after it arrive.
                                break (at, None);
                            }
                            match &buf[at + dl..at + dl + 2] {
                                b"\r\n" => break (at, Some(State::Headers)),
                                b"--" => break (at, Some(State::End)),
                                _ => from = at + 1,
                            }
                        }
                    }
                };
                if data_len > 0 {
                    let data = buf.split_to(data_len);
                    self.write(sink, &data).await?;
                }
                let Some(next) = next else { return Ok(false) };
                buf.advance(dl + 2);
                *state = next;
                self.finish(sink.take(), temps).await?;
                Ok(true)
            }
            State::End => Ok(false),
        }
    }

    async fn write(&mut self, sink: &mut Sink, data: &[u8]) -> Result<(), UploadError> {
        match sink {
            Some(FileSink { file, size, .. }) => {
                self.file_bytes += data.len() as u64;
                if self.file_bytes > self.max_file_bytes {
                    return Err(UploadError::TooLarge);
                }
                *size += data.len() as u64;
                file.write_all(data).await.map_err(failed)
            }
            None => {
                self.field_bytes += data.len() as u64;
                if self.field_bytes > MAX_FIELDS_BYTES {
                    return Err(failed("options.maxFieldsSize exceeded"));
                }
                Ok(())
            }
        }
    }

    async fn finish(&mut self, sink: Sink, temps: &mut TempFiles) -> Result<(), UploadError> {
        match sink {
            None => {
                self.fields += 1;
                if self.fields > MAX_FIELDS {
                    return Err(failed("options.maxFields exceeded"));
                }
            }
            Some(FileSink { mut file, path, name, filename, mime, size }) => {
                file.flush().await.map_err(failed)?;
                drop(file);
                if name.as_deref() == Some("file") {
                    // `multiples: false`: a later `file` replaces an earlier one.
                    if let Some(previous) = self.upload.take() {
                        let _ = tokio::fs::remove_file(&previous.path).await;
                        temps.forget(&previous.path);
                    }
                    self.upload = Some(Received { path, original_filename: filename, mime, size });
                } else {
                    let _ = tokio::fs::remove_file(&path).await;
                    temps.forget(&path);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(content_type: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        h
    }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("tg-upload-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Splits the body into tiny frames so boundaries straddle chunks.
    fn chunked(bytes: Vec<u8>, size: usize) -> Body {
        let frames: Vec<Result<bytes::Bytes, std::io::Error>> =
            bytes.chunks(size).map(|c| Ok(bytes::Bytes::copy_from_slice(c))).collect();
        Body::from_stream(futures_stream(frames))
    }

    fn futures_stream(
        items: Vec<Result<bytes::Bytes, std::io::Error>>,
    ) -> impl futures_core::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static {
        struct Iter(std::vec::IntoIter<Result<bytes::Bytes, std::io::Error>>);
        impl futures_core::Stream for Iter {
            type Item = Result<bytes::Bytes, std::io::Error>;
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                std::task::Poll::Ready(self.0.next())
            }
        }
        Iter(items.into_iter())
    }

    type FormPart<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a [u8]);

    fn form(parts: &[FormPart<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, filename, mime, data) in parts {
            out.extend_from_slice(b"--XyZ\r\n");
            let mut disposition = format!("Content-Disposition: form-data; name=\"{name}\"");
            if let Some(f) = filename {
                disposition.push_str(&format!("; filename=\"{f}\""));
            }
            out.extend_from_slice(disposition.as_bytes());
            out.extend_from_slice(b"\r\n");
            if let Some(m) = mime {
                out.extend_from_slice(format!("Content-Type: {m}\r\n").as_bytes());
            }
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"--XyZ--\r\n");
        out
    }

    #[tokio::test]
    async fn the_last_file_part_named_file_is_the_upload_and_nothing_else_stays_on_disk() {
        let d = dir();
        let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let body = form(&[
            ("note", None, None, b"a field"),
            ("file", Some("first.mp3"), Some("audio/mpeg"), b"old"),
            ("other", Some("x.bin"), Some("application/octet-stream"), b"ignored"),
            ("file", Some("C:\\Users\\me\\My Song.mp3"), Some("audio/mpeg"), &payload),
        ]);
        for chunk in [1, 7, 64, 100_000] {
            let got = receive(&headers("multipart/form-data; boundary=XyZ"), chunked(body.clone(), chunk), &d, "trim_", 1 << 20)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(got.original_filename.as_deref(), Some("My Song.mp3"));
            assert_eq!(got.mime, "audio/mpeg");
            assert_eq!(got.size, 5000);
            assert_eq!(std::fs::read(&got.path).unwrap(), payload, "chunk size {chunk}");
            assert!(got.path.file_name().unwrap().to_str().unwrap().starts_with("trim_"));
            let left: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().path()).collect();
            assert_eq!(left, vec![got.path.clone()]);
            std::fs::remove_file(&got.path).unwrap();
        }
        std::fs::remove_dir(&d).unwrap();
    }

    #[tokio::test]
    async fn file_bytes_beyond_the_limit_fail_and_leave_no_file() {
        let d = dir();
        let body = form(&[("file", Some("a.wav"), Some("audio/wav"), &[0u8; 101])]);
        let at_limit = form(&[("file", Some("a.wav"), Some("audio/wav"), &[0u8; 100])]);
        let h = headers("multipart/form-data; boundary=\"XyZ\"");
        assert!(matches!(receive(&h, chunked(body, 13), &d, "conv_", 100).await, Err(UploadError::TooLarge)));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let ok = receive(&h, chunked(at_limit, 13), &d, "conv_", 100).await.unwrap().unwrap();
        assert_eq!(ok.size, 100);
        std::fs::remove_file(ok.path).unwrap();
        std::fs::remove_dir(&d).unwrap();
    }

    #[tokio::test]
    async fn no_file_part_truncated_bodies_and_unknown_types_are_told_apart() {
        let d = dir();
        let fields = form(&[("file", Some("a.mp3"), None, b"no content type means a field")]);
        let h = headers("multipart/form-data; boundary=XyZ");
        assert!(receive(&h, chunked(fields, 5), &d, "x_", 1000).await.unwrap().is_none());
        let mut truncated = form(&[("file", Some("a.mp3"), Some("audio/mpeg"), b"abc")]);
        truncated.truncate(truncated.len() - 12);
        assert!(matches!(receive(&h, chunked(truncated, 5), &d, "x_", 1000).await, Err(UploadError::Failed(_))));
        assert!(matches!(
            receive(&headers("multipart/form-data"), chunked(b"x".to_vec(), 1), &d, "x_", 1000).await,
            Err(UploadError::Failed(_))
        ));
        assert!(matches!(
            receive(&headers("text/plain"), chunked(b"x".to_vec(), 1), &d, "x_", 1000).await,
            Err(UploadError::Failed(_))
        ));
        assert!(receive(&headers("application/json"), chunked(b"{}".to_vec(), 1), &d, "x_", 1000).await.unwrap().is_none());
        let mut empty = headers("multipart/form-data; boundary=XyZ");
        empty.insert(header::CONTENT_LENGTH, "0".parse().unwrap());
        assert!(receive(&empty, Body::empty(), &d, "x_", 1000).await.unwrap().is_none());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        std::fs::remove_dir(&d).unwrap();
    }

    #[tokio::test]
    async fn delimiter_lookalikes_inside_file_bytes_are_payload_wherever_the_chunks_split() {
        let d = dir();
        // CRLF--XyZ followed by anything but CRLF or "--" is data, as in formidable.
        let payload = b"head\r\n--XyZX middle \r\n--XyZ\t\r\n--XyZ-x\r\n--Xy tail\r\n".to_vec();
        let body = form(&[("file", Some("a.wav"), Some("audio/wav"), &payload), ("after", None, None, b"field")]);
        let h = headers("multipart/form-data; boundary=XyZ");
        for chunk in 1..=12 {
            let got = receive(&h, chunked(body.clone(), chunk), &d, "x_", 1 << 20).await.unwrap().unwrap();
            assert_eq!(std::fs::read(&got.path).unwrap(), payload, "chunks of {chunk}");
            std::fs::remove_file(&got.path).unwrap();
        }
        let mut preamble = b"junk\r\n".to_vec();
        preamble.extend(form(&[("file", Some("a.wav"), Some("audio/wav"), b"x")]));
        assert!(matches!(receive(&h, chunked(preamble, 4), &d, "x_", 1000).await, Err(UploadError::Failed(_))));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        std::fs::remove_dir(&d).unwrap();
    }

    #[test]
    fn disposition_parameters_follow_formidable() {
        let v = r#"form-data; name="file"; filename="a&#0233;b%22.mp3""#;
        assert_eq!(disposition_name(v).as_deref(), Some("file"));
        assert_eq!(disposition_filename(v).as_deref(), Some("aéb\".mp3"));
        assert_eq!(disposition_filename("form-data; name=file; filename=plain.wav").as_deref(), Some("plain.wav"));
        assert_eq!(disposition_filename("form-data; name=\"file\"").as_deref(), None);
        assert_eq!(disposition_filename("form-data; filename=\"\"").as_deref(), Some(""));
        assert_eq!(boundary("multipart/form-data; boundary=abc; charset=x").as_deref(), Some("abc"));
        assert_eq!(boundary("multipart/form-data; BOUNDARY=\"q q\"").as_deref(), Some("q q"));
    }
}
