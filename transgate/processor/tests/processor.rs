//! End-to-end scenarios against a real loopback server, real FFmpeg/ffprobe
//! child processes and tiny media generated per test with FFmpeg's lavfi
//! sources (sine tone, test pattern). No fixture is checked in and nothing
//! leaves the machine. Requires `ffmpeg` and `ffprobe` on PATH.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use transgate_core::tools::Family;
use transgate_processor::{Config, Processor, Purged};

const ORIGIN: &str = "https://transgate.test";

#[path = "processor/acceptance.rs"]
mod acceptance;

struct Server {
    base: String,
    processor: Arc<Processor>,
    dir: PathBuf,
    http: reqwest::Client,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-processor-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn start(tag: &str) -> Server {
    start_with(tag, |_| {}).await
}

async fn start_with(tag: &str, configure: impl FnOnce(&mut Config)) -> Server {
    let dir = work_dir(tag);
    let mut config = Config::from_lookup(
        |name| match name {
            "ALLOWED_ORIGINS" => Some(ORIGIN.to_string()),
            _ => None,
        },
        dir.clone(),
    )
    .unwrap();
    configure(&mut config);
    let processor = Processor::new(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = processor.router().into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { base, processor, dir, http: client() }
}

/// Generates a small media file with FFmpeg.
fn media(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = dir.join("fixtures").join(name);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y"])
        .args(args)
        .arg(&out)
        .status()
        .expect("ffmpeg on PATH");
    assert!(status.success(), "generating {name}");
    out
}

fn tone_wav(dir: &Path) -> PathBuf {
    media(dir, "tone.wav", &["-f", "lavfi", "-i", "sine=frequency=440:duration=4", "-ac", "2"])
}

fn clip_mp4(dir: &Path, seconds: u32, size: &str) -> PathBuf {
    let video = format!("testsrc=size={size}:rate=25:duration={seconds}");
    let audio = format!("sine=frequency=330:duration={seconds}");
    media(
        dir,
        &format!("clip{seconds}_{size}.mp4"),
        &[
            "-f", "lavfi", "-i", &video, "-f", "lavfi", "-i", &audio, "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-x264-params", "keyint=25:min-keyint=25:scenecut=0", "-c:a", "aac", "-shortest",
        ],
    )
}

/// ffprobe a value of the first stream of `selector` (e.g. `a:0`).
fn probe(path: &Path, selector: &str, entry: &str) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-count_packets", "-select_streams", selector, "-show_entries", entry, "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success(), "ffprobe {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().lines().next().unwrap_or("").to_string()
}

/// Duration of the first audio packet: the granularity of a stream-copied cut.
fn first_audio_packet_seconds(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "packet=duration_time", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().next().and_then(|l| l.trim().parse().ok()).expect("a timed first packet")
}

fn duration(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn video_packets(path: &Path) -> u64 {
    probe(path, "v:0", "stream=nb_read_packets").parse().expect("ffprobe counted video packets")
}

fn multipart(field: &str, filename: &str, mime: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----transgate-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

struct Reply {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&self.body)))
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

impl Server {
    async fn send(&self, request: reqwest::RequestBuilder) -> Reply {
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await.unwrap().to_vec();
        Reply { status, headers, body }
    }

    async fn get(&self, path: &str) -> Reply {
        self.send(self.http.get(format!("{}{path}", self.base))).await
    }

    async fn json(&self, method: reqwest::Method, path: &str, body: Value) -> Reply {
        self.send(
            self.http
                .request(method, format!("{}{path}", self.base))
                .header("content-type", "application/json")
                .body(body.to_string()),
        )
        .await
    }

    async fn post(&self, path: &str, body: Value) -> Reply {
        self.json(reqwest::Method::POST, path, body).await
    }

    async fn upload(&self, family: &str, file: &Path, mime: &str) -> Value {
        let data = std::fs::read(file).unwrap();
        let name = file.file_name().unwrap().to_str().unwrap();
        let (content_type, body) = multipart("file", name, mime, &data);
        let reply = self
            .send(self.http.post(format!("{}/{family}/upload", self.base)).header("content-type", content_type).body(body))
            .await;
        assert_eq!(reply.status, 200, "{}", String::from_utf8_lossy(&reply.body));
        reply.json()
    }

    /// Polls `/progress` until `done` says so; panics after a minute.
    async fn poll(&self, family: &str, id: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let reply = self.get(&format!("/{family}/progress?sessionId={id}")).await;
            assert_eq!(reply.status, 200, "{family}: {}", String::from_utf8_lossy(&reply.body));
            let body = reply.json();
            if done(&body) {
                return body;
            }
            assert!(Instant::now() < deadline, "{family} never finished: {body}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn files_with_prefix(&self, prefix: &str) -> Vec<String> {
        std::fs::read_dir(&self.dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with(prefix))
            .collect()
    }

    /// Download-triggered cleanup runs after the body; wait for it.
    async fn wait_until_no_files(&self, prefix: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.files_with_prefix(prefix).is_empty() {
            assert!(Instant::now() < deadline, "left behind: {:?}", self.files_with_prefix(prefix));
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

fn save(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join("fixtures").join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[tokio::test]
async fn all_forty_routes_are_mounted_and_anything_else_is_the_express_404() {
    let server = start("routes").await;
    let health = server.get("/health").await;
    assert_eq!((health.status, health.json()), (200, json!({ "status": "ok" })));
    assert_eq!(health.header("content-type"), Some("application/json; charset=utf-8"));
    assert_eq!(server.get("/HEALTH/").await.status, 200, "Express matches case-insensitively with a trailing slash");
    assert_eq!(server.send(server.http.head(format!("{}/health", server.base))).await.status, 200);

    let empty_upload = |family: &str| {
        server.http.post(format!("{}/{family}/upload", server.base)).header("content-length", "0")
    };
    let bad_id = "Invalid or missing sessionId";
    let mut checked = 1;
    for (family, process) in [
        ("convert-audio", "convert"),
        ("reduce-audio", "reduce"),
        ("remove-noise", "denoise"),
        ("video-to-audio", "extract"),
        ("compress-video", "compress"),
        ("convert-video", "convert"),
    ] {
        let upload = server.send(empty_upload(family)).await;
        assert_eq!((upload.status, upload.json()), (400, json!({ "error": "No file provided" })), "{family}");
        let processing = server.post(&format!("/{family}/{process}"), json!({})).await;
        assert_eq!((processing.status, processing.json()), (400, json!({ "error": bad_id })), "{family}");
        let mut gets = vec!["progress", "download"];
        if matches!(family, "video-to-audio" | "compress-video" | "convert-video") {
            gets.push("thumbnail");
        }
        for action in &gets {
            let reply = server.get(&format!("/{family}/{action}?sessionId=nope")).await;
            assert_eq!((reply.status, reply.json()), (400, json!({ "error": bad_id })), "{family}/{action}");
        }
        let invalidate = server.json(reqwest::Method::DELETE, &format!("/{family}/invalidate"), json!({})).await;
        assert_eq!((invalidate.status, invalidate.json()), (400, json!({ "error": bad_id })), "{family}");
        checked += 3 + gets.len();
    }
    let trim = [
        (server.send(empty_upload("trim-audio")).await, "No file provided"),
        (server.post("/trim-audio/trim", json!({ "sessionId": "x" })).await, "sessionId and trimPoint are required"),
        (server.get("/trim-audio/progress").await, "sessionId is required"),
        (server.get("/trim-audio/download?sessionId=x").await, "sessionId and part are required"),
        (server.get("/trim-audio/download-zip").await, "sessionId is required"),
        (server.json(reqwest::Method::DELETE, "/trim-audio/invalidate", json!({})).await, "sessionId is required"),
    ];
    for (reply, message) in trim {
        assert_eq!((reply.status, reply.json()), (400, json!({ "error": message })));
        checked += 1;
    }
    assert_eq!(checked, 40);

    for (method, path) in [(reqwest::Method::GET, "/trim-audio/upload"), (reqwest::Method::POST, "/nope")] {
        let reply = server.send(server.http.request(method.clone(), format!("{}{path}", server.base))).await;
        assert_eq!(reply.status, 404);
        assert_eq!(reply.header("content-type"), Some("text/html; charset=utf-8"));
        assert!(String::from_utf8_lossy(&reply.body).contains(&format!("<pre>Cannot {method} {path}</pre>")));
    }
    // A malformed JSON body reaches the error middleware.
    let malformed = server
        .send(server.http.post(format!("{}/trim-audio/trim", server.base)).header("content-type", "application/json").body("{"))
        .await;
    assert_eq!(malformed.status, 500);
}

/// `express.json()` (body-parser 1.20) inflates gzip and deflate bodies,
/// limits the decoded size to 100 kb and reads UTF-16 charsets.
#[tokio::test]
async fn json_bodies_are_read_through_gzip_deflate_and_utf16_like_express() {
    use std::io::Write;
    let server = start("json").await;
    let body = json!({ "sessionId": uuid::Uuid::new_v4().to_string(), "outputFormat": "webm" }).to_string();
    let post = |encoding: Option<&str>, content_type: &str, bytes: Vec<u8>, ip: &str| {
        let mut request = server
            .http
            .post(format!("{}/convert-video/convert", server.base))
            .header("content-type", content_type)
            .header("x-forwarded-for", ip.to_string())
            .body(bytes);
        if let Some(encoding) = encoding {
            request = request.header("content-encoding", encoding);
        }
        server.send(request)
    };
    let parsed = json!({ "error": "Invalid output format." });
    let gzip = {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(body.as_bytes()).unwrap();
        e.finish().unwrap()
    };
    let reply = post(Some("GZIP"), "application/json", gzip, "10.0.0.1").await;
    assert_eq!((reply.status, reply.json()), (400, parsed.clone()), "gzip is inflated, case-insensitively");
    let deflate = {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(body.as_bytes()).unwrap();
        e.finish().unwrap()
    };
    let reply = post(Some("deflate"), "application/json", deflate, "10.0.0.2").await;
    assert_eq!((reply.status, reply.json()), (400, parsed.clone()));
    let utf16: Vec<u8> = body.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let reply = post(None, "application/json; charset=UTF-16LE", utf16, "10.0.0.3").await;
    assert_eq!((reply.status, reply.json()), (400, parsed));

    // 150 kb of whitespace compresses to a few hundred bytes; the decoded limit still applies.
    let bomb = {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(format!("{}{body}", " ".repeat(150 * 1024)).as_bytes()).unwrap();
        e.finish().unwrap()
    };
    assert!(bomb.len() < 2048);
    assert_eq!(post(Some("gzip"), "application/json", bomb, "10.0.0.4").await.status, 500);
    assert_eq!(post(Some("br"), "application/json", body.clone().into_bytes(), "10.0.0.5").await.status, 500);
    assert_eq!(post(None, "application/json; charset=latin1", body.clone().into_bytes(), "10.0.0.6").await.status, 500);
    // Not JSON: the body is left empty, so only the session id is missing.
    let ignored = post(Some("br"), "text/plain", body.into_bytes(), "10.0.0.7").await;
    assert_eq!((ignored.status, ignored.json()), (400, json!({ "error": "Invalid or missing sessionId" })));
}

#[tokio::test]
async fn cors_reflects_only_listed_origins_and_answers_every_preflight() {
    let server = start("cors").await;
    let preflight = server
        .send(
            server
                .http
                .request(reqwest::Method::OPTIONS, format!("{}/convert-audio/convert", server.base))
                .header("origin", ORIGIN)
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type"),
        )
        .await;
    assert_eq!(preflight.status, 204);
    assert_eq!(preflight.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(preflight.header("access-control-allow-methods"), Some("GET,POST,DELETE,OPTIONS"));
    assert_eq!(preflight.header("access-control-allow-headers"), Some("content-type"));
    let foreign = server.send(server.http.get(format!("{}/health", server.base)).header("origin", "https://evil.test")).await;
    assert_eq!((foreign.status, foreign.header("access-control-allow-origin")), (200, None));
    let listed = server.send(server.http.get(format!("{}/health", server.base)).header("origin", ORIGIN)).await;
    assert_eq!(listed.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(listed.header("vary"), Some("Origin"));
}

#[tokio::test]
async fn trim_audio_cuts_two_parts_serves_them_singly_and_as_a_zip_then_cleans_up() {
    let server = start("trim").await;
    let wav = tone_wav(&server.dir);
    let up = server.upload("trim-audio", &wav, "audio/wav").await;
    let id = s(&up["sessionId"]).to_string();
    assert_eq!((s(&up["originalName"]), s(&up["extension"])), ("tone", "wav"));
    assert!((up["totalDuration"].as_f64().unwrap() - 4.0).abs() < 0.05, "{up}");
    let waveform = up["waveform"].as_array().unwrap();
    assert_eq!(waveform.len(), 200);
    // lavfi's sine has amplitude 1/8; the failure fallback is a flat 0.1.
    assert!(waveform.iter().all(|v| v.as_f64().is_some_and(|a| a > 0.1 && a < 0.2)), "{waveform:?}");

    let out_of_range = server.post("/trim-audio/trim", json!({ "sessionId": id, "trimPoint": 4.0 })).await;
    assert_eq!(out_of_range.status, 400);
    let accepted = server.post("/trim-audio/trim", json!({ "sessionId": id, "trimPoint": 1.5 })).await;
    assert_eq!(accepted.status, 202);
    assert_eq!(
        accepted.json(),
        json!({
            "accepted": true,
            "sessionId": id,
            "part1": { "filename": "tone_part1.wav", "duration": 1.5 },
            "part2": { "filename": "tone_part2.wav", "duration": 2.5 },
        })
    );
    let done = server.poll("trim-audio", &id, |b| b["status"] != "processing").await;
    assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{done}");
    assert!(done["part1"]["sizeBytes"].as_u64().unwrap() > 0);

    let part1 = server.get(&format!("/trim-audio/download?sessionId={id}&part=1")).await;
    assert_eq!(part1.status, 200);
    assert_eq!(part1.header("content-type"), Some("audio/wav"));
    assert_eq!(part1.header("content-disposition"), Some("attachment; filename=\"tone_part1.wav\""));
    let part1_path = save(&server.dir, "part1.wav", &part1.body);
    // Captured source behavior (trim-audio.ts:375-379): PCM in WAV is stream-copied, and `-t` on
    // a copy keeps whole input packets, so part 1 ends on the first packet boundary at or after
    // the cut (here 17 x 4096 frames = 1.578957 s). Part 2 starts exactly at the cut (below).
    let packet = first_audio_packet_seconds(&wav);
    let first = duration(&part1_path);
    assert!(first >= 1.5 && first < 1.5 + packet, "part 1 lasts {first}s with {packet}s input packets");

    // One part downloaded keeps the session; the ZIP ends it.
    assert_eq!(server.get(&format!("/trim-audio/progress?sessionId={id}")).await.json()["status"], "done");
    let zip = server.get(&format!("/trim-audio/download-zip?sessionId={id}")).await;
    assert_eq!(zip.status, 200);
    assert_eq!(zip.header("content-disposition"), Some("attachment; filename=\"tone_trimmed.zip\""));
    assert_eq!(zip.header("content-length").unwrap().parse::<usize>().unwrap(), zip.body.len());
    let archive = save(&server.dir, "parts.zip", &zip.body);
    let listing = Command::new("unzip").arg("-t").arg(&archive).output().expect("unzip on PATH");
    let listing = String::from_utf8_lossy(&listing.stdout);
    assert!(listing.contains("tone_part1.wav") && listing.contains("tone_part2.wav") && listing.contains("No errors"), "{listing}");
    let extracted = server.dir.join("fixtures").join("unzipped");
    assert!(Command::new("unzip").arg("-q").arg(&archive).arg("-d").arg(&extracted).status().unwrap().success());
    assert_eq!(std::fs::read(extracted.join("tone_part1.wav")).unwrap(), part1.body);
    assert!((duration(&extracted.join("tone_part2.wav")) - 2.5).abs() < 0.05);

    server.wait_until_no_files("trim_").await;
    assert_eq!(server.get(&format!("/trim-audio/progress?sessionId={id}")).await.json(), json!({ "percent": 0, "status": "expired" }));
    assert_eq!(server.get(&format!("/trim-audio/download?sessionId={id}&part=2")).await.status, 410);
}

#[tokio::test]
async fn trim_video_reencodes_off_a_keyframe_and_copies_on_one_without_duplicate_frames() {
    let server = start("trimvideo").await;
    let clip = clip_mp4(&server.dir, 3, "160x120");
    let source_frames = video_packets(&clip);
    for (cut, label) in [(1.5, "off keyframe"), (1.0, "on keyframe")] {
        let up = server.upload("trim-audio", &clip, "video/mp4").await;
        let id = s(&up["sessionId"]).to_string();
        assert_eq!(server.post("/trim-audio/trim", json!({ "sessionId": id, "trimPoint": cut })).await.status, 202);
        let done = server.poll("trim-audio", &id, |b| b["status"] != "processing").await;
        assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{label}: {done}");
        let mut parts = Vec::new();
        for part in [1, 2] {
            let reply = server.get(&format!("/trim-audio/download?sessionId={id}&part={part}")).await;
            assert_eq!((reply.status, reply.header("content-type")), (200, Some("video/mp4")));
            parts.push(save(&server.dir, &format!("cut{cut}_part{part}.mp4"), &reply.body));
        }
        let first = duration(&parts[0]);
        assert!((first - cut).abs() < 0.1, "{label}: part 1 lasts {first}s, cut at {cut}s");
        if cut == 1.0 {
            // FT-QA-05: the copied seam must not repeat the reorder delay's frames.
            let frames = video_packets(&parts[0]) + video_packets(&parts[1]);
            assert_eq!(frames, source_frames, "{label}: the two parts together hold each source frame once");
        }
    }
    server.wait_until_no_files("trim_").await;
}

#[tokio::test]
async fn convert_audio_answers_when_done_forces_opus_to_48k_and_cleans_up_after_download() {
    let server = start("convaudio").await;
    let wav = tone_wav(&server.dir);
    let id = s(&server.upload("convert-audio", &wav, "audio/wav").await["sessionId"]).to_string();
    let same = server
        .post("/convert-audio/convert", json!({ "sessionId": id, "outputFormat": "wav", "sampleRate": 44100, "channels": 2 }))
        .await;
    assert_eq!((same.status, same.json()["error"].clone()), (400, json!("Output format must be different from input format.")));
    let mp3 = server
        .post(
            "/convert-audio/convert",
            json!({ "sessionId": id, "outputFormat": "mp3", "bitrate": 128, "sampleRate": 44100, "channels": 1 }),
        )
        .await;
    assert_eq!(mp3.status, 200, "{}", String::from_utf8_lossy(&mp3.body));
    let body = mp3.json();
    assert_eq!((body["success"].clone(), s(&body["filename"]), body["effectiveSampleRate"].clone()), (json!(true), "tone.mp3", json!(44100)));
    let opus = server
        .post(
            "/convert-audio/convert",
            json!({ "sessionId": id, "outputFormat": "opus", "bitrate": 64, "sampleRate": 8000, "channels": 2 }),
        )
        .await;
    assert_eq!((opus.status, opus.json()["effectiveSampleRate"].clone()), (200, json!(48000)));
    assert_eq!(server.get(&format!("/convert-audio/progress?sessionId={id}")).await.json(), json!({ "progress": 100 }));
    let download = server.get(&format!("/convert-audio/download?sessionId={id}")).await;
    assert_eq!((download.status, download.header("content-type")), (200, Some("audio/opus")));
    let path = save(&server.dir, "tone.opus", &download.body);
    assert_eq!(probe(&path, "a:0", "stream=codec_name"), "opus");
    assert_eq!(probe(&path, "a:0", "stream=sample_rate"), "48000");
    // Cleanup fences all workers and removes this session's superseded outputs
    // too, not only the final output path recorded in the session.
    server.wait_until_no_files("conv_").await;
    let gone = server.get(&format!("/convert-audio/progress?sessionId={id}")).await;
    assert_eq!((gone.status, gone.json()), (410, json!({ "error": "Session not found or expired." })));
}

#[tokio::test]
async fn reduce_denoise_and_extract_run_detached_and_poll_to_their_outputs() {
    let server = start("audio").await;
    let wav = tone_wav(&server.dir);
    let mp3 = media(&server.dir, "tone.mp3", &["-i", wav.to_str().unwrap(), "-c:a", "libmp3lame", "-b:a", "192k"]);

    let up = server.upload("reduce-audio", &mp3, "audio/mpeg").await;
    let id = s(&up["sessionId"]).to_string();
    assert_eq!((up["sampleRate"].clone(), up["channels"].clone()), (json!(44100), json!(2)));
    let accepted = server.post("/reduce-audio/reduce", json!({ "sessionId": id, "quality": "low" })).await;
    assert_eq!((accepted.status, accepted.json()["filename"].clone()), (202, json!("tone_reduced.mp3")));
    let done = server.poll("reduce-audio", &id, |b| b["status"] != "processing").await;
    assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{done}");
    let reduced = server.get(&format!("/reduce-audio/download?sessionId={id}")).await;
    let reduced = save(&server.dir, "reduced.mp3", &reduced.body);
    assert_eq!((probe(&reduced, "a:0", "stream=sample_rate"), probe(&reduced, "a:0", "stream=channels")), ("22050".into(), "1".into()));

    let up = server.upload("remove-noise", &wav, "audio/wav").await;
    let id = s(&up["sessionId"]).to_string();
    let accepted = server.post("/remove-noise/denoise", json!({ "sessionId": id, "level": "medium" })).await;
    assert_eq!((accepted.status, accepted.json()["outputFilename"].clone()), (202, json!("tone_clean.wav")));
    let done = server.poll("remove-noise", &id, |b| b["status"] == "done" || b["status"] == "error").await;
    assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{done}");
    assert_eq!(done["outputFilename"], "tone_clean.wav", "{done}");
    let cleaned = server.get(&format!("/remove-noise/download?sessionId={id}")).await;
    assert_eq!((cleaned.status, cleaned.header("content-type")), (200, Some("audio/wav")));
    assert!((duration(&save(&server.dir, "clean.wav", &cleaned.body)) - 4.0).abs() < 0.05);

    let clip = clip_mp4(&server.dir, 3, "160x120");
    let up = server.upload("video-to-audio", &clip, "video/mp4").await;
    let id = s(&up["sessionId"]).to_string();
    assert_eq!((up["hasThumbnail"].clone(), s(&up["audioCodec"])), (json!(true), "aac"));
    let thumb = server.get(&format!("/video-to-audio/thumbnail?sessionId={id}")).await;
    assert_eq!((thumb.status, thumb.header("content-type")), (200, Some("image/png")));
    assert!(thumb.body.starts_with(b"\x89PNG"));
    let early = server.get(&format!("/video-to-audio/download?sessionId={id}")).await;
    assert_eq!(early.status, 425);
    let wav_bitrate = server
        .post("/video-to-audio/extract", json!({ "sessionId": id, "outputFormat": "wav", "bitrate": 128, "sampleRate": 44100, "channels": 2 }))
        .await;
    assert_eq!(wav_bitrate.status, 400);
    let accepted = server
        .post("/video-to-audio/extract", json!({ "sessionId": id, "outputFormat": "mp3", "bitrate": 128, "sampleRate": 44100, "channels": 2 }))
        .await;
    assert_eq!(accepted.status, 202);
    let done = server.poll("video-to-audio", &id, |b| b["status"] == "done" || b["status"] == "error").await;
    assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{done}");
    assert_eq!(done["outputExtension"], "mp3", "{done}");
    let audio = server.get(&format!("/video-to-audio/download?sessionId={id}")).await;
    assert_eq!(audio.header("content-type"), Some("audio/mpeg"));
    assert_eq!(probe(&save(&server.dir, "extracted.mp3", &audio.body), "a:0", "stream=codec_name"), "mp3");

    let silent = media(&server.dir, "silent.mp4", &["-f", "lavfi", "-i", "testsrc=size=160x120:rate=25:duration=1", "-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    let data = std::fs::read(&silent).unwrap();
    let (content_type, body) = multipart("file", "silent.mp4", "video/mp4", &data);
    let refused = server
        .send(server.http.post(format!("{}/video-to-audio/upload", server.base)).header("content-type", content_type).body(body))
        .await;
    assert_eq!((refused.status, refused.json()), (422, json!({ "error": "This video has no audio track. Nothing to extract." })));

    for prefix in ["redaud_", "rmnoise_", "v2audio_"] {
        server.wait_until_no_files(prefix).await;
    }
}

#[tokio::test]
async fn compress_and_convert_video_report_results_and_refuse_a_second_concurrent_job() {
    let server = start("video").await;
    let clip = clip_mp4(&server.dir, 20, "640x360");

    let up = server.upload("compress-video", &clip, "video/mp4").await;
    let id = s(&up["sessionId"]).to_string();
    assert!(s(&up["thumbnailBase64"]).starts_with("data:image/jpeg;base64,/9j/"), "a JPEG starts with FFD8FF");
    assert_eq!((up["width"].clone(), up["height"].clone(), up["fps"].clone()), (json!(640), json!(360), json!(25)));
    let thumb = server.get(&format!("/compress-video/thumbnail?sessionId={id}")).await;
    assert_eq!((thumb.status, thumb.header("content-type")), (200, Some("image/jpeg")));
    let thumbnail = save(&server.dir, "compression-thumbnail.jpg", &thumb.body);
    assert_eq!(probe(&thumbnail, "v:0", "stream=codec_name"), "mjpeg");
    let accepted = server.post("/compress-video/compress", json!({ "sessionId": id, "quality": "low" })).await;
    assert_eq!((accepted.status, accepted.json()), (202, json!({ "accepted": true, "sessionId": id })));
    let busy = server.post("/compress-video/compress", json!({ "sessionId": id, "quality": "high" })).await;
    assert_eq!((busy.status, busy.json()), (409, json!({ "error": "Compression already in progress." })));
    let done = server.poll("compress-video", &id, |b| b["status"] == "done" || b["status"] == "error").await;
    assert_eq!((done["status"].clone(), done["progress"].clone()), (json!("done"), json!(100)), "{done}");
    assert_eq!(
        (done["result"]["filename"].clone(), done["result"]["outputWidth"].clone(), done["result"]["outputHeight"].clone()),
        (json!("clip20_640x360_compressed.mp4"), json!(640), json!(360)),
        "{done}"
    );
    let compressed = server.get(&format!("/compress-video/download?sessionId={id}")).await;
    assert_eq!(compressed.header("content-type"), Some("video/mp4"));
    let compressed = save(&server.dir, "compressed.mp4", &compressed.body);
    assert_eq!(probe(&compressed, "a:0", "stream=channels"), "1", "low quality is mono audio");

    let up = server.upload("convert-video", &clip, "video/mp4").await;
    let id = s(&up["sessionId"]).to_string();
    assert_eq!((s(&up["resolution"]), up["fps"].clone(), up["hasThumbnail"].clone()), ("640x360", json!(25), json!(false)));
    let options = json!({
        "sessionId": id, "outputFormat": "mkv", "videoCodec": "libx264", "resolution": "854x480", "fps": "original",
        "videoBitrate": 800, "audioBitrate": 128, "sampleRate": 48000, "channels": 2,
    });
    assert_eq!(server.post("/convert-video/convert", options.clone()).await.status, 202);
    assert_eq!(server.post("/convert-video/convert", options).await.status, 409);
    let done = server.poll("convert-video", &id, |b| b["status"] == "done" || b["status"] == "failed").await;
    assert_eq!((done["status"].clone(), done["percent"].clone()), (json!("done"), json!(100)), "{done}");
    assert_eq!((done["outputResolution"].clone(), done["outputFps"].clone()), (json!("854x480"), json!(25)), "{done}");
    let thumb = server.get(&format!("/convert-video/thumbnail?sessionId={id}")).await;
    assert_eq!((thumb.status, thumb.header("content-type")), (200, Some("image/png")), "the background thumbnail has landed");
    let thumbnail = save(&server.dir, "conversion-thumbnail.png", &thumb.body);
    assert_eq!(probe(&thumbnail, "v:0", "stream=codec_name"), "png");
    let converted = server.get(&format!("/convert-video/download?sessionId={id}")).await;
    assert_eq!(converted.header("content-type"), Some("video/x-matroska"));
    let converted = save(&server.dir, "converted.mkv", &converted.body);
    assert_eq!(probe(&converted, "v:0", "stream=width"), "854");
    for prefix in ["compvid_", "convvideo_"] {
        server.wait_until_no_files(prefix).await;
    }
}

#[tokio::test]
async fn the_processing_allowance_is_per_family_and_address_and_charges_refused_options() {
    let server = start("allowance").await;
    let id = uuid::Uuid::new_v4().to_string();
    let from = |ip: &str, family: &str, body: Value| {
        server
            .http
            .post(format!("{}/{family}", server.base))
            .header("content-type", "application/json")
            .header("x-forwarded-for", format!("6.6.6.6, {ip}"))
            .body(body.to_string())
    };
    for _ in 0..10 {
        let reply = server.send(from("1.1.1.1", "convert-video/convert", json!({ "sessionId": id, "outputFormat": "webm" }))).await;
        assert_eq!((reply.status, reply.json()), (400, json!({ "error": "Invalid output format." })));
    }
    let limited = server.send(from("1.1.1.1", "convert-video/convert", json!({ "sessionId": id, "outputFormat": "mp4" }))).await;
    assert_eq!((limited.status, limited.json()), (429, json!({ "error": "Too many requests. Please wait before trying again." })));
    let other_address = server.send(from("2.2.2.2", "convert-video/convert", json!({ "sessionId": id }))).await;
    assert_eq!(other_address.status, 400, "the spoofable leftmost entry is not the key");
    let other_family = server.send(from("1.1.1.1", "compress-video/compress", json!({ "sessionId": id, "quality": "low" }))).await;
    assert_eq!((other_family.status, other_family.json()), (410, json!({ "error": "Session expired. Please upload your file again." })));
    // A malformed session id is refused before the allowance is charged.
    for _ in 0..12 {
        let reply = server.send(from("3.3.3.3", "convert-video/convert", json!({ "sessionId": "../../etc/passwd" }))).await;
        assert_eq!(reply.status, 400);
    }
}

#[tokio::test]
async fn session_ids_that_are_not_server_minted_never_reach_the_filesystem() {
    let server = start("ids").await;
    let wav = tone_wav(&server.dir);
    let id = s(&server.upload("convert-audio", &wav, "audio/wav").await["sessionId"]).to_string();
    for forged in ["../../../etc/passwd", "%2e%2e%2fetc", &format!("{id}/../x")] {
        let reply = server.get(&format!("/convert-audio/download?sessionId={forged}")).await;
        assert_eq!(reply.status, 400, "{forged}");
    }
    let upper = server.get(&format!("/convert-audio/progress?sessionId={}", id.to_uppercase())).await;
    assert_eq!(upper.status, 410, "a well-formed id that was never issued is unknown");
    let invalidated = server.json(reqwest::Method::DELETE, "/convert-audio/invalidate", json!({ "sessionId": id })).await;
    assert_eq!((invalidated.status, invalidated.json()), (200, json!({ "success": true })));
    assert!(server.files_with_prefix("conv_").is_empty());
}

#[tokio::test]
async fn cleanup_purges_sessions_past_their_ttl_and_orphans_by_age_and_prefix() {
    let server = start("purge").await;
    let wav = tone_wav(&server.dir);
    let convert = s(&server.upload("convert-audio", &wav, "audio/wav").await["sessionId"]).to_string();
    let reduce = s(&server.upload("reduce-audio", &wav, "audio/wav").await["sessionId"]).to_string();
    let now = SystemTime::now();
    let old = server.dir.join("trim_left_by_a_crash.wav");
    std::fs::write(&old, b"x").unwrap();
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(now - Duration::from_secs(2 * 3600)).unwrap();
    let unrelated = server.dir.join("someone_else.wav");
    std::fs::write(&unrelated, b"x").unwrap();
    std::fs::File::options().write(true).open(&unrelated).unwrap().set_modified(now - Duration::from_secs(9 * 3600)).unwrap();

    assert_eq!(server.processor.purge(now).await, Purged { sessions: 0, orphans: 1 });
    assert!(!old.exists() && unrelated.exists());
    // One hour is still inside the window; just past it, convert-audio's session goes.
    assert_eq!(server.processor.purge(now + Duration::from_secs(3600) - Duration::from_secs(5)).await, Purged::default());
    let past_hour = server.processor.purge(now + Duration::from_secs(3601)).await;
    assert_eq!(past_hour.sessions, 1);
    assert_eq!(server.processor.session_count(Family::ConvertAudio), 0);
    assert_eq!(server.processor.session_count(Family::ReduceAudio), 1);
    assert!(server.files_with_prefix("conv_").is_empty());
    assert_eq!(server.get(&format!("/convert-audio/progress?sessionId={convert}")).await.status, 410);
    assert_eq!(server.get(&format!("/reduce-audio/progress?sessionId={reduce}")).await.json()["status"], "processing");
    assert_eq!(server.processor.purge(now + Duration::from_secs(2 * 3600 + 1)).await.sessions, 1);
    assert!(server.files_with_prefix("redaud_").is_empty());
    assert!(unrelated.exists());
}

struct Binary {
    child: std::process::Child,
    base: String,
}

impl Drop for Binary {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn spawn_binary(dir: &Path, port: u16) -> Binary {
    let child = Command::new(env!("CARGO_BIN_EXE_transgate-processor"))
        .env("PORT", port.to_string())
        .env("TMPDIR", dir)
        .env("ALLOWED_ORIGINS", ORIGIN)
        .env("FFMPEG_THREADS", "1")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let http = client();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(r) = http.get(format!("{base}/health")).send().await && r.status() == 200 {
            break;
        }
        assert!(Instant::now() < deadline, "the binary never became healthy");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Binary { child, base }
}

#[tokio::test]
async fn the_deployable_binary_loses_sessions_on_restart_keeps_their_files_and_stops_on_sigterm() {
    let dir = work_dir("binary");
    let port = free_port();
    let mut first = spawn_binary(&dir, port).await;
    let wav = tone_wav(&dir);
    let server = Server {
        base: first.base.clone(),
        processor: Processor::new(Config::from_lookup(|_| None, dir.clone()).unwrap()),
        dir: dir.clone(),
        http: client(),
    };
    let id = s(&server.upload("convert-audio", &wav, "audio/wav").await["sessionId"]).to_string();
    assert_eq!(server.get(&format!("/convert-audio/progress?sessionId={id}")).await.json(), json!({ "progress": 0 }));
    first.child.kill().unwrap();
    first.child.wait().unwrap();

    let mut second = spawn_binary(&dir, port).await;
    let lost = server.get(&format!("/convert-audio/progress?sessionId={id}")).await;
    assert_eq!((lost.status, lost.json()), (410, json!({ "error": "Session not found or expired." })));
    assert_eq!(server.files_with_prefix("conv_").len(), 1, "the upload is an orphan until the periodic purge");

    let terminated = Command::new("kill").args(["-TERM", &second.child.id().to_string()]).status().unwrap();
    assert!(terminated.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = second.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "SIGTERM did not stop the service");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(status.success(), "{status:?}");
}

#[test]
fn the_binary_refuses_to_start_on_an_invalid_port_instead_of_moving_to_8080() {
    let dir = work_dir("badport");
    let out = Command::new(env!("CARGO_BIN_EXE_transgate-processor"))
        .env("PORT", "80O0")
        .env("TMPDIR", &dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("PORT must be a port number from 1 to 65535, got \\\"80O0\\\""), "{stderr}");
    std::fs::remove_dir_all(&dir).unwrap();
}
