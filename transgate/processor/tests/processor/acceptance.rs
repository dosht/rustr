//! Additional backend acceptance scenarios. Gates delay real FFmpeg dispatch
//! through the existing FFMPEG_PATH configuration; they never fake media output.

use super::*;
use std::os::unix::fs::PermissionsExt;

// Expectations are literal consumer contracts, not derived from the policy
// being tested. Every row is exercised over HTTP below.
const FAMILIES: [(Family, &str, &str, u32, u64); 7] = [
    (Family::TrimAudio, "trim-audio", "trim_", 20, 3600),
    (Family::ConvertAudio, "convert-audio", "conv_", 20, 3600),
    (Family::ReduceAudio, "reduce-audio", "redaud_", 20, 7200),
    (Family::RemoveNoise, "remove-noise", "rmnoise_", 20, 7200),
    (Family::VideoToAudio, "video-to-audio", "v2audio_", 20, 7200),
    (Family::CompressVideo, "compress-video", "compvid_", 10, 7200),
    (Family::ConvertVideo, "convert-video", "convvideo_", 10, 3600),
];

fn action(family: &str) -> &str {
    match family {
        "trim-audio" => "trim",
        "reduce-audio" => "reduce",
        "remove-noise" => "denoise",
        "video-to-audio" => "extract",
        "compress-video" => "compress",
        _ => "convert",
    }
}

fn options(family: &str, id: &str) -> Value {
    match family {
        "trim-audio" => json!({ "sessionId": id, "trimPoint": 1.5 }),
        "reduce-audio" | "compress-video" => json!({ "sessionId": id, "quality": "low" }),
        "remove-noise" => json!({ "sessionId": id, "level": "low" }),
        "convert-video" => json!({
            "sessionId": id, "outputFormat": "mkv", "videoCodec": "libx264", "resolution": "original",
            "fps": "original", "videoBitrate": 300, "audioBitrate": 64, "sampleRate": 44100, "channels": 1,
        }),
        _ => json!({ "sessionId": id, "outputFormat": "mp3", "bitrate": 64, "sampleRate": 44100, "channels": 1 }),
    }
}

fn bad_options(family: &str, id: &str) -> Value {
    let mut body = options(family, id);
    match family {
        "trim-audio" => body["trimPoint"] = json!(0), // numeric, but outside this live session's duration
        "reduce-audio" | "compress-video" => body["quality"] = json!("invalid"),
        "remove-noise" => body["level"] = json!("invalid"),
        _ => body["outputFormat"] = json!("invalid"),
    }
    body
}

fn input<'a>(family: &str, audio: &'a Path, video: &'a Path) -> (&'a Path, &'static str) {
    if matches!(family, "video-to-audio" | "compress-video" | "convert-video") {
        (video, "video/mp4")
    } else {
        (audio, "audio/wav")
    }
}

async fn process_from(server: &Server, family: &str, body: Value, ip: &str) -> Reply {
    server.send(server.http.post(format!("{}/{}/{}", server.base, family, action(family)))
        .header("content-type", "application/json").header("x-forwarded-for", ip).body(body.to_string())).await
}

async fn upload_from(server: &Server, family: &str, file: &Path, mime: &str, ip: &str) -> Value {
    let (content_type, body) = multipart("file", file.file_name().unwrap().to_str().unwrap(), mime, &std::fs::read(file).unwrap());
    let reply = server.send(server.http.post(format!("{}/{family}/upload", server.base))
        .header("x-forwarded-for", ip).header("content-type", content_type).body(body)).await;
    assert_eq!(reply.status, 200, "{family}: {}", String::from_utf8_lossy(&reply.body));
    let body = reply.json();
    wait_for_upload_side_work(server, family, s(&body["sessionId"])).await;
    body
}

async fn invalidate(server: &Server, family: &str, id: &str) {
    let reply = server.json(reqwest::Method::DELETE, &format!("/{family}/invalidate"), json!({ "sessionId": id })).await;
    assert_eq!((reply.status, reply.json()), (200, json!({ "success": true })), "{family}");
}

// Isolate idle-session cleanup from the separately covered active-worker race.
// convert-video alone returns its upload before its thumbnail worker finishes.
async fn wait_for_upload_side_work(server: &Server, family: &str, id: &str) {
    if family != "convert-video" { return; }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reply = server.get(&format!("/{family}/thumbnail?sessionId={id}")).await;
        if reply.status == 200 {
            assert_eq!(reply.header("content-type"), Some("image/png"));
            return;
        }
        assert!(Instant::now() < deadline, "thumbnail did not finish: {}", reply.json());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn assert_expired(server: &Server, family: &str, id: &str) {
    let progress = server.get(&format!("/{family}/progress?sessionId={id}")).await;
    match family {
        "convert-audio" => assert_eq!((progress.status, progress.json()), (410, json!({ "error": "Session not found or expired." }))),
        "compress-video" => assert_eq!((progress.status, progress.json()), (410, json!({ "error": "Session expired." }))),
        "reduce-audio" => assert_eq!((progress.status, progress.json()), (200, json!({ "progress": 0, "percent": 0, "status": "expired" }))),
        _ => assert_eq!((progress.status, progress.json()), (200, json!({ "percent": 0, "status": "expired" }))),
    }
    let part = if family == "trim-audio" { "&part=1" } else { "" };
    assert_eq!(server.get(&format!("/{family}/download?sessionId={id}{part}")).await.status, 410, "{family}");
}

async fn done(server: &Server, family: &str, id: &str) -> Value {
    let body = server.poll(family, id, |v| matches!(v["status"].as_str(), Some("done" | "failed" | "error"))).await;
    assert_eq!(body["status"], "done", "{family}: {body}");
    let key = if family == "compress-video" { "progress" } else { "percent" };
    assert_eq!(body[key], 100, "{family}: {body}");
    if family == "reduce-audio" {
        assert_eq!(body["progress"], 100, "{body}");
    }
    body
}

async fn download(server: &Server, family: &str, id: &str, name: &str) -> PathBuf {
    let reply = server.get(&format!("/{family}/download?sessionId={id}")).await;
    assert_eq!(reply.status, 200, "{family}: {}", String::from_utf8_lossy(&reply.body));
    save(&server.dir, name, &reply.body)
}

fn limited(family: &str, reply: Reply) {
    let message = if matches!(family, "trim-audio" | "convert-audio") {
        "Too many requests. Please wait a moment before trying again."
    } else {
        "Too many requests. Please wait before trying again."
    };
    assert_eq!((reply.status, reply.json()), (429, json!({ "error": message })), "{family}");
}

/// The child is the wrapper until released, then execs real FFmpeg with the
/// untouched arguments and same PID. Probes/waveforms pass through; the
/// thumbnail gate is opt-in. A marker appears only after the child starts.
struct DispatchGate {
    dir: PathBuf,
}

impl DispatchGate {
    fn install(config: &mut Config) -> Self {
        let dir = config.work_dir.join("dispatch-gates");
        std::fs::create_dir(&dir).unwrap();
        let script = dir.join("ffmpeg-gate");
        let quote = |p: &Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
        std::fs::write(&script, format!(r#"#!/bin/sh
control={}
output=
for arg do
    case "$arg" in
        *_output.*|*_part1.*|*_part2.*) output="$arg" ;;
        *_thumb.png) if [ -f "$control/gate-thumbnails" ]; then output="$arg"; fi ;;
    esac
done
if [ -n "$output" ]; then
    marker="$control/${{output##*/}}.$$.pid"
    printf '%s\n' "$$" > "$marker"
    while [ ! -f "$control/release" ]; do sleep 0.02; done
fi
exec ffmpeg "$@"
"#, quote(&dir))).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        config.ffmpeg = script.to_str().unwrap().to_owned();
        Self { dir }
    }

    fn hold_thumbnails(&self) {
        std::fs::write(self.dir.join("gate-thumbnails"), b"hold").unwrap();
    }

    fn markers(&self, id: &str) -> Vec<PathBuf> {
        std::fs::read_dir(&self.dir).unwrap().map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_str().unwrap().contains(id) && p.extension().is_some_and(|e| e == "pid")).collect()
    }

    async fn started(&self, id: &str, count: usize) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let paths = self.markers(id);
            let pids: Vec<u32> = paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()?.trim().parse().ok()).collect();
            if pids.len() == count {
                return pids;
            }
            assert!(Instant::now() < deadline, "processing children did not reach gate: {id}, {paths:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn release(&self) {
        std::fs::write(self.dir.join("release"), b"go").unwrap();
    }
}

impl Drop for DispatchGate {
    fn drop(&mut self) {
        // Never leave a gated worker behind when an assertion fails. Check the
        // unique work-dir marker in argv before sending a signal (no PID reuse).
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let owner = self.dir.parent().unwrap().to_string_lossy();
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "pid") && let Ok(text) = std::fs::read_to_string(entry.path()) && let Ok(pid) = text.trim().parse::<u32>() && std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|v| String::from_utf8_lossy(&v).contains(owner.as_ref())) {
                let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
            }
        }
    }
}

#[tokio::test]
async fn every_family_invalidates_its_live_session_and_only_its_own_files() {
    let server = start("invalidate-all").await;
    let audio = tone_wav(&server.dir);
    let video = clip_mp4(&server.dir, 2, "160x120");
    for (_, family, prefix, _, _) in FAMILIES {
        let (file, mime) = input(family, &audio, &video);
        let first = server.upload(family, file, mime).await;
        let second = server.upload(family, file, mime).await;
        let (id, other) = (s(&first["sessionId"]), s(&second["sessionId"]));
        wait_for_upload_side_work(&server, family, id).await;
        wait_for_upload_side_work(&server, family, other).await;
        let reply = process_from(&server, family, options(family, id), "10.40.0.1").await;
        if family == "convert-audio" {
            assert_eq!(reply.status, 200, "{}", reply.json());
            assert_eq!(server.get(&format!("/{family}/progress?sessionId={id}")).await.json(), json!({ "progress": 100 }));
        } else {
            assert_eq!(reply.status, 202);
            done(&server, family, id).await;
        }
        let own: Vec<_> = server.files_with_prefix(prefix).into_iter().filter(|n| n.contains(id)).collect();
        let other_files: Vec<_> = server.files_with_prefix(prefix).into_iter().filter(|n| n.contains(other)).collect();
        assert!(own.iter().any(|n| n.contains("_upload.")), "{family}: {own:?}");
        assert!(own.iter().any(|n| n.contains("_output.") || n.contains("_part1.")), "{family}: missing completed output");
        invalidate(&server, family, id).await;
        assert_expired(&server, family, id).await;
        for name in own {
            assert!(!server.dir.join(name).exists(), "{family}: invalidated file remains");
        }
        for name in other_files {
            assert!(server.dir.join(name).exists(), "{family}: another session's file was removed");
        }
        let other_progress = server.get(&format!("/{family}/progress?sessionId={other}")).await;
        assert_eq!(other_progress.status, 200);
        assert_ne!(other_progress.json()["status"], "expired");
        invalidate(&server, family, other).await;
        server.wait_until_no_files(prefix).await;
    }
}

#[tokio::test]
async fn invalidating_in_flight_processing_reaps_children_and_cannot_publish_late_files() {
    for (_, family, prefix, _, _) in FAMILIES {
        let mut gate = None;
        let server = start_with("invalidate-in-flight", |c| gate = Some(DispatchGate::install(c))).await;
        let gate = gate.unwrap();
        let audio = tone_wav(&server.dir);
        let video = clip_mp4(&server.dir, 2, "160x120");
        let (file, mime) = input(family, &audio, &video);
        let up = server.upload(family, file, mime).await;
        let other = server.upload(family, file, mime).await;
        let (id, other_id) = (s(&up["sessionId"]), s(&other["sessionId"]));
        wait_for_upload_side_work(&server, family, id).await;
        wait_for_upload_side_work(&server, family, other_id).await;
        // convert-audio holds its HTTP response until the worker completes;
        // keep it alive concurrently with DELETE, not by dropping the request.
        let processing = tokio::spawn(request_owned(&server, family, options(family, id), "10.10.0.1"));
        let other_processing = tokio::spawn(request_owned(&server, family, options(family, other_id), "10.10.0.2"));
        let count = if family == "trim-audio" { 2 } else { 1 };
        let pids = gate.started(id, count).await;
        let other_pids = gate.started(other_id, count).await;
        // Two callers must share completion, not let the second DELETE return
        // while the first still owns uncancelled work.
        let checked_delete = || async {
            invalidate(&server, family, id).await;
            assert!(pids.iter().all(|pid| !Path::new(&format!("/proc/{pid}")).exists()),
                "{family}: DELETE returned before owned children were reaped: {pids:?}");
        };
        let ((), ()) = tokio::join!(checked_delete(), checked_delete());
        assert_expired(&server, family, id).await;
        assert!(other_pids.iter().all(|pid| Path::new(&format!("/proc/{pid}")).exists()), "{family}: another session was killed");
        assert!(server.files_with_prefix(prefix).iter().all(|n| !n.contains(id)), "{family}: owned files survived invalidation");
        gate.release();
        let reply = processing.await.unwrap();
        assert_eq!(reply.status, if family == "convert-audio" { 500 } else { 202 });
        let other_reply = other_processing.await.unwrap();
        assert_eq!(other_reply.status, if family == "convert-audio" { 200 } else { 202 });
        if family == "convert-audio" {
            assert_eq!(server.get(&format!("/{family}/progress?sessionId={other_id}")).await.json(), json!({ "progress": 100 }));
        } else {
            done(&server, family, other_id).await;
        }
        assert!(server.files_with_prefix(prefix).iter().all(|n| !n.contains(id)), "{family}: files appeared after invalidation");
        invalidate(&server, family, other_id).await;
        server.wait_until_no_files(prefix).await;
    }
}

// Own the request arguments so a synchronous conversion can run alongside
// invalidation without requiring Server (which owns its temp directory) in Arc.
fn request_owned(server: &Server, family: &str, body: Value, ip: &str) -> impl Future<Output = Reply> + Send + use<> {
    let request = server.http.post(format!("{}/{}/{}", server.base, family, action(family)))
        .header("content-type", "application/json").header("x-forwarded-for", ip).body(body.to_string());
    async move {
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await.unwrap().to_vec();
        Reply { status, headers, body }
    }
}

#[tokio::test]
async fn every_family_leaves_uploads_uncharged_and_charges_invalid_options_and_absent_sessions() {
    let server = start("quota-all").await;
    let audio = tone_wav(&server.dir);
    let video = clip_mp4(&server.dir, 2, "160x120");
    for (_, family, _, quota, _) in FAMILIES {
        let (file, mime) = input(family, &audio, &video);
        let up = upload_from(&server, family, file, mime, "10.20.0.1").await;
        let id = s(&up["sessionId"]);
        // Upload before processing plus upload after exhaustion must neither
        // consume an allowance nor be blocked by it.
        for _ in 0..quota {
            let reply = process_from(&server, family, bad_options(family, id), "10.20.0.1").await;
            assert_eq!(reply.status, 400, "{family}: {}", reply.json());
        }
        limited(family, process_from(&server, family, options(family, id), "10.20.0.1").await);
        let extra = upload_from(&server, family, file, mime, "10.20.0.1").await;
        invalidate(&server, family, s(&extra["sessionId"])).await;
        limited(family, process_from(&server, family, options(family, id), "10.20.0.1").await);

        let absent = uuid::Uuid::new_v4().to_string();
        for _ in 0..quota {
            let reply = process_from(&server, family, options(family, &absent), "10.20.0.2").await;
            assert_eq!(reply.status, 410, "{family}: {}", reply.json());
        }
        limited(family, process_from(&server, family, options(family, &absent), "10.20.0.2").await);
        // Independent family/address: this address was not exhausted by earlier rows.
        assert_eq!(process_from(&server, family, options(family, &absent), "10.20.0.3").await.status, 410);
        invalidate(&server, family, id).await;
    }
}

#[tokio::test]
async fn every_asynchronous_family_charges_busy_checks_without_starting_another_worker() {
    for (_, family, prefix, quota, _) in FAMILIES.into_iter().filter(|(_, f, _, _, _)| *f != "convert-audio") {
        let mut gate = None;
        let server = start_with("quota-busy", |c| gate = Some(DispatchGate::install(c))).await;
        let gate = gate.unwrap();
        let audio = tone_wav(&server.dir);
        let video = clip_mp4(&server.dir, 2, "160x120");
        let (file, mime) = input(family, &audio, &video);
        let up = server.upload(family, file, mime).await;
        let id = s(&up["sessionId"]);
        let body = options(family, id);
        assert_eq!(process_from(&server, family, body.clone(), "10.30.0.1").await.status, 202);
        let count = if family == "trim-audio" { 2 } else { 1 };
        gate.started(id, count).await;
        let message = match family {
            "trim-audio" => "A trim is already in progress for this file.",
            "reduce-audio" => "A reduction is already in progress for this file.",
            "remove-noise" => "Noise removal already in progress.",
            "video-to-audio" => "Extraction already in progress.",
            "compress-video" => "Compression already in progress.",
            _ => "Conversion already in progress.",
        };
        for _ in 1..quota {
            let reply = process_from(&server, family, body.clone(), "10.30.0.1").await;
            assert_eq!((reply.status, reply.json()), (409, json!({ "error": message })), "{family}");
        }
        limited(family, process_from(&server, family, body, "10.30.0.1").await);
        assert_eq!(gate.markers(id).len(), count, "{family}: a busy check dispatched another child");
        gate.release();
        done(&server, family, id).await;
        invalidate(&server, family, id).await;
        assert_expired(&server, family, id).await;
        server.wait_until_no_files(prefix).await;
    }
}

fn orphan(path: &Path, modified: SystemTime) {
    std::fs::write(path, b"orphan").unwrap();
    std::fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
}

#[tokio::test]
async fn every_family_purges_only_past_its_session_and_orphan_ttl() {
    for (kind, family, prefix, _, ttl) in FAMILIES {
        let server = start("ttl-all").await;
        let audio = tone_wav(&server.dir);
        let video = clip_mp4(&server.dir, 2, "160x120");
        let (file, mime) = input(family, &audio, &video);
        let before = SystemTime::now();
        let up = server.upload(family, file, mime).await;
        let after = SystemTime::now();
        wait_for_upload_side_work(&server, family, s(&up["sessionId"])).await;
        let id = s(&up["sessionId"]);
        let ttl = Duration::from_secs(ttl);
        // Exact, integral-second mtime boundaries are controlled independently
        // of the upload's creation time (no private session mutation).
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(after.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() + 1);
        let old = server.dir.join(format!("{prefix}orphan-old"));
        let edge = server.dir.join(format!("{prefix}orphan-boundary"));
        let young = server.dir.join(format!("{prefix}orphan-young"));
        let unrelated = server.dir.join("not-a-tool-file");
        orphan(&old, now - ttl - Duration::from_secs(1));
        orphan(&edge, now - ttl);
        orphan(&young, now - ttl + Duration::from_secs(1));
        orphan(&unrelated, now - 3 * ttl);
        assert_eq!(server.processor.purge(now).await, Purged { sessions: 0, orphans: 1 }, "{family}");
        assert!(!old.exists() && edge.exists() && young.exists() && unrelated.exists());
        assert_eq!(server.processor.purge(now + Duration::from_secs(1)).await, Purged { sessions: 0, orphans: 1 });
        assert!(!edge.exists() && young.exists());
        assert_eq!(server.processor.purge(now + Duration::from_secs(2)).await, Purged { sessions: 0, orphans: 1 });
        assert!(!young.exists() && unrelated.exists());

        // Upload creation lies between before/after: prove neither early
        // expiry nor retention past that bracket, without forging timestamps.
        assert_eq!(server.processor.purge(before + ttl).await.sessions, 0, "{family}: expired early");
        assert_eq!(server.processor.session_count(kind), 1);
        assert_eq!(server.processor.purge(after + ttl + Duration::from_nanos(1)).await.sessions, 1, "{family}: retained too long");
        assert_eq!(server.processor.session_count(kind), 0);
        assert_expired(&server, family, id).await;
        assert!(server.files_with_prefix(prefix).is_empty(), "{family}: TTL left session files");
        assert!(unrelated.exists());
    }
}

#[tokio::test]
async fn audio_conversion_encodes_each_distinct_codec_and_container_with_effective_options() {
    let server = start("audio-codecs").await;
    let wav = tone_wav(&server.dir);
    let mp3 = media(&server.dir, "input.mp3", &["-i", wav.to_str().unwrap(), "-c:a", "libmp3lame", "-b:a", "128k"]);
    // Eight outputs exercise different encoders/container flags, not a Cartesian
    // format x rate x bitrate x channels expansion.
    for (format, codec, container, bitrate, requested_rate, effective_rate, channels) in [
        ("mp3", "mp3", "mp3", Some(32), 8000, 8000, 1),
        ("aac", "aac", "aac", Some(320), 48000, 48000, 2),
        ("m4a", "aac", "mov,mp4", Some(96), 22050, 22050, 2),
        ("ogg", "vorbis", "ogg", Some(128), 44100, 44100, 2),
        ("opus", "opus", "ogg", Some(64), 8000, 48000, 1),
        ("flac", "flac", "flac", None, 96000, 96000, 1),
        ("wav", "pcm_s16le", "wav", None, 8000, 8000, 1),
        ("wma", "wmav2", "asf", Some(96), 44100, 44100, 2),
    ] {
        let (source, mime) = if format == "wav" { (&mp3, "audio/mpeg") } else { (&wav, "audio/wav") };
        let upload = server.upload("convert-audio", source, mime).await;
        let id = s(&upload["sessionId"]);
        let reply = server.post("/convert-audio/convert", json!({
            "sessionId": id, "outputFormat": format, "bitrate": bitrate, "sampleRate": requested_rate, "channels": channels,
        })).await;
        assert_eq!(reply.status, 200, "{format}: {}", reply.json());
        assert_eq!(reply.json()["effectiveSampleRate"], effective_rate, "{format}");
        assert_eq!(server.get(&format!("/convert-audio/progress?sessionId={id}")).await.json(), json!({ "progress": 100 }));
        let output = download(&server, "convert-audio", id, &format!("converted.{format}")).await;
        assert_eq!(probe(&output, "a:0", "stream=codec_name"), codec);
        assert_eq!(probe(&output, "a:0", "stream=sample_rate"), effective_rate.to_string());
        assert_eq!(probe(&output, "a:0", "stream=channels"), channels.to_string());
        assert!(probe(&output, "a:0", "format=format_name").contains(container), "{format}: wrong container");
        server.wait_until_no_files("conv_").await;
        assert_expired(&server, "convert-audio", id).await;
    }
}

#[tokio::test]
async fn extraction_encodes_lossless_native_vorbis_and_forced_opus_rate_as_real_media() {
    let server = start("extraction-codecs").await;
    let video = clip_mp4(&server.dir, 2, "160x120");
    for (format, codec, bitrate, requested_rate, effective_rate, channels) in [
        ("mp3", "mp3", Some(32), 8000, 8000, 1),
        ("aac", "aac", Some(320), 48000, 48000, 2),
        ("wav", "pcm_s16le", None, 22050, 22050, 1),
        ("ogg", "vorbis", Some(128), 44100, 44100, 2),
        ("opus", "opus", Some(64), 8000, 48000, 1),
    ] {
        let upload = server.upload("video-to-audio", &video, "video/mp4").await;
        let id = s(&upload["sessionId"]);
        assert_eq!(server.post("/video-to-audio/extract", json!({
            "sessionId": id, "outputFormat": format, "bitrate": bitrate, "sampleRate": requested_rate, "channels": channels,
        })).await.status, 202);
        let terminal = done(&server, "video-to-audio", id).await;
        assert_eq!(terminal["outputExtension"], format);
        let output = download(&server, "video-to-audio", id, &format!("extracted.{format}")).await;
        assert_eq!(probe(&output, "a:0", "stream=codec_name"), codec);
        assert_eq!(probe(&output, "a:0", "stream=sample_rate"), effective_rate.to_string());
        assert_eq!(probe(&output, "a:0", "stream=channels"), channels.to_string());
        assert_eq!(probe(&output, "v:0", "stream=codec_name"), "", "extraction must not carry the video track");
        server.wait_until_no_files("v2audio_").await;
    }
}

#[tokio::test]
async fn reduction_encodes_lossless_aac_opus_and_mp3_presets_with_their_real_channels_and_rates() {
    let server = start("reduction-presets").await;
    let wav = tone_wav(&server.dir);
    for (format, source_codec, mime, quality, output_codec, rate, channels, bitrate) in [
        ("mp3", "libmp3lame", "audio/mpeg", "medium", "mp3", "44100", "2", Some("128000")),
        ("mp3", "libmp3lame", "audio/mpeg", "high", "mp3", "44100", "2", Some("192000")),
        ("flac", "flac", "audio/flac", "low", "flac", "22050", "1", None),
        // libopus signals a 48 kHz decode clock even when fed 24 kHz PCM.
        ("opus", "libopus", "audio/opus", "low", "opus", "48000", "1", None),
        ("m4a", "aac", "audio/mp4", "high", "aac", "44100", "2", None),
    ] {
        let input = media(&server.dir, &format!("source.{format}"), &["-i", wav.to_str().unwrap(), "-c:a", source_codec]);
        let upload = server.upload("reduce-audio", &input, mime).await;
        let id = s(&upload["sessionId"]);
        assert_eq!(server.post("/reduce-audio/reduce", json!({ "sessionId": id, "quality": quality })).await.status, 202);
        let terminal = done(&server, "reduce-audio", id).await;
        assert_eq!(terminal["extension"], format);
        let output = download(&server, "reduce-audio", id, &format!("reduced-{quality}.{format}")).await;
        assert_eq!(probe(&output, "a:0", "stream=codec_name"), output_codec);
        assert_eq!(probe(&output, "a:0", "stream=sample_rate"), rate);
        assert_eq!(probe(&output, "a:0", "stream=channels"), channels);
        if let Some(bitrate) = bitrate {
            assert_eq!(probe(&output, "a:0", "stream=bit_rate"), bitrate, "{quality}");
        }
        server.wait_until_no_files("redaud_").await;
    }
}

#[tokio::test]
async fn denoise_encodes_audio_containers_and_preserves_video_for_video_inputs() {
    let server = start("denoise-containers").await;
    let wav = tone_wav(&server.dir);
    let mp4 = clip_mp4(&server.dir, 2, "160x120");
    let flac = media(&server.dir, "input.flac", &["-i", wav.to_str().unwrap(), "-c:a", "flac"]);
    let ogg = media(&server.dir, "input.ogg", &["-i", wav.to_str().unwrap(), "-c:a", "libvorbis"]);
    let webm = media(&server.dir, "input.webm", &[
        "-i", mp4.to_str().unwrap(), "-c:v", "libvpx-vp9", "-threads", "1", "-c:a", "libvorbis",
    ]);
    for (input, mime, level, extension, audio_codec, video_codec) in [
        (&wav, "audio/wav", "low", "wav", "pcm_s16le", None),
        (&flac, "audio/flac", "high", "flac", "flac", None),
        (&ogg, "audio/ogg", "medium", "ogg", "vorbis", None),
        (&mp4, "video/mp4", "high", "mp4", "aac", Some("h264")),
        (&webm, "video/webm", "low", "webm", "vorbis", Some("vp9")),
    ] {
        let upload = server.upload("remove-noise", input, mime).await;
        let id = s(&upload["sessionId"]);
        assert_eq!(server.post("/remove-noise/denoise", json!({ "sessionId": id, "level": level })).await.status, 202);
        let terminal = done(&server, "remove-noise", id).await;
        assert_eq!(terminal["outputExtension"], extension);
        let output = download(&server, "remove-noise", id, &format!("clean.{extension}")).await;
        assert_eq!(probe(&output, "a:0", "stream=codec_name"), audio_codec);
        if let Some(codec) = video_codec {
            assert_eq!(probe(&output, "v:0", "stream=codec_name"), codec);
            assert_eq!(video_packets(&output), video_packets(input), "denoise must retain every video packet");
            // -c:v copy must preserve the elementary stream, not just its count.
            let digest = |path: &Path| {
                let out = Command::new("ffmpeg").args(["-v", "error", "-i"]).arg(path)
                    .args(["-map", "0:v:0", "-c", "copy", "-f", "hash", "-hash", "sha256", "-"]).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
                out.stdout
            };
            assert_eq!(digest(&output), digest(input), "video track was re-encoded");
        }
        server.wait_until_no_files("rmnoise_").await;
    }
}

#[tokio::test]
async fn compression_encodes_all_containers_and_the_scale_quality_and_audio_branches() {
    let server = start("compression-containers").await;
    let small = clip_mp4(&server.dir, 2, "160x120");
    let tall = clip_mp4(&server.dir, 1, "160x800");
    for (format, quality, height, rate, channels, codec) in [
        ("mp4", "low", "720", "44100", "1", "h264"),
        ("mov", "medium", "120", "44100", "2", "h264"),
        ("avi", "high", "120", "48000", "2", "mpeg4"),
        ("mkv", "high", "120", "48000", "2", "h264"),
    ] {
        let source = if format == "mp4" { tall.clone() } else {
            media(&server.dir, &format!("source.{format}"), &[
                "-i", small.to_str().unwrap(), "-c:v", if format == "avi" { "mpeg4" } else { "libx264" },
                "-c:a", if format == "avi" { "libmp3lame" } else { "aac" },
            ])
        };
        let mime = match format { "mov" => "video/quicktime", "avi" => "video/x-msvideo", "mkv" => "video/x-matroska", _ => "video/mp4" };
        let upload = server.upload("compress-video", &source, mime).await;
        let id = s(&upload["sessionId"]);
        assert_eq!(server.post("/compress-video/compress", json!({ "sessionId": id, "quality": quality })).await.status, 202);
        let terminal = done(&server, "compress-video", id).await;
        assert_eq!(terminal["result"]["extension"], format);
        assert_eq!(terminal["result"]["outputHeight"].as_u64().unwrap().to_string(), height);
        let output = download(&server, "compress-video", id, &format!("compressed.{format}")).await;
        assert_eq!(probe(&output, "v:0", "stream=codec_name"), codec);
        assert_eq!(probe(&output, "v:0", "stream=height"), height);
        assert_eq!(probe(&output, "a:0", "stream=sample_rate"), rate);
        assert_eq!(probe(&output, "a:0", "stream=channels"), channels);
        server.wait_until_no_files("compvid_").await;
    }
}

#[tokio::test]
async fn video_conversion_encodes_four_codecs_containers_and_numeric_or_original_fps() {
    let server = start("video-codecs").await;
    let input = clip_mp4(&server.dir, 2, "160x120");
    for (format, encoder, codec, fps, rate, channels) in [
        ("mp4", "libx264", "h264", json!(1), 44100, 1),
        ("mov", "libx265", "hevc", json!("original"), 48000, 2),
        ("avi", "mpeg4", "mpeg4", json!(12), 44100, 1),
        ("mkv", "libvpx-vp9", "vp9", json!(30), 48000, 2),
    ] {
        let upload = server.upload("convert-video", &input, "video/mp4").await;
        let id = s(&upload["sessionId"]);
        assert_eq!(server.post("/convert-video/convert", json!({
            "sessionId": id, "outputFormat": format, "videoCodec": encoder, "resolution": "original", "fps": fps,
            "videoBitrate": 300, "audioBitrate": 64, "sampleRate": rate, "channels": channels,
        })).await.status, 202);
        let terminal = done(&server, "convert-video", id).await;
        let expected_fps = if fps == json!("original") { 25 } else { fps.as_u64().unwrap() };
        assert_eq!(terminal["outputFps"], expected_fps);
        assert_eq!(terminal["outputResolution"], "160x120");
        let output = download(&server, "convert-video", id, &format!("converted.{format}")).await;
        assert_eq!(probe(&output, "v:0", "stream=codec_name"), codec);
        assert_eq!(probe(&output, "v:0", "stream=width"), "160");
        assert_eq!(probe(&output, "v:0", "stream=r_frame_rate"), format!("{expected_fps}/1"));
        assert_eq!(probe(&output, "a:0", "stream=codec_name"), if format == "avi" { "mp3" } else { "aac" });
        assert_eq!(probe(&output, "a:0", "stream=sample_rate"), rate.to_string());
        assert_eq!(probe(&output, "a:0", "stream=channels"), channels.to_string());
        server.wait_until_no_files("convvideo_").await;
    }
}

#[tokio::test]
async fn cleanup_timer_waits_ten_minutes_then_sweeps_all_seven_orphan_prefixes() {
    let dir = work_dir("cleanup-timer");
    let processor = Processor::new(Config::from_lookup(|_| None, dir.clone()).unwrap());
    let old = SystemTime::now() - Duration::from_secs(3 * 3600);
    let files: Vec<_> = FAMILIES.iter().map(|(_, _, prefix, _, _)| dir.join(format!("{prefix}old-orphan"))).collect();
    for path in &files { orphan(path, old); }
    let unrelated = dir.join("unrelated-old-file");
    orphan(&unrelated, old);
    // Only the existing Tokio scheduler is paused. File ages remain genuine
    // SystemTime ages, avoiding a production clock abstraction or wall sleep.
    tokio::time::pause();
    let cleanup = processor.spawn_cleanup();
    tokio::task::yield_now().await; // install interval_at relative to paused start
    tokio::time::advance(Duration::from_secs(599)).await;
    tokio::task::yield_now().await;
    assert!(files.iter().all(|p| p.exists()), "timer ran before ten minutes");
    tokio::time::advance(Duration::from_secs(1)).await;
    // Resume before waiting on filesystem I/O; an idle paused runtime must not
    // auto-advance through further cleanup intervals while those tasks finish.
    tokio::time::resume();
    let deadline = Instant::now() + Duration::from_secs(10);
    while files.iter().any(|p| p.exists()) {
        assert!(Instant::now() < deadline, "ten-minute callback failed to sweep all seven families");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(unrelated.exists(), "timer crossed the owned-prefix boundary");
    cleanup.abort();
    assert!(cleanup.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn invalidating_video_upload_reaps_its_background_thumbnail_before_returning() {
    let mut gate = None;
    let server = start_with("invalidate-thumbnail", |c| {
        let installed = DispatchGate::install(c);
        installed.hold_thumbnails();
        gate = Some(installed);
    }).await;
    let gate = gate.unwrap();
    let video = clip_mp4(&server.dir, 2, "160x120");
    let upload = server.upload("convert-video", &video, "video/mp4").await;
    assert_eq!(upload["hasThumbnail"], false);
    let id = s(&upload["sessionId"]);
    let pids = gate.started(id, 1).await;
    invalidate(&server, "convert-video", id).await;
    assert!(pids.iter().all(|pid| !Path::new(&format!("/proc/{pid}")).exists()), "thumbnail child outlived invalidation");
    assert_expired(&server, "convert-video", id).await;
    gate.release();
    assert!(server.files_with_prefix("convvideo_").is_empty(), "a late thumbnail survived invalidation");
}
