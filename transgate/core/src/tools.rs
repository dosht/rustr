//! Free-tool policy for the seven server-first processor families (P3 D1).
//!
//! Captured from `frontend/services/free-tools-processor/src/routes/*.ts` and
//! `src/lib/{sessionId,sessionStore}.ts` (API-022..API-030, RTL-006..RTL-008,
//! CRN-008/CRN-009). These are the product decisions: which uploads a tool
//! accepts, how long a session lives, how many processing requests an address
//! may make, which options are valid and which encoder settings they mean, and
//! how FFmpeg progress becomes a percentage. Pure: no filesystem, subprocess
//! or network. `transgate-processor` runs FFmpeg and renders these over HTTP.

use std::collections::HashMap;
use std::time::{Duration, Instant};

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const GIB: u64 = 1024 * 1024 * 1024;

/// The processor calls every family's session and orphan purge this often.
pub const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * MINUTE);
/// A processing allowance window: fixed, starting at an address's first request.
pub const ALLOWANCE_WINDOW: Duration = Duration::from_secs(HOUR);

/// One mounted tool router.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    TrimAudio,
    ConvertAudio,
    ReduceAudio,
    RemoveNoise,
    VideoToAudio,
    CompressVideo,
    ConvertVideo,
}

impl Family {
    /// In the order the source cleanup interval calls them (`index.ts:106-121`).
    pub const ALL: [Family; 7] = [
        Family::TrimAudio,
        Family::ConvertAudio,
        Family::ConvertVideo,
        Family::ReduceAudio,
        Family::CompressVideo,
        Family::VideoToAudio,
        Family::RemoveNoise,
    ];

    /// Mount path segment, without the leading slash.
    pub fn mount(self) -> &'static str {
        match self {
            Family::TrimAudio => "trim-audio",
            Family::ConvertAudio => "convert-audio",
            Family::ReduceAudio => "reduce-audio",
            Family::RemoveNoise => "remove-noise",
            Family::VideoToAudio => "video-to-audio",
            Family::CompressVideo => "compress-video",
            Family::ConvertVideo => "convert-video",
        }
    }

    /// Every temporary file of this family starts with this; orphan purge keys off it.
    pub fn file_prefix(self) -> &'static str {
        match self {
            Family::TrimAudio => "trim_",
            Family::ConvertAudio => "conv_",
            Family::ReduceAudio => "redaud_",
            Family::RemoveNoise => "rmnoise_",
            Family::VideoToAudio => "v2audio_",
            Family::CompressVideo => "compvid_",
            Family::ConvertVideo => "convvideo_",
        }
    }

    /// A session older than this (by creation) and a file older than this (by
    /// modification) are purged.
    pub fn session_ttl(self) -> Duration {
        match self {
            Family::TrimAudio | Family::ConvertAudio | Family::ConvertVideo => Duration::from_secs(HOUR),
            Family::ReduceAudio | Family::RemoveNoise | Family::VideoToAudio | Family::CompressVideo => {
                Duration::from_secs(2 * HOUR)
            }
        }
    }

    /// Largest accepted upload, in bytes; exactly this size is still accepted.
    pub fn max_upload_bytes(self) -> u64 {
        match self {
            Family::TrimAudio => GIB,
            _ => 2 * GIB,
        }
    }

    /// Processing requests per address per allowance window.
    pub fn processing_per_hour(self) -> u32 {
        match self {
            Family::CompressVideo | Family::ConvertVideo => 10,
            _ => 20,
        }
    }

    /// Hard SIGKILL ceiling on the processing FFmpeg child, where the source has one.
    pub fn processing_kill_after(self) -> Option<Duration> {
        match self {
            Family::RemoveNoise | Family::VideoToAudio | Family::ConvertVideo => Some(Duration::from_secs(10 * MINUTE)),
            Family::CompressVideo => Some(Duration::from_secs(15 * MINUTE)),
            Family::TrimAudio | Family::ConvertAudio | Family::ReduceAudio => None,
        }
    }

    /// Message for an upload this family refuses.
    pub fn upload_refusal(self, refusal: UploadRefusal) -> &'static str {
        match (self, refusal) {
            (Family::TrimAudio, UploadRefusal::Unsupported) => {
                "Unsupported format. Please upload an MP3, MP4, WAV, M4A, or AAC file."
            }
            (Family::TrimAudio, UploadRefusal::TooLarge) => "This file exceeds the 1GB limit. Try compressing it first.",
            (_, UploadRefusal::TooLarge) => "This file exceeds the 2GB limit.",
            (Family::ConvertAudio | Family::ReduceAudio, UploadRefusal::Unsupported) => {
                "This format is not supported. Please upload a valid audio file."
            }
            (Family::RemoveNoise, UploadRefusal::Unsupported) => {
                "This format is not supported. Please upload a valid audio or video file."
            }
            (Family::VideoToAudio | Family::CompressVideo | Family::ConvertVideo, UploadRefusal::Unsupported) => {
                "This format isn't supported. Please upload an MP4, MOV, AVI, or MKV file."
            }
        }
    }

    /// Decides whether an uploaded file is accepted, in the source's check
    /// order, and names it. `size` is the received byte count.
    pub fn accept_upload(self, file: &UploadedFile<'_>) -> Result<AcceptedUpload, UploadRefusal> {
        let ext = extension_of(file.original_filename.unwrap_or(""));
        let table = self.upload_table();
        let ext_ok = !ext.is_empty() && table.extensions.contains(&ext.as_str());
        let mime_ok = table.mimes.contains(&file.mime);
        let too_large = file.size > self.max_upload_bytes();
        let canonical = match self {
            Family::TrimAudio | Family::ConvertAudio | Family::ReduceAudio | Family::RemoveNoise => {
                if !mime_ok && !ext_ok {
                    return Err(UploadRefusal::Unsupported);
                }
                if too_large {
                    return Err(UploadRefusal::TooLarge);
                }
                let canonical = if ext_ok { ext } else { exact_mime_ext(table, file.mime).to_string() };
                if canonical.is_empty() || !table.extensions.contains(&canonical.as_str()) {
                    return Err(UploadRefusal::Unsupported);
                }
                canonical
            }
            Family::VideoToAudio | Family::ConvertVideo => {
                if !mime_ok && !ext_ok {
                    return Err(UploadRefusal::Unsupported);
                }
                let canonical = if ext_ok {
                    ext
                } else if self == Family::VideoToAudio {
                    fuzzy_video_mime_ext(file.mime).to_string()
                } else {
                    exact_mime_ext(table, file.mime).to_string()
                };
                if !table.extensions.contains(&canonical.as_str()) {
                    return Err(UploadRefusal::Unsupported);
                }
                if too_large {
                    return Err(UploadRefusal::TooLarge);
                }
                canonical
            }
            Family::CompressVideo => {
                if !ext_ok {
                    return Err(UploadRefusal::Unsupported);
                }
                if too_large {
                    return Err(UploadRefusal::TooLarge);
                }
                ext
            }
        };
        let default_name = match self {
            Family::TrimAudio | Family::ConvertAudio | Family::ReduceAudio => "audio",
            Family::RemoveNoise => "media",
            Family::VideoToAudio | Family::CompressVideo | Family::ConvertVideo => "video",
        };
        Ok(AcceptedUpload { extension: canonical, safe_name: safe_name(file.original_filename, default_name) })
    }

    /// `Content-Type` of a download with this extension.
    pub fn content_type(self, ext: &str) -> &'static str {
        let table: &[(&str, &str)] = match self {
            Family::TrimAudio => {
                &[("mp3", "audio/mpeg"), ("wav", "audio/wav"), ("m4a", "audio/mp4"), ("aac", "audio/aac"), ("mp4", "video/mp4")]
            }
            Family::ConvertAudio | Family::ReduceAudio => &AUDIO_CONTENT_TYPES,
            Family::RemoveNoise => &[
                ("mp3", "audio/mpeg"),
                ("wav", "audio/wav"),
                ("m4a", "audio/mp4"),
                ("aac", "audio/aac"),
                ("ogg", "audio/ogg"),
                ("flac", "audio/flac"),
                ("opus", "audio/opus"),
                ("mp4", "video/mp4"),
                ("mov", "video/quicktime"),
                ("avi", "video/x-msvideo"),
                ("wmv", "video/x-ms-wmv"),
                ("webm", "video/webm"),
                ("mkv", "video/x-matroska"),
            ],
            Family::VideoToAudio => &[
                ("mp3", "audio/mpeg"),
                ("aac", "audio/aac"),
                ("wav", "audio/wav"),
                ("ogg", "audio/ogg"),
                ("opus", "audio/opus"),
            ],
            Family::CompressVideo | Family::ConvertVideo => &VIDEO_CONTENT_TYPES,
        };
        table.iter().find(|(e, _)| *e == ext).map_or("application/octet-stream", |(_, t)| t)
    }

    fn upload_table(self) -> &'static UploadTable {
        match self {
            Family::TrimAudio => &TRIM_UPLOADS,
            Family::ConvertAudio | Family::ReduceAudio => &AUDIO_UPLOADS,
            Family::RemoveNoise => &NOISE_UPLOADS,
            Family::VideoToAudio | Family::ConvertVideo => &VIDEO_UPLOADS,
            Family::CompressVideo => &COMPRESS_UPLOADS,
        }
    }
}

const AUDIO_CONTENT_TYPES: [(&str, &str); 8] = [
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("ogg", "audio/ogg"),
    ("flac", "audio/flac"),
    ("opus", "audio/opus"),
    ("wma", "audio/x-ms-wma"),
];

const VIDEO_CONTENT_TYPES: [(&str, &str); 4] =
    [("mp4", "video/mp4"), ("mov", "video/quicktime"), ("avi", "video/x-msvideo"), ("mkv", "video/x-matroska")];

struct UploadTable {
    mimes: &'static [&'static str],
    extensions: &'static [&'static str],
    mime_ext: &'static [(&'static str, &'static str)],
}

const TRIM_UPLOADS: UploadTable = UploadTable {
    mimes: &[
        "audio/mpeg", "audio/mp3", "audio/wav", "audio/x-wav", "audio/wave", "audio/m4a", "audio/x-m4a", "audio/aac",
        "audio/x-aac", "video/mp4", "audio/mp4", "video/quicktime", "video/x-msvideo", "video/avi", "video/x-ms-wmv",
        "video/wmv",
    ],
    extensions: &["mp3", "wav", "m4a", "aac", "mp4", "mov", "avi", "wmv"],
    mime_ext: &[
        ("audio/mpeg", "mp3"),
        ("audio/mp3", "mp3"),
        ("audio/wav", "wav"),
        ("audio/x-wav", "wav"),
        ("audio/wave", "wav"),
        ("audio/m4a", "m4a"),
        ("audio/x-m4a", "m4a"),
        ("audio/aac", "aac"),
        ("audio/x-aac", "aac"),
        ("video/mp4", "mp4"),
        ("audio/mp4", "mp4"),
        ("video/quicktime", "mov"),
        ("video/x-msvideo", "avi"),
        ("video/avi", "avi"),
        ("video/x-ms-wmv", "wmv"),
        ("video/wmv", "wmv"),
    ],
};

const AUDIO_UPLOADS: UploadTable = UploadTable {
    mimes: &[
        "audio/mpeg", "audio/mp3", "audio/wav", "audio/x-wav", "audio/wave", "audio/m4a", "audio/x-m4a", "audio/aac",
        "audio/x-aac", "audio/mp4", "video/mp4", "audio/ogg", "video/ogg", "audio/flac", "audio/x-flac", "audio/opus",
        "audio/x-ms-wma", "audio/wma",
    ],
    extensions: &["mp3", "wav", "m4a", "aac", "ogg", "flac", "opus", "wma"],
    mime_ext: &[
        ("audio/mpeg", "mp3"),
        ("audio/mp3", "mp3"),
        ("audio/wav", "wav"),
        ("audio/x-wav", "wav"),
        ("audio/wave", "wav"),
        ("audio/m4a", "m4a"),
        ("audio/x-m4a", "m4a"),
        ("audio/aac", "aac"),
        ("audio/x-aac", "aac"),
        ("audio/mp4", "m4a"),
        ("video/mp4", "m4a"),
        ("audio/ogg", "ogg"),
        ("video/ogg", "ogg"),
        ("audio/flac", "flac"),
        ("audio/x-flac", "flac"),
        ("audio/opus", "opus"),
        ("audio/x-ms-wma", "wma"),
        ("audio/wma", "wma"),
    ],
};

const NOISE_UPLOADS: UploadTable = UploadTable {
    mimes: &[
        "audio/mpeg", "audio/mp3", "audio/wav", "audio/x-wav", "audio/wave", "audio/m4a", "audio/x-m4a", "audio/aac",
        "audio/x-aac", "audio/mp4", "audio/ogg", "audio/flac", "audio/x-flac", "audio/opus", "video/mp4",
        "video/quicktime", "video/x-msvideo", "video/x-ms-wmv", "video/webm", "video/x-matroska", "video/ogg",
    ],
    extensions: &["mp3", "wav", "m4a", "aac", "ogg", "flac", "opus", "mp4", "mov", "avi", "wmv", "webm", "mkv"],
    mime_ext: &[
        ("audio/mpeg", "mp3"),
        ("audio/mp3", "mp3"),
        ("audio/wav", "wav"),
        ("audio/x-wav", "wav"),
        ("audio/wave", "wav"),
        ("audio/m4a", "m4a"),
        ("audio/x-m4a", "m4a"),
        ("audio/mp4", "m4a"),
        ("audio/aac", "aac"),
        ("audio/x-aac", "aac"),
        ("audio/ogg", "ogg"),
        ("audio/flac", "flac"),
        ("audio/x-flac", "flac"),
        ("audio/opus", "opus"),
        ("video/mp4", "mp4"),
        ("video/quicktime", "mov"),
        ("video/x-msvideo", "avi"),
        ("video/x-ms-wmv", "wmv"),
        ("video/webm", "webm"),
        ("video/x-matroska", "mkv"),
        ("video/ogg", "ogg"),
    ],
};

const VIDEO_UPLOADS: UploadTable = UploadTable {
    mimes: &["video/mp4", "video/quicktime", "video/x-msvideo", "video/x-matroska", "video/mkv", "video/avi", "video/mov"],
    extensions: &["mp4", "mov", "avi", "mkv"],
    mime_ext: &[
        ("video/mp4", "mp4"),
        ("video/quicktime", "mov"),
        ("video/x-msvideo", "avi"),
        ("video/x-matroska", "mkv"),
        ("video/mkv", "mkv"),
        ("video/avi", "avi"),
        ("video/mov", "mov"),
    ],
};

/// Compression checks the extension only; its MIME list is not consulted (`compress-video.ts:611`).
const COMPRESS_UPLOADS: UploadTable = UploadTable { mimes: &[], extensions: &["mp4", "mov", "avi", "mkv"], mime_ext: &[] };

fn exact_mime_ext(table: &UploadTable, mime: &str) -> &'static str {
    table.mime_ext.iter().find(|(m, _)| *m == mime).map_or("", |(_, e)| e)
}

/// `video-to-audio.ts:143-149`: substring matching, in this order.
fn fuzzy_video_mime_ext(mime: &str) -> &'static str {
    if mime.contains("mp4") {
        "mp4"
    } else if mime.contains("quicktime") {
        "mov"
    } else if mime.contains("x-msvideo") || mime.contains("avi") {
        "avi"
    } else if mime.contains("matroska") || mime.contains("mkv") {
        "mkv"
    } else {
        ""
    }
}

/// What the upload parser received for the `file` field.
#[derive(Clone, Copy, Debug)]
pub struct UploadedFile<'a> {
    /// The part's filename, `None` when it had none.
    pub original_filename: Option<&'a str>,
    /// The part's `Content-Type`, verbatim.
    pub mime: &'a str,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedUpload {
    /// Lower-case extension without the dot; one of the family's extensions.
    pub extension: String,
    /// Filesystem- and header-safe base name, at most 80 ASCII characters.
    pub safe_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadRefusal {
    Unsupported,
    TooLarge,
}

/// `path.extname(name).toLowerCase().replace('.', '')`.
pub fn extension_of(filename: &str) -> String {
    node_extname(filename).to_lowercase().replacen('.', "", 1)
}

/// The base name the source derives from an upload: `path.basename(name,
/// path.extname(name))` with every UTF-16 unit outside `[A-Za-z0-9._-]`
/// replaced by `_`, cut to 80.
pub fn safe_name(original_filename: Option<&str>, default_name: &str) -> String {
    let name = original_filename.filter(|n| !n.is_empty()).unwrap_or(default_name);
    let ext = node_extname(name);
    let base = node_basename(name);
    let base = match base.strip_suffix(ext) {
        Some(stripped) if !ext.is_empty() && !stripped.is_empty() => stripped,
        _ => base,
    };
    let mut safe = String::new();
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            safe.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                safe.push('_');
            }
        }
    }
    safe.truncate(80);
    safe
}

/// The last path segment of a POSIX path, ignoring trailing slashes.
fn node_basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// Node's `path.posix.extname`.
fn node_extname(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut start_dot: Option<usize> = None;
    let mut start_part = 0;
    let mut end: Option<usize> = None;
    let mut matched_slash = true;
    // 0: no non-dot character seen before the last dot; 1: only dots; -1: other.
    let mut pre_dot_state = 0;
    for i in (0..bytes.len()).rev() {
        let c = bytes[i];
        if c == b'/' {
            if !matched_slash {
                start_part = i + 1;
                break;
            }
            continue;
        }
        if end.is_none() {
            matched_slash = false;
            end = Some(i + 1);
        }
        if c == b'.' {
            if start_dot.is_none() {
                start_dot = Some(i);
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot.is_some() {
            pre_dot_state = -1;
        }
    }
    match (start_dot, end) {
        (Some(dot), Some(end))
            if pre_dot_state != 0 && !(pre_dot_state == 1 && dot == end - 1 && dot == start_part + 1) =>
        {
            &path[dot..end]
        }
        _ => "",
    }
}

/// Session ids are server-generated UUID v4s; anything else never reaches a path.
pub fn is_valid_session_id(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    let hex = |c: u8| c.is_ascii_hexdigit();
    b.iter().enumerate().all(|(i, &c)| match i {
        8 | 13 | 18 | 23 => c == b'-',
        14 => c == b'4',
        19 => matches!(c.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'),
        _ => hex(c),
    })
}

/// Per-address processing allowance (`sessionStore.ts:51-70`). A window starts
/// at an address's first request and resets on the first request more than
/// [`ALLOWANCE_WINDOW`] later. Accepted checks count; refused checks do not.
/// Process-local, like the source (API-D10).
#[derive(Debug)]
pub struct ProcessingAllowance {
    per_window: u32,
    hits: HashMap<String, (u32, Instant)>,
}

impl ProcessingAllowance {
    pub fn new(per_window: u32) -> Self {
        ProcessingAllowance { per_window, hits: HashMap::new() }
    }

    pub fn check(&mut self, key: &str, now: Instant) -> bool {
        let per_window = self.per_window;
        if let Some((count, start)) = self.hits.get_mut(key)
            && now.saturating_duration_since(*start) <= ALLOWANCE_WINDOW
        {
            if *count >= per_window {
                return false;
            }
            *count += 1;
            return true;
        }
        self.hits.insert(key.to_string(), (1, now));
        true
    }

    /// Drops windows that the next check would reset anyway.
    pub fn forget_expired(&mut self, now: Instant) {
        self.hits.retain(|_, (_, start)| now.saturating_duration_since(*start) <= ALLOWANCE_WINDOW);
    }
}

/// A JSON request value as the source's `typeof`/truthiness checks see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg<'a> {
    Absent,
    Null,
    Bool(bool),
    Number(f64),
    Text(&'a str),
    /// An array or object.
    Composite,
}

impl Arg<'_> {
    pub fn truthy(&self) -> bool {
        match *self {
            Arg::Absent | Arg::Null => false,
            Arg::Bool(b) => b,
            Arg::Number(n) => n != 0.0 && !n.is_nan(),
            Arg::Text(s) => !s.is_empty(),
            Arg::Composite => true,
        }
    }

    fn number(&self) -> Option<f64> {
        match *self {
            Arg::Number(n) => Some(n),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match *self {
            Arg::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Why a request is refused; the processor maps each to its HTTP status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// 400
    Invalid,
    /// 410
    Expired,
    /// 409
    Busy,
    /// 429
    Limited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub kind: RefusalKind,
    pub message: &'static str,
}

const fn invalid(message: &'static str) -> Refusal {
    Refusal { kind: RefusalKind::Invalid, message }
}

/// The 400 every family except trim answers for a malformed session id.
pub const BAD_SESSION_ID: Refusal = invalid("Invalid or missing sessionId");

/// The processing allowance refusal. Trim and convert-audio say "a moment".
pub fn allowance_refusal(family: Family) -> Refusal {
    let message = match family {
        Family::TrimAudio | Family::ConvertAudio => "Too many requests. Please wait a moment before trying again.",
        _ => "Too many requests. Please wait before trying again.",
    };
    Refusal { kind: RefusalKind::Limited, message }
}

/// The expired-session refusal of a processing request.
pub fn processing_expired(family: Family) -> Refusal {
    let message = match family {
        Family::TrimAudio | Family::ConvertAudio => "Your session has expired. Please upload your file again.",
        _ => "Session expired. Please upload your file again.",
    };
    Refusal { kind: RefusalKind::Expired, message }
}

/// The refusal while that session is already processing.
pub fn already_processing(family: Family) -> Refusal {
    let message = match family {
        Family::TrimAudio => "A trim is already in progress for this file.",
        Family::ReduceAudio => "A reduction is already in progress for this file.",
        Family::RemoveNoise => "Noise removal already in progress.",
        Family::VideoToAudio => "Extraction already in progress.",
        Family::CompressVideo => "Compression already in progress.",
        Family::ConvertVideo => "Conversion already in progress.",
        // Audio conversion runs inside its request and has no busy state.
        Family::ConvertAudio => "Conversion already in progress.",
    };
    Refusal { kind: RefusalKind::Busy, message }
}

/// JavaScript `Math.round`: halves go towards positive infinity.
pub fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

// ---------------------------------------------------------------------------
// Trim
// ---------------------------------------------------------------------------

/// A stream copy can only start on a keyframe: a cut further than this from
/// one is not representable by copying. Half a frame at 25 fps.
pub const KEYFRAME_EPSILON_SECONDS: f64 = 0.02;

/// `POST /trim-audio/trim` shape check, before the allowance is charged.
pub fn trim_request(session_id: Arg<'_>, trim_point: Arg<'_>) -> Result<f64, Refusal> {
    match trim_point.number() {
        Some(point) if session_id.truthy() => Ok(point),
        _ => Err(invalid("sessionId and trimPoint are required")),
    }
}

/// The cut must be strictly inside the file. A duration ffprobe could not
/// read (NaN) refuses nothing, as in the source.
pub fn trim_point_in_range(trim_point: f64, total_duration: f64) -> Result<(), Refusal> {
    if trim_point <= 0.0 || trim_point >= total_duration {
        return Err(invalid("Please choose a trim point between the start and end of the file."));
    }
    Ok(())
}

/// Extensions trimmed as video (keyframe-aware) rather than as one audio stream.
pub fn trim_as_video(ext: &str) -> bool {
    matches!(ext, "mp4" | "mov" | "avi" | "wmv")
}

/// Audio trim codec: stream copy when the probed codec fits the container.
pub fn trim_audio_codec(probed_codec: &str, ext: &str) -> &'static str {
    let compatible: &[&str] = match probed_codec {
        "mp3" => &["mp3"],
        "aac" => &["aac", "m4a", "mp4", "mov"],
        "h264" => &["mp4", "mov", "avi"],
        "hevc" => &["mp4", "mov"],
        "mpeg4" => &["mp4", "mov", "avi"],
        "msmpeg4v3" => &["avi", "wmv"],
        "wmv2" | "wmv3" => &["wmv"],
        "pcm_s16le" => &["wav", "avi"],
        "pcm_s24le" | "pcm_f32le" => &["wav"],
        _ => &[],
    };
    if compatible.contains(&ext) {
        return "copy";
    }
    match ext {
        "mp3" => "libmp3lame",
        "aac" | "m4a" => "aac",
        "wav" => "pcm_s16le",
        "wmv" => "wmv2",
        _ => "copy",
    }
}

/// Video and audio encoders a re-encoded trim uses for its container.
pub fn trim_video_encoders(ext: &str) -> (&'static str, &'static str) {
    match ext {
        "avi" => ("mpeg4", "libmp3lame"),
        "wmv" => ("wmv2", "wmav2"),
        _ => ("libx264", "aac"),
    }
}

/// A copy reproduces the cut only when the cut sits on a keyframe.
pub fn can_copy_at_trim_point(nearest_keyframe: Option<f64>, trim_point: f64) -> bool {
    nearest_keyframe.is_some_and(|k| (k - trim_point).abs() <= KEYFRAME_EPSILON_SECONDS)
}

/// Where a copied part 1 must stop to meet part 2 on the same frame: a stream
/// with B-frames overshoots `-t` by its reorder delay of `has_b_frames` frames.
pub fn copy_part1_limit(keyframe: f64, has_b_frames: f64, fps: f64) -> f64 {
    if !fps.is_finite() || fps <= 0.0 || has_b_frames <= 0.0 {
        return keyframe;
    }
    keyframe - has_b_frames / fps
}

/// How the two parts of a video are cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VideoCut {
    /// Stream copy; part 1 stops at `part1_limit`, part 2 starts at `part2_start`.
    Copy { part1_limit: f64, part2_start: f64 },
    /// Frame-exact re-encode at the requested point.
    Reencode,
}

/// The copy-versus-re-encode decision (`trim-audio.ts:339-372`).
pub fn video_cut(trim_point: f64, keyframe: Option<f64>, seam: impl FnOnce() -> (f64, f64)) -> VideoCut {
    let (Some(keyframe), true) = (keyframe, can_copy_at_trim_point(keyframe, trim_point)) else {
        return VideoCut::Reencode;
    };
    let (has_b_frames, fps) = seam();
    let part1_limit = copy_part1_limit(keyframe, has_b_frames, fps);
    if part1_limit > 0.0 { VideoCut::Copy { part1_limit, part2_start: keyframe } } else { VideoCut::Reencode }
}

/// Trim progress from the two parts' `out_time_ms` (microseconds). `None`
/// while neither part has reported.
pub fn trim_percent(part1_us: f64, part2_us: f64, trim_point: f64, total_duration: f64) -> Option<f64> {
    if part1_us == 0.0 && part2_us == 0.0 {
        return None;
    }
    let p1 = (part1_us / 1_000_000.0).min(trim_point);
    let p2 = (part2_us / 1_000_000.0).min(total_duration - trim_point);
    Some(js_round((p1 + p2) / total_duration * 100.0).min(99.0))
}

// ---------------------------------------------------------------------------
// Audio formats: convert-audio and reduce-audio
// ---------------------------------------------------------------------------

/// Encoder settings for one audio output.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioEncoding {
    pub codec: &'static str,
    /// `-b:a`, omitted for lossless outputs.
    pub bitrate_kbps: Option<f64>,
    pub sample_rate: f64,
    pub channels: f64,
    /// `-f` value, when the container is not implied by the extension.
    pub container: Option<&'static str>,
    pub extra: &'static [&'static str],
    pub output_ext: &'static str,
}

fn audio_codec(format: &str) -> Option<(&'static str, Option<&'static str>, &'static [&'static str], &'static str)> {
    Some(match format {
        "mp3" => ("libmp3lame", None, &[], "mp3"),
        "aac" => ("aac", None, &[], "aac"),
        "m4a" => ("aac", Some("mp4"), &[], "m4a"),
        "ogg" => ("vorbis", None, &["-strict", "-2"], "ogg"),
        "opus" => ("libopus", Some("opus"), &[], "opus"),
        "flac" => ("flac", None, &[], "flac"),
        "wav" => ("pcm_s16le", None, &[], "wav"),
        "wma" => ("wmav2", Some("asf"), &[], "wma"),
        _ => return None,
    })
}

fn lossless_audio(format: &str) -> bool {
    matches!(format, "flac" | "wav")
}

/// `POST /convert-audio/convert` options, in the source's check order.
pub fn convert_audio_options(
    output_format: Arg<'_>,
    bitrate: Arg<'_>,
    sample_rate: Arg<'_>,
    channels: Arg<'_>,
) -> Result<AudioEncoding, Refusal> {
    let (format, (codec, container, extra, output_ext)) =
        match output_format.text().and_then(|f| audio_codec(f).map(|c| (f, c))) {
            Some(found) => found,
            None => return Err(invalid("Invalid output format.")),
        };
    let lossless = lossless_audio(format);
    if !lossless && !bitrate.number().is_some_and(|b| (32.0..=320.0).contains(&b)) {
        return Err(invalid("Bitrate must be between 32 and 320 kbps."));
    }
    let sample_rate = if format == "opus" { Some(48000.0) } else { sample_rate.number() };
    let Some(sample_rate) = sample_rate.filter(|r| [8000.0, 22050.0, 44100.0, 48000.0, 96000.0].contains(r)) else {
        return Err(invalid("Please select a valid sample rate."));
    };
    let Some(channels) = channels.number().filter(|c| *c == 1.0 || *c == 2.0) else {
        return Err(invalid("Please select Mono or Stereo."));
    };
    Ok(AudioEncoding {
        codec,
        bitrate_kbps: if lossless { None } else { bitrate.number() },
        sample_rate,
        channels,
        container,
        extra,
        output_ext,
    })
}

/// Converting to the upload's own format is refused once the session is known.
pub fn convert_audio_differs(input_ext: &str, encoding: &AudioEncoding) -> Result<(), Refusal> {
    if input_ext == encoding.output_ext {
        return Err(invalid("Output format must be different from input format."));
    }
    Ok(())
}

/// Low, medium or high: reduction, denoise and compression presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    Medium,
    High,
}

impl Level {
    fn parse(arg: Arg<'_>) -> Option<Level> {
        match arg.text()? {
            "low" => Some(Level::Low),
            "medium" => Some(Level::Medium),
            "high" => Some(Level::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }
}

/// `quality` for reduce-audio and compress-video.
pub fn quality(arg: Arg<'_>) -> Result<Level, Refusal> {
    Level::parse(arg).ok_or(invalid("quality must be low, medium, or high"))
}

/// `level` for remove-noise.
pub fn noise_level(arg: Arg<'_>) -> Result<Level, Refusal> {
    Level::parse(arg).ok_or(invalid("level must be low, medium, or high"))
}

/// The reduction preset for the upload's format (`reduce-audio.ts:135-204`).
pub fn reduce_audio(input_ext: &str, level: Level) -> Result<AudioEncoding, Refusal> {
    let Some((codec, container, extra, output_ext)) = audio_codec(input_ext) else {
        return Err(invalid("Unsupported audio format."));
    };
    let (bitrate, sample_rate, channels) = match (input_ext, level) {
        ("mp3", Level::Low) => (Some(64.0), 22050.0, 1.0),
        ("mp3", Level::Medium) => (Some(128.0), 44100.0, 2.0),
        ("mp3", Level::High) => (Some(192.0), 44100.0, 2.0),
        ("opus", Level::Low) => (Some(32.0), 24000.0, 1.0),
        ("opus", Level::Medium) => (Some(64.0), 48000.0, 2.0),
        ("opus", Level::High) => (Some(128.0), 48000.0, 2.0),
        ("flac" | "wav", Level::Low) => (None, 22050.0, 1.0),
        ("flac" | "wav", _) => (None, 44100.0, 2.0),
        // aac, m4a, ogg and wma share one table.
        (_, Level::Low) => (Some(48.0), 22050.0, 1.0),
        (_, Level::Medium) => (Some(96.0), 44100.0, 2.0),
        (_, Level::High) => (Some(160.0), 44100.0, 2.0),
    };
    Ok(AudioEncoding {
        codec,
        bitrate_kbps: if lossless_audio(input_ext) { None } else { bitrate },
        sample_rate,
        channels,
        container,
        extra,
        output_ext,
    })
}

/// Progress parsed from `-progress pipe:2`: only ever increases, capped at 99
/// until the process exits cleanly. `None` when the duration is unknown.
pub fn stream_percent(out_time_us: f64, total_us: Option<f64>, last: f64) -> Option<f64> {
    let total = total_us.filter(|t| *t > 0.0)?;
    let pct = js_round(out_time_us / total * 100.0).min(99.0);
    (pct > last).then_some(pct)
}

/// Duration in microseconds as the source rounds it for progress.
pub fn duration_us(seconds: f64) -> Option<f64> {
    (!seconds.is_nan() && seconds > 0.0).then(|| js_round(seconds * 1_000_000.0))
}

// ---------------------------------------------------------------------------
// Remove noise
// ---------------------------------------------------------------------------

/// afftdn settings and the codec the cleaned track is re-encoded with.
#[derive(Clone, Debug, PartialEq)]
pub struct Denoise {
    /// `-af` value.
    pub filter: String,
    pub codec: &'static str,
    pub extra: &'static [&'static str],
    /// Video inputs keep their video stream (`-c:v copy`).
    pub copy_video: bool,
}

/// `afftdn` noise reduction in dB by level; kept in lockstep with the browser path.
pub fn afftdn_nr(level: Level) -> u32 {
    match level {
        Level::Low => 10,
        Level::Medium => 25,
        Level::High => 45,
    }
}

pub const NOISE_FLOOR_DB: i32 = -25;

pub fn denoise(ext: &str, level: Level) -> Denoise {
    let (codec, extra): (&'static str, &'static [&'static str]) = match ext {
        "mp3" | "avi" | "wmv" => ("libmp3lame", &[]),
        "wav" => ("pcm_s16le", &[]),
        "ogg" => ("vorbis", &["-strict", "-2"]),
        "flac" => ("flac", &[]),
        "opus" => ("libopus", &[]),
        "webm" => ("libvorbis", &[]),
        _ => ("aac", &[]),
    };
    Denoise {
        filter: format!("afftdn=nr={}:nf={}", afftdn_nr(level), NOISE_FLOOR_DB),
        codec,
        extra,
        copy_video: matches!(ext, "mp4" | "mov" | "avi" | "wmv" | "webm" | "mkv"),
    }
}

/// Progress from a `-progress` file's last `out_time_ms` (microseconds) and
/// the probed duration. `None` while there is nothing to divide.
pub fn file_percent(out_time_us: f64, duration_seconds: Option<f64>) -> Option<f64> {
    let duration = duration_seconds.filter(|d| *d > 0.0)?;
    if out_time_us == 0.0 {
        return None;
    }
    Some(js_round(out_time_us / 1_000_000.0 / duration * 100.0).clamp(0.0, 99.0))
}

// ---------------------------------------------------------------------------
// Video to audio
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Extraction {
    pub output_ext: &'static str,
    pub codec: &'static str,
    pub bitrate_kbps: Option<f64>,
    pub sample_rate: f64,
    pub channels: f64,
}

/// `POST /video-to-audio/extract` options, in the source's check order.
pub fn extraction_options(
    output_format: Arg<'_>,
    bitrate: Arg<'_>,
    sample_rate: Arg<'_>,
    channels: Arg<'_>,
) -> Result<Extraction, Refusal> {
    let (output_ext, codec) = match output_format.text() {
        Some("mp3") => ("mp3", "libmp3lame"),
        Some("aac") => ("aac", "aac"),
        Some("wav") => ("wav", "pcm_s16le"),
        Some("ogg") => ("ogg", "vorbis"),
        Some("opus") => ("opus", "libopus"),
        _ => return Err(invalid("Invalid output format.")),
    };
    let lossless = output_ext == "wav";
    if lossless && !matches!(bitrate, Arg::Null | Arg::Absent) {
        return Err(invalid("WAV is lossless — bitrate is not applicable."));
    }
    if !lossless && !bitrate.number().is_some_and(|b| (32.0..=320.0).contains(&b)) {
        return Err(invalid("Bitrate must be between 32 and 320 kbps."));
    }
    let sample_rate = if output_ext == "opus" { Some(48000.0) } else { sample_rate.number() };
    let Some(sample_rate) = sample_rate.filter(|r| [8000.0, 22050.0, 44100.0, 48000.0].contains(r)) else {
        return Err(invalid("Please select a valid sample rate."));
    };
    let Some(channels) = channels.number().filter(|c| *c == 1.0 || *c == 2.0) else {
        return Err(invalid("Please select Mono or Stereo."));
    };
    Ok(Extraction {
        output_ext,
        codec,
        bitrate_kbps: if lossless { None } else { bitrate.number() },
        sample_rate,
        channels,
    })
}

// ---------------------------------------------------------------------------
// Compress video
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Compression {
    pub video_codec: &'static str,
    /// `-crf` for libx264, `-q:v` for mpeg4.
    pub quality_flag: &'static str,
    pub quality_value: u32,
    /// `-vf scale=-2:<h>` when the source is taller than the level allows.
    pub scale: Option<String>,
    pub preset: Option<&'static str>,
    /// `-maxrate` and `-bufsize` in kbps, from the source video bitrate.
    pub rate_cap_kbps: Option<(f64, f64)>,
    pub audio_bitrate_kbps: u32,
    pub audio_sample_rate: u32,
    pub audio_channels: u32,
}

/// Compression is refused for a session whose container is not a video one.
pub fn compression_format(input_ext: &str) -> Result<(), Refusal> {
    if matches!(input_ext, "mp4" | "mov" | "avi" | "mkv") { Ok(()) } else { Err(invalid("Unsupported video format.")) }
}

/// `compress-video.ts:144-202`.
pub fn compression(
    format: &str,
    level: Level,
    source_height: Option<f64>,
    source_video_kbps: Option<f64>,
) -> Compression {
    let avi = format == "avi";
    let quality_value = match (avi, level) {
        (true, Level::Low) => 8,
        (true, Level::Medium) => 5,
        (true, Level::High) => 2,
        (false, Level::Low) => 32,
        (false, Level::Medium) => 26,
        (false, Level::High) => 20,
    };
    let max_height = match level {
        Level::Low => Some(720.0),
        Level::Medium => Some(1080.0),
        Level::High => None,
    };
    let (audio_bitrate_kbps, audio_sample_rate, audio_channels) = match level {
        Level::Low => (64, 44100, 1),
        Level::Medium => (128, 44100, 2),
        Level::High => (192, 48000, 2),
    };
    let scale = match (max_height, source_height) {
        (Some(max), Some(height)) if height > max => Some(format!("scale=-2:{max}")),
        _ => None,
    };
    let factor = match level {
        Level::Low => 0.35,
        Level::Medium => 0.55,
        Level::High => 0.75,
    };
    let rate_cap_kbps = match source_video_kbps {
        Some(kbps) if !avi && kbps > 0.0 => {
            let max = js_round(kbps * factor).max(100.0);
            Some((max, max * 2.0))
        }
        _ => None,
    };
    Compression {
        video_codec: if avi { "mpeg4" } else { "libx264" },
        quality_flag: if avi { "-q:v" } else { "-crf" },
        quality_value,
        scale,
        preset: if avi { None } else { Some("veryfast") },
        rate_cap_kbps,
        audio_bitrate_kbps,
        audio_sample_rate,
        audio_channels,
    }
}

/// Compression progress from the last `out_time_us`; 0 without a duration.
pub fn compress_percent(out_time_us: Option<f64>, total_us: Option<f64>) -> f64 {
    match (out_time_us, total_us.filter(|t| *t > 0.0)) {
        (Some(out), Some(total)) => js_round(out / total * 100.0).min(99.0),
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Convert video
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct VideoConversion {
    pub output_ext: &'static str,
    pub video_codec: &'static str,
    /// `WxH` for `-vf scale=W:H`; `None` keeps the original size.
    pub scale: Option<(&'static str, &'static str)>,
    /// `-r`; `None` keeps the original rate.
    pub fps: Option<f64>,
    pub video_bitrate_kbps: f64,
    pub audio_codec: &'static str,
    pub audio_bitrate_kbps: f64,
    pub sample_rate: f64,
    pub channels: f64,
    /// `-f` for containers FFmpeg is told explicitly.
    pub container: Option<&'static str>,
}

/// `POST /convert-video/convert` options, in the source's check order.
#[allow(clippy::too_many_arguments)]
pub fn video_conversion_options(
    output_format: Arg<'_>,
    video_codec: Arg<'_>,
    resolution: Arg<'_>,
    fps: Arg<'_>,
    video_bitrate: Arg<'_>,
    audio_bitrate: Arg<'_>,
    sample_rate: Arg<'_>,
    channels: Arg<'_>,
) -> Result<VideoConversion, Refusal> {
    let Some(output_ext) =
        output_format.text().and_then(|f| ["mp4", "mov", "avi", "mkv"].into_iter().find(|v| *v == f))
    else {
        return Err(invalid("Invalid output format."));
    };
    let Some(video_codec) =
        video_codec.text().and_then(|c| ["libx264", "libx265", "mpeg4", "libvpx-vp9"].into_iter().find(|v| *v == c))
    else {
        return Err(invalid("Invalid video codec."));
    };
    let scale = match resolution.text() {
        Some("original") => None,
        Some("3840x2160") => Some(("3840", "2160")),
        Some("1920x1080") => Some(("1920", "1080")),
        Some("1280x720") => Some(("1280", "720")),
        Some("854x480") => Some(("854", "480")),
        Some("640x360") => Some(("640", "360")),
        _ => return Err(invalid("Invalid resolution.")),
    };
    let fps = match fps {
        Arg::Text("original") => None,
        Arg::Number(n) if n.fract() == 0.0 && (1.0..=120.0).contains(&n) => Some(n),
        _ => return Err(invalid("FPS must be between 1 and 120.")),
    };
    let Some(video_bitrate_kbps) = video_bitrate.number().filter(|b| (100.0..=50000.0).contains(b)) else {
        return Err(invalid("Video bitrate must be between 100 and 50,000 kbps."));
    };
    let Some(audio_bitrate_kbps) = audio_bitrate.number().filter(|b| (32.0..=320.0).contains(b)) else {
        return Err(invalid("Audio bitrate must be between 32 and 320 kbps."));
    };
    let Some(sample_rate) = sample_rate.number().filter(|r| *r == 44100.0 || *r == 48000.0) else {
        return Err(invalid("Please select a valid sample rate."));
    };
    let Some(channels) = channels.number().filter(|c| *c == 1.0 || *c == 2.0) else {
        return Err(invalid("Please select Mono or Stereo."));
    };
    Ok(VideoConversion {
        output_ext,
        video_codec,
        scale,
        fps,
        video_bitrate_kbps,
        audio_codec: if output_ext == "avi" { "libmp3lame" } else { "aac" },
        audio_bitrate_kbps,
        sample_rate,
        channels,
        container: match output_ext {
            "avi" => Some("avi"),
            "mov" => Some("mov"),
            _ => None,
        },
    })
}

/// A conversion that ended on a signal (no exit code) we did not send was the
/// container memory limit, not a bad input (`convert-video.ts:467-482`).
pub fn conversion_failure(exit_code: Option<i32>) -> &'static str {
    if exit_code.is_none() {
        "This video needs more memory than our servers allow. Try a smaller file or a lower output resolution."
    } else {
        "Something went wrong during conversion. Please try again."
    }
}

/// `Math.round(fps * 100) / 100` of an ffprobe `num/den` rate.
pub fn rounded_fps(num: f64, den: f64) -> Option<f64> {
    (den != 0.0).then(|| js_round(num / den * 100.0) / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(name: Option<&'a str>, mime: &'a str) -> UploadedFile<'a> {
        UploadedFile { original_filename: name, mime, size: 10 }
    }

    #[test]
    fn every_family_has_its_captured_mount_prefix_ttl_size_and_allowance() {
        let rows: Vec<_> = Family::ALL
            .iter()
            .map(|f| {
                (f.mount(), f.file_prefix(), f.session_ttl().as_secs() / 60, f.max_upload_bytes() >> 30, f.processing_per_hour())
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("trim-audio", "trim_", 60, 1, 20),
                ("convert-audio", "conv_", 60, 2, 20),
                ("convert-video", "convvideo_", 60, 2, 10),
                ("reduce-audio", "redaud_", 120, 2, 20),
                ("compress-video", "compvid_", 120, 2, 10),
                ("video-to-audio", "v2audio_", 120, 2, 20),
                ("remove-noise", "rmnoise_", 120, 2, 20),
            ]
        );
        assert_eq!(CLEANUP_INTERVAL, Duration::from_secs(600));
        // No prefix is a prefix of another, so orphan purge never crosses families.
        for a in Family::ALL {
            for b in Family::ALL {
                if a != b {
                    assert!(!b.file_prefix().starts_with(a.file_prefix()), "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_file_of_exactly_the_limit_is_accepted_and_one_byte_more_is_not() {
        let at = UploadedFile { original_filename: Some("a.mp3"), mime: "audio/mpeg", size: 1 << 30 };
        assert!(Family::TrimAudio.accept_upload(&at).is_ok());
        let over = UploadedFile { size: (1 << 30) + 1, ..at };
        assert_eq!(Family::TrimAudio.accept_upload(&over), Err(UploadRefusal::TooLarge));
        assert!(Family::ConvertAudio.accept_upload(&over).is_ok());
    }

    #[test]
    fn the_extension_wins_over_the_mime_type_and_the_mime_type_fills_in_for_an_unknown_one() {
        let accept = |f: Family, name, mime| f.accept_upload(&file(name, mime)).map(|a| a.extension);
        assert_eq!(accept(Family::TrimAudio, Some("Song.MP3"), "video/mp4"), Ok("mp3".into()));
        assert_eq!(accept(Family::TrimAudio, Some("clip"), "video/quicktime"), Ok("mov".into()));
        // convert-audio maps video/mp4 to m4a, trim to mp4.
        assert_eq!(accept(Family::ConvertAudio, Some("clip.bin"), "video/mp4"), Ok("m4a".into()));
        assert_eq!(accept(Family::TrimAudio, Some("clip.bin"), "video/mp4"), Ok("mp4".into()));
        assert_eq!(accept(Family::ConvertAudio, Some("a.txt"), "text/plain"), Err(UploadRefusal::Unsupported));
        // video-to-audio matches MIME substrings, which miss video/mov; convert-video maps it exactly.
        assert_eq!(accept(Family::VideoToAudio, None, "video/mov"), Err(UploadRefusal::Unsupported));
        assert_eq!(accept(Family::VideoToAudio, None, "video/x-matroska"), Ok("mkv".into()));
        assert_eq!(accept(Family::ConvertVideo, None, "video/mov"), Ok("mov".into()));
        // compression looks at the extension only.
        assert_eq!(accept(Family::CompressVideo, None, "video/mp4"), Err(UploadRefusal::Unsupported));
        assert_eq!(accept(Family::CompressVideo, Some("x.MKV"), "application/x-whatever"), Ok("mkv".into()));
        assert_eq!(accept(Family::RemoveNoise, Some("x"), "video/webm"), Ok("webm".into()));
    }

    #[test]
    fn format_and_size_refusals_follow_each_familys_order() {
        let big = |name| UploadedFile { original_filename: Some(name), mime: "text/plain", size: 3 << 30 };
        // Audio families check the format first only when both MIME and extension fail.
        assert_eq!(Family::ConvertAudio.accept_upload(&big("a.mp3")), Err(UploadRefusal::TooLarge));
        assert_eq!(Family::ConvertAudio.accept_upload(&big("a.txt")), Err(UploadRefusal::Unsupported));
        assert_eq!(Family::VideoToAudio.accept_upload(&big("a.mp4")), Err(UploadRefusal::TooLarge));
        assert_eq!(Family::CompressVideo.accept_upload(&big("a.txt")), Err(UploadRefusal::Unsupported));
        assert_eq!(
            Family::TrimAudio.upload_refusal(UploadRefusal::TooLarge),
            "This file exceeds the 1GB limit. Try compressing it first."
        );
    }

    #[test]
    fn safe_names_replace_utf16_units_strip_the_extension_and_cut_at_80() {
        assert_eq!(safe_name(Some("My Song (final).mp3"), "audio"), "My_Song__final_");
        assert_eq!(safe_name(Some("ünïcode.wav"), "audio"), "_n_code");
        // An astral character is two UTF-16 units.
        assert_eq!(safe_name(Some("a😀b.mp3"), "audio"), "a__b");
        assert_eq!(safe_name(Some("dir/sub/name.tar.gz"), "audio"), "name.tar");
        assert_eq!(safe_name(Some(".hidden"), "audio"), ".hidden");
        assert_eq!(safe_name(None, "video"), "video");
        assert_eq!(safe_name(Some(""), "media"), "media");
        assert_eq!(safe_name(Some(&format!("{}.mp3", "x".repeat(100))), "audio").len(), 80);
        assert_eq!(extension_of("archive.TAR.GZ"), "gz");
        assert_eq!(extension_of("noext"), "");
        assert_eq!(extension_of(".bashrc"), "");
        assert_eq!(extension_of("..."), "");
    }

    #[test]
    fn only_server_shaped_uuid_v4_session_ids_are_valid() {
        assert!(is_valid_session_id("3f2504e0-4f89-41d3-9a0c-0305e82c3301"));
        assert!(is_valid_session_id("3F2504E0-4F89-41D3-9A0C-0305E82C3301"));
        assert!(!is_valid_session_id("3f2504e0-4f89-11d3-9a0c-0305e82c3301"));
        assert!(!is_valid_session_id("3f2504e0-4f89-41d3-7a0c-0305e82c3301"));
        assert!(!is_valid_session_id("../../../../etc/passwd-aaaaaaaaaaaaaaaa"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn the_allowance_is_a_fixed_window_that_counts_accepted_checks_only() {
        let start = Instant::now();
        let mut allowance = ProcessingAllowance::new(2);
        assert!(allowance.check("ip", start));
        assert!(allowance.check("ip", start + Duration::from_secs(10)));
        assert!(!allowance.check("ip", start + Duration::from_secs(20)));
        assert!(allowance.check("other", start), "per address");
        // Exactly one hour after the first request is still the same window.
        assert!(!allowance.check("ip", start + ALLOWANCE_WINDOW));
        assert!(allowance.check("ip", start + ALLOWANCE_WINDOW + Duration::from_millis(1)));
        assert!(allowance.check("ip", start + ALLOWANCE_WINDOW + Duration::from_millis(2)));
        assert!(!allowance.check("ip", start + ALLOWANCE_WINDOW + Duration::from_millis(3)));
        allowance.forget_expired(start + 3 * ALLOWANCE_WINDOW);
        assert!(allowance.hits.is_empty());
    }

    #[test]
    fn trim_requires_a_numeric_point_strictly_inside_a_known_duration() {
        assert_eq!(trim_request(Arg::Text("id"), Arg::Number(2.5)), Ok(2.5));
        assert!(trim_request(Arg::Text("id"), Arg::Text("2.5")).is_err());
        assert!(trim_request(Arg::Text(""), Arg::Number(2.5)).is_err());
        assert!(trim_request(Arg::Number(7.0), Arg::Number(1.0)).is_ok(), "any truthy id passes the shape check");
        assert!(trim_point_in_range(0.0, 10.0).is_err());
        assert!(trim_point_in_range(10.0, 10.0).is_err());
        assert!(trim_point_in_range(9.99, 10.0).is_ok());
        assert!(trim_point_in_range(5.0, f64::NAN).is_ok(), "an unreadable duration refuses nothing");
    }

    #[test]
    fn a_video_cut_copies_only_on_a_keyframe_and_backs_off_by_the_reorder_delay() {
        assert!(!can_copy_at_trim_point(None, 5.0));
        assert!(!can_copy_at_trim_point(Some(0.0), 5.0));
        assert!(can_copy_at_trim_point(Some(8.333333), 8.34));
        assert_eq!(copy_part1_limit(8.333333, 2.0, 30.0), 8.333333 - 2.0 / 30.0);
        assert_eq!(copy_part1_limit(8.0, 0.0, 30.0), 8.0);
        assert_eq!(copy_part1_limit(8.0, 2.0, 0.0), 8.0);
        assert_eq!(video_cut(5.0, Some(0.0), || panic!("no seam probe off a keyframe")), VideoCut::Reencode);
        assert_eq!(
            video_cut(8.34, Some(8.333333), || (2.0, 30.0)),
            VideoCut::Copy { part1_limit: 8.333333 - 2.0 / 30.0, part2_start: 8.333333 }
        );
        // A keyframe inside the reorder delay of the start leaves no part 1 to copy.
        assert_eq!(video_cut(0.05, Some(0.05), || (2.0, 30.0)), VideoCut::Reencode);
        assert_eq!(trim_video_encoders("avi"), ("mpeg4", "libmp3lame"));
        assert_eq!(trim_video_encoders("wmv"), ("wmv2", "wmav2"));
        assert_eq!(trim_video_encoders("mov"), ("libx264", "aac"));
        assert_eq!(trim_audio_codec("aac", "m4a"), "copy");
        assert_eq!(trim_audio_codec("pcm_s16le", "mp3"), "libmp3lame");
        assert_eq!(trim_audio_codec("", "wav"), "pcm_s16le");
    }

    #[test]
    fn trim_progress_sums_both_parts_and_never_reports_done() {
        assert_eq!(trim_percent(0.0, 0.0, 4.0, 10.0), None);
        assert_eq!(trim_percent(2_000_000.0, 3_000_000.0, 4.0, 10.0), Some(50.0));
        assert_eq!(trim_percent(9e9, 9e9, 4.0, 10.0), Some(99.0));
    }

    #[test]
    fn audio_conversion_validates_in_order_and_forces_opus_to_48k() {
        let ok = convert_audio_options(Arg::Text("opus"), Arg::Number(96.0), Arg::Number(8000.0), Arg::Number(2.0));
        assert_eq!(ok.as_ref().map(|e| (e.sample_rate, e.container, e.codec)), Ok((48000.0, Some("opus"), "libopus")));
        let flac = convert_audio_options(Arg::Text("flac"), Arg::Absent, Arg::Number(96000.0), Arg::Number(1.0)).unwrap();
        assert_eq!(flac.bitrate_kbps, None, "lossless ignores bitrate");
        let message = |r: Result<AudioEncoding, Refusal>| r.unwrap_err().message;
        assert_eq!(
            message(convert_audio_options(Arg::Text("aiff"), Arg::Number(1.0), Arg::Absent, Arg::Absent)),
            "Invalid output format."
        );
        assert_eq!(
            message(convert_audio_options(Arg::Text("mp3"), Arg::Number(321.0), Arg::Absent, Arg::Absent)),
            "Bitrate must be between 32 and 320 kbps."
        );
        assert_eq!(
            message(convert_audio_options(Arg::Text("mp3"), Arg::Number(320.0), Arg::Number(96000.5), Arg::Absent)),
            "Please select a valid sample rate."
        );
        assert_eq!(
            message(convert_audio_options(Arg::Text("mp3"), Arg::Number(32.0), Arg::Number(44100.0), Arg::Text("2"))),
            "Please select Mono or Stereo."
        );
        let wma = convert_audio_options(Arg::Text("wma"), Arg::Number(128.0), Arg::Number(44100.0), Arg::Number(1.0)).unwrap();
        assert_eq!(convert_audio_differs("wma", &wma).unwrap_err().message, "Output format must be different from input format.");
        assert!(convert_audio_differs("wav", &wma).is_ok());
    }

    #[test]
    fn reduction_presets_follow_the_format_table() {
        let row = |ext, level| {
            let e = reduce_audio(ext, level).unwrap();
            (e.codec, e.bitrate_kbps, e.sample_rate, e.channels, e.container)
        };
        assert_eq!(row("mp3", Level::Low), ("libmp3lame", Some(64.0), 22050.0, 1.0, None));
        assert_eq!(row("opus", Level::Low), ("libopus", Some(32.0), 24000.0, 1.0, Some("opus")));
        assert_eq!(row("m4a", Level::High), ("aac", Some(160.0), 44100.0, 2.0, Some("mp4")));
        assert_eq!(row("wav", Level::Medium), ("pcm_s16le", None, 44100.0, 2.0, None));
        assert_eq!(row("wma", Level::Medium), ("wmav2", Some(96.0), 44100.0, 2.0, Some("asf")));
        assert_eq!(reduce_audio("mp4", Level::Low).unwrap_err().message, "Unsupported audio format.");
        assert_eq!(quality(Arg::Text("max")).unwrap_err().message, "quality must be low, medium, or high");
        assert_eq!(noise_level(Arg::Absent).unwrap_err().message, "level must be low, medium, or high");
    }

    #[test]
    fn stream_progress_only_increases_and_waits_for_exit_to_reach_100() {
        assert_eq!(stream_percent(500_000.0, Some(1_000_000.0), 0.0), Some(50.0));
        assert_eq!(stream_percent(400_000.0, Some(1_000_000.0), 50.0), None);
        assert_eq!(stream_percent(2_000_000.0, Some(1_000_000.0), 50.0), Some(99.0));
        assert_eq!(stream_percent(1.0, None, 0.0), None);
        assert_eq!(duration_us(f64::NAN), None);
        assert_eq!(duration_us(0.0), None);
        assert_eq!(duration_us(2.0), Some(2_000_000.0));
    }

    #[test]
    fn denoise_keeps_video_and_picks_a_codec_per_container() {
        let d = denoise("webm", Level::High);
        assert_eq!((d.filter.as_str(), d.codec, d.copy_video), ("afftdn=nr=45:nf=-25", "libvorbis", true));
        let d = denoise("ogg", Level::Low);
        assert_eq!((d.filter.as_str(), d.codec, d.extra, d.copy_video), ("afftdn=nr=10:nf=-25", "vorbis", &["-strict", "-2"][..], false));
        assert_eq!(denoise("avi", Level::Medium).codec, "libmp3lame");
        assert_eq!(file_percent(0.0, Some(10.0)), None);
        assert_eq!(file_percent(5_000_000.0, None), None);
        assert_eq!(file_percent(5_000_000.0, Some(10.0)), Some(50.0));
        assert_eq!(file_percent(50_000_000.0, Some(10.0)), Some(99.0));
    }

    #[test]
    fn extraction_refuses_a_wav_bitrate_and_forces_opus_to_48k() {
        let message = |r: Result<Extraction, Refusal>| r.unwrap_err().message;
        assert_eq!(
            message(extraction_options(Arg::Text("wav"), Arg::Number(128.0), Arg::Number(44100.0), Arg::Number(2.0))),
            "WAV is lossless — bitrate is not applicable."
        );
        assert!(extraction_options(Arg::Text("wav"), Arg::Null, Arg::Number(44100.0), Arg::Number(2.0)).is_ok());
        assert_eq!(
            message(extraction_options(Arg::Text("flac"), Arg::Null, Arg::Absent, Arg::Absent)),
            "Invalid output format."
        );
        assert_eq!(
            message(extraction_options(Arg::Text("mp3"), Arg::Number(128.0), Arg::Number(96000.0), Arg::Number(2.0))),
            "Please select a valid sample rate."
        );
        let opus = extraction_options(Arg::Text("opus"), Arg::Number(64.0), Arg::Absent, Arg::Number(1.0)).unwrap();
        assert_eq!((opus.sample_rate, opus.codec), (48000.0, "libopus"));
    }

    #[test]
    fn compression_scales_down_tall_sources_and_caps_libx264_rate() {
        let c = compression("mp4", Level::Low, Some(1080.0), Some(4000.0));
        assert_eq!(c.scale.as_deref(), Some("scale=-2:720"));
        assert_eq!((c.quality_flag, c.quality_value, c.preset), ("-crf", 32, Some("veryfast")));
        assert_eq!(c.rate_cap_kbps, Some((1400.0, 2800.0)));
        assert_eq!((c.audio_bitrate_kbps, c.audio_sample_rate, c.audio_channels), (64, 44100, 1));
        let tiny = compression("mkv", Level::Medium, Some(360.0), Some(100.0));
        assert_eq!((tiny.scale, tiny.rate_cap_kbps), (None, Some((100.0, 200.0))));
        let avi = compression("avi", Level::High, Some(2160.0), Some(9000.0));
        assert_eq!((avi.video_codec, avi.quality_flag, avi.quality_value, avi.scale, avi.rate_cap_kbps), ("mpeg4", "-q:v", 2, None, None));
        assert_eq!(compression("mov", Level::Low, None, None).rate_cap_kbps, None);
        assert_eq!(compress_percent(Some(2_500_000.0), Some(10_000_000.0)), 25.0);
        assert_eq!(compress_percent(Some(1.0), None), 0.0);
        assert!(compression_format("webm").is_err());
    }

    #[test]
    fn video_conversion_validates_every_option_in_order() {
        let n = Arg::Number;
        let ok = video_conversion_options(
            Arg::Text("avi"),
            Arg::Text("mpeg4"),
            Arg::Text("640x360"),
            Arg::Text("original"),
            n(800.0),
            n(128.0),
            n(44100.0),
            n(2.0),
        )
        .unwrap();
        assert_eq!((ok.audio_codec, ok.container, ok.scale, ok.fps), ("libmp3lame", Some("avi"), Some(("640", "360")), None));
        let refuse = |fmt, codec, res, fps, vb, ab, sr, ch| {
            video_conversion_options(fmt, codec, res, fps, vb, ab, sr, ch).unwrap_err().message
        };
        let (f, c, r) = (Arg::Text("mp4"), Arg::Text("libx264"), Arg::Text("original"));
        assert_eq!(refuse(Arg::Text("webm"), c, r, n(30.0), n(800.0), n(128.0), n(44100.0), n(2.0)), "Invalid output format.");
        assert_eq!(refuse(f, Arg::Text("h264"), r, n(30.0), n(800.0), n(128.0), n(44100.0), n(2.0)), "Invalid video codec.");
        assert_eq!(refuse(f, c, Arg::Text("1x1"), n(30.0), n(800.0), n(128.0), n(44100.0), n(2.0)), "Invalid resolution.");
        assert_eq!(refuse(f, c, r, n(29.97), n(800.0), n(128.0), n(44100.0), n(2.0)), "FPS must be between 1 and 120.");
        assert_eq!(refuse(f, c, r, n(121.0), n(800.0), n(128.0), n(44100.0), n(2.0)), "FPS must be between 1 and 120.");
        assert_eq!(
            refuse(f, c, r, n(30.0), n(99.0), n(128.0), n(44100.0), n(2.0)),
            "Video bitrate must be between 100 and 50,000 kbps."
        );
        assert_eq!(refuse(f, c, r, n(30.0), n(800.0), n(31.0), n(44100.0), n(2.0)), "Audio bitrate must be between 32 and 320 kbps.");
        assert_eq!(refuse(f, c, r, n(30.0), n(800.0), n(128.0), n(22050.0), n(2.0)), "Please select a valid sample rate.");
        assert_eq!(refuse(f, c, r, n(30.0), n(800.0), n(128.0), n(48000.0), n(3.0)), "Please select Mono or Stereo.");
        assert_eq!(conversion_failure(None), "This video needs more memory than our servers allow. Try a smaller file or a lower output resolution.");
        assert_eq!(rounded_fps(30000.0, 1001.0), Some(29.97));
        assert_eq!(rounded_fps(1.0, 0.0), None);
    }

    #[test]
    fn js_round_sends_halves_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }
}
