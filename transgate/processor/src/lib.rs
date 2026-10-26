//! Transgate's free-tools processor: the separately deployed, server-first
//! FFmpeg runtime behind the public tool pages (P3 D1). Ported from
//! `frontend/services/free-tools-processor`: seven anonymous tool routers
//! (39 endpoints) plus `GET /health`, in-memory sessions with temporary files,
//! per-address processing allowances, and a ten-minute cleanup of expired
//! sessions and orphaned files. Product rules live in
//! `transgate_core::tools`; this crate is the HTTP, filesystem and FFmpeg shell.
//!
//! Composition: [`Config::from_env`], [`Processor::new`], [`Processor::router`]
//! and [`Processor::spawn_cleanup`]. `main.rs` is the deployable.

pub mod families;
pub mod http;
pub mod js;
pub mod log;
pub mod media;
pub mod store;
pub mod upload;
pub mod zip;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::Method;
use axum::response::Response;
use serde_json::json;
use transgate_core::tools::{CLEANUP_INTERVAL, Family};

use crate::families::Tool;
use crate::http::{Incoming, Query};
use crate::media::Media;

/// Runtime settings, by the source's environment names.
#[derive(Clone, Debug)]
pub struct Config {
    /// `PORT`, default 8080.
    pub port: u16,
    /// `ALLOWED_ORIGINS`, comma-separated, default `https://transgate.ai`.
    pub allowed_origins: Vec<String>,
    /// `REQUEST_TIMEOUT_MS`, default 30 minutes: the longest a request body may take.
    pub request_timeout: Duration,
    /// `FREE_TOOLS_TRUSTED_PROXY_COUNT`, default and minimum 1.
    pub trusted_proxy_count: usize,
    /// `FFMPEG_THREADS`, a positive integer, default 1.
    pub ffmpeg_threads: u32,
    /// `FFMPEG_PATH` / `FFPROBE_PATH`, default from `PATH`.
    pub ffmpeg: String,
    pub ffprobe: String,
    /// Where sessions keep their files: the system temporary directory
    /// (`TMPDIR`, else `/tmp`), as the source hard-coded `/tmp`.
    pub work_dir: PathBuf,
}

/// A setting the service cannot start with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        Config::from_lookup(|name| std::env::var(name).ok(), std::env::temp_dir())
    }

    /// An unset `PORT` means 8080; a set one must be a port number, so a typo
    /// stops the service instead of moving it to another port.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>, work_dir: PathBuf) -> Result<Config, ConfigError> {
        let port = match get("PORT") {
            None => 8080,
            Some(raw) => raw
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0 && raw.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| ConfigError(format!("PORT must be a port number from 1 to 65535, got {raw:?}")))?,
        };
        let number = |name: &str| get(name).map_or(f64::NAN, |v| js::number(&v));
        let timeout_ms = number("REQUEST_TIMEOUT_MS");
        let proxies = number("FREE_TOOLS_TRUSTED_PROXY_COUNT");
        let threads = number("FFMPEG_THREADS");
        let origins: Vec<String> = get("ALLOWED_ORIGINS")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "https://transgate.ai".into())
            .split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect();
        Ok(Config {
            port,
            allowed_origins: origins,
            request_timeout: if timeout_ms.is_finite() && timeout_ms > 0.0 {
                Duration::from_millis(timeout_ms as u64)
            } else {
                Duration::from_secs(30 * 60)
            },
            trusted_proxy_count: if proxies.is_finite() && proxies >= 1.0 { proxies as usize } else { 1 },
            ffmpeg_threads: if threads.fract() == 0.0 && threads > 0.0 && threads <= f64::from(u32::MAX) {
                threads as u32
            } else {
                1
            },
            ffmpeg: get("FFMPEG_PATH").filter(|v| !v.is_empty()).unwrap_or_else(|| "ffmpeg".into()),
            ffprobe: get("FFPROBE_PATH").filter(|v| !v.is_empty()).unwrap_or_else(|| "ffprobe".into()),
            work_dir,
        })
    }
}

/// The running service: every family's sessions and allowances.
pub struct Processor {
    pub(crate) config: Config,
    pub(crate) media: Media,
    pub(crate) trim_audio: Tool<families::trim_audio::TrimSession>,
    pub(crate) convert_audio: Tool<families::convert_audio::ConvertAudioSession>,
    pub(crate) reduce_audio: Tool<families::reduce_audio::ReduceAudioSession>,
    pub(crate) remove_noise: Tool<families::remove_noise::RemoveNoiseSession>,
    pub(crate) video_to_audio: Tool<families::video_to_audio::VideoToAudioSession>,
    pub(crate) compress_video: Tool<families::compress_video::CompressVideoSession>,
    pub(crate) convert_video: Tool<families::convert_video::ConvertVideoSession>,
}

/// What one cleanup pass removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Purged {
    pub sessions: usize,
    pub orphans: usize,
}

impl Processor {
    pub fn new(config: Config) -> Arc<Processor> {
        let media = Media { ffmpeg: config.ffmpeg.clone(), ffprobe: config.ffprobe.clone(), threads: config.ffmpeg_threads };
        Arc::new(Processor {
            config,
            media,
            trim_audio: Tool::new(Family::TrimAudio),
            convert_audio: Tool::new(Family::ConvertAudio),
            reduce_audio: Tool::new(Family::ReduceAudio),
            remove_noise: Tool::new(Family::RemoveNoise),
            video_to_audio: Tool::new(Family::VideoToAudio),
            compress_video: Tool::new(Family::CompressVideo),
            convert_video: Tool::new(Family::ConvertVideo),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn work_dir(&self) -> &Path {
        &self.config.work_dir
    }

    /// All 40 routes, CORS included. Serve it with
    /// `into_make_service_with_connect_info::<SocketAddr>()` so the
    /// allowance sees the socket peer when no proxy header is present.
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new().fallback(dispatch).with_state(self.clone())
    }

    /// Every [`CLEANUP_INTERVAL`], purges expired sessions and orphaned files
    /// of all seven families. The first pass runs one interval after start.
    pub fn spawn_cleanup(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let processor = self.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + CLEANUP_INTERVAL, CLEANUP_INTERVAL);
            loop {
                ticks.tick().await;
                let purged = processor.purge(SystemTime::now()).await;
                if purged.sessions > 0 || purged.orphans > 0 {
                    log::info(
                        "Cleanup interval complete",
                        json!({ "sessionsPurged": purged.sessions, "orphansPurged": purged.orphans }),
                    );
                }
            }
        })
    }

    /// One cleanup pass as of `now`.
    pub async fn purge(&self, now: SystemTime) -> Purged {
        let dir = self.work_dir();
        let mut total = Purged::default();
        for family in Family::ALL {
            let sessions = match family {
                Family::TrimAudio => self.trim_audio.purge_expired(now, dir).await,
                Family::ConvertAudio => self.convert_audio.purge_expired(now, dir).await,
                Family::ReduceAudio => self.reduce_audio.purge_expired(now, dir).await,
                Family::RemoveNoise => self.remove_noise.purge_expired(now, dir).await,
                Family::VideoToAudio => self.video_to_audio.purge_expired(now, dir).await,
                Family::CompressVideo => self.compress_video.purge_expired(now, dir).await,
                Family::ConvertVideo => self.convert_video.purge_expired(now, dir).await,
            };
            if sessions > 0 {
                let name = if family == Family::TrimAudio { "trim" } else { family.mount() };
                log::info(&format!("Purged {sessions} expired {name} sessions"), json!({}));
            }
            total.sessions += sessions;
            total.orphans += families::purge_orphans(family, now, dir).await;
        }
        total
    }

    /// Live sessions of one family, for operations and tests.
    pub fn session_count(&self, family: Family) -> usize {
        match family {
            Family::TrimAudio => self.trim_audio.store.len(),
            Family::ConvertAudio => self.convert_audio.store.len(),
            Family::ReduceAudio => self.reduce_audio.store.len(),
            Family::RemoveNoise => self.remove_noise.store.len(),
            Family::VideoToAudio => self.video_to_audio.store.len(),
            Family::CompressVideo => self.compress_video.store.len(),
            Family::ConvertVideo => self.convert_video.store.len(),
        }
    }
}

async fn dispatch(State(p): State<Arc<Processor>>, request: Request) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let (parts, body) = request.into_parts();
    let origins = &p.config.allowed_origins;
    if parts.method == Method::OPTIONS {
        return http::preflight(&parts.headers, origins);
    }
    let request_headers = parts.headers.clone();
    let mut response = route(&p, parts, body, peer).await;
    http::add_cors(&mut response, &request_headers, origins);
    response
}

async fn route(p: &Arc<Processor>, parts: axum::http::request::Parts, body: Body, peer: Option<std::net::IpAddr>) -> Response {
    let raw_path = parts.uri.path().to_string();
    // Express matches case-insensitively and tolerates one trailing slash.
    let lower = raw_path.to_ascii_lowercase();
    let path = match lower.strip_suffix('/') {
        Some(stripped) if !stripped.is_empty() => stripped,
        _ => lower.as_str(),
    };
    // HEAD is answered by the GET route, without a body.
    let method = if parts.method == Method::HEAD { Method::GET } else { parts.method.clone() };
    if path == "/health" && method == Method::GET {
        return http::json(200, crate::obj! { "status" => "ok" });
    }
    let Some((mount, action)) = path.strip_prefix('/').and_then(|p| p.split_once('/')) else {
        return http::not_found(&parts.method, &raw_path);
    };
    let Some(family) = Family::ALL.into_iter().find(|f| f.mount() == mount) else {
        return http::not_found(&parts.method, &raw_path);
    };
    let incoming = Incoming {
        method,
        query: Query::parse(parts.uri.query()),
        ip: http::client_ip(&parts.headers, peer, p.config.trusted_proxy_count),
        headers: parts.headers,
        body,
    };
    let handled = match family {
        Family::TrimAudio => families::trim_audio::route(p, action, incoming).await,
        Family::ConvertAudio => families::convert_audio::route(p, action, incoming).await,
        Family::ReduceAudio => families::reduce_audio::route(p, action, incoming).await,
        Family::RemoveNoise => families::remove_noise::route(p, action, incoming).await,
        Family::VideoToAudio => families::video_to_audio::route(p, action, incoming).await,
        Family::CompressVideo => families::compress_video::route(p, action, incoming).await,
        Family::ConvertVideo => families::convert_video::route(p, action, incoming).await,
    };
    handled.unwrap_or_else(|| http::not_found(&parts.method, &raw_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_follows_the_source_defaults_and_refuses_an_invalid_port() {
        let none = Config::from_lookup(|_| None, "/tmp".into()).unwrap();
        assert_eq!(none.port, 8080);
        assert_eq!(none.allowed_origins, ["https://transgate.ai"]);
        assert_eq!(none.request_timeout, Duration::from_secs(1800));
        assert_eq!((none.trusted_proxy_count, none.ffmpeg_threads), (1, 1));
        assert_eq!((none.ffmpeg.as_str(), none.ffprobe.as_str()), ("ffmpeg", "ffprobe"));
        let set = |name: &str| {
            Some(
                match name {
                    "PORT" => "9090",
                    "ALLOWED_ORIGINS" => "https://a.test, ,https://b.test",
                    "REQUEST_TIMEOUT_MS" => "5000",
                    "FREE_TOOLS_TRUSTED_PROXY_COUNT" => "0",
                    "FFMPEG_THREADS" => "1.5",
                    _ => return None,
                }
                .to_string(),
            )
        };
        let c = Config::from_lookup(set, "/tmp".into()).unwrap();
        assert_eq!(c.port, 9090);
        assert_eq!(c.allowed_origins, ["https://a.test", "https://b.test"]);
        assert_eq!(c.request_timeout, Duration::from_millis(5000));
        assert_eq!((c.trusted_proxy_count, c.ffmpeg_threads), (1, 1));
        for bad in ["", "abc", "0", "65536", "+80", " 80", "80.0", "0x50"] {
            let err = Config::from_lookup(|name| (name == "PORT").then(|| bad.to_string()), "/tmp".into()).unwrap_err();
            assert!(err.0.starts_with("PORT must be a port number"), "{bad:?}: {err}");
        }
    }
}
