//! FFmpeg and ffprobe child processes. Arguments are passed as a vector, never
//! through a shell (the source's `exec` quoted paths into a shell string).

use std::ffi::OsStr;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

tokio::task_local! {
    static SESSION_CANCEL: tokio::sync::watch::Receiver<bool>;
}

/// Every child/probe of one registered session worker inherits cancellation
/// without changing the FFmpeg argument or HTTP contracts.
pub(crate) async fn with_cancellation<T>(
    cancel: tokio::sync::watch::Receiver<bool>,
    work: impl Future<Output = T>,
) -> T {
    SESSION_CANCEL.scope(cancel, work).await
}

fn is_cancelled() -> bool {
    SESSION_CANCEL.try_with(|cancel| *cancel.borrow()).unwrap_or(false)
}

async fn cancelled() {
    match SESSION_CANCEL.try_with(Clone::clone) {
        Ok(mut cancel) => {
            let _ = cancel.wait_for(|stop| *stop).await;
        }
        Err(_) => std::future::pending().await,
    }
}

/// `exec` captures at most this much per stream by default in the source (10 MiB).
pub const CAPTURE_LIMIT: usize = 10 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Media {
    pub ffmpeg: String,
    pub ffprobe: String,
    /// `-threads N`, placed before `-i` and before the output codec options.
    pub threads: u32,
}

impl Media {
    pub fn thread_args(&self) -> [String; 2] {
        ["-threads".into(), self.threads.to_string()]
    }
}

#[derive(Debug)]
pub enum RunError {
    Spawn(std::io::Error),
    /// Exited unsuccessfully; `None` when it died on a signal.
    Exit(Option<i32>, String),
    TooMuchOutput,
    TimedOut,
    /// Stopped because a sibling command failed.
    Cancelled,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "spawn failed: {e}"),
            RunError::Exit(code, stderr) => write!(f, "Process exited {code:?}: {}", stderr.chars().take(500).collect::<String>()),
            RunError::TooMuchOutput => f.write_str("maxBuffer exceeded"),
            RunError::TimedOut => f.write_str("timed out"),
            RunError::Cancelled => f.write_str("cancelled after its sibling failed"),
        }
    }
}

fn command<S: AsRef<OsStr>>(bin: &str, args: &[S]) -> Command {
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    cmd
}

async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => return Ok(out),
            Ok(n) => {
                if out.len() + n > limit {
                    return Err(());
                }
                out.extend_from_slice(&buf[..n]);
            }
        }
    }
}

/// Runs to completion and returns stdout, like `execFile` with `maxBuffer`
/// and an optional `timeout`: non-zero exit, too much output or the timeout
/// is an error. A child stopped early is killed and reaped before returning.
pub async fn output<S: AsRef<OsStr>>(
    bin: &str,
    args: &[S],
    limit: usize,
    timeout: Option<Duration>,
) -> Result<String, RunError> {
    output_until(bin, args, limit, timeout, std::future::pending()).await
}

/// [`output`] that also stops, killing and reaping the child, when `cancel` completes.
pub async fn output_until<S: AsRef<OsStr>>(
    bin: &str,
    args: &[S],
    limit: usize,
    timeout: Option<Duration>,
    cancel: impl Future<Output = ()>,
) -> Result<String, RunError> {
    if is_cancelled() {
        return Err(RunError::Cancelled);
    }
    let mut child = command(bin, args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(RunError::Spawn)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    // Reading both pipes and reaping the exit are one unit under the deadline
    // and cancellation: a child that closes its pipes but stays alive is still stopped.
    let run = async {
        let (out, err) =
            tokio::try_join!(read_capped(stdout, limit), read_capped(stderr, limit)).map_err(|()| RunError::TooMuchOutput)?;
        let status = child.wait().await.map_err(RunError::Spawn)?;
        Ok::<_, RunError>((status, out, err))
    };
    let deadline = async {
        match timeout {
            Some(limit) => tokio::time::sleep(limit).await,
            None => std::future::pending().await,
        }
    };
    let outcome = tokio::select! {
        biased;
        () = cancelled() => Err(RunError::Cancelled),
        () = cancel => Err(RunError::Cancelled),
        () = deadline => Err(RunError::TimedOut),
        outcome = run => outcome,
    };
    // `run` is dropped here, so the child can be killed and reaped.
    let (status, out, err) = match outcome {
        Ok(done) => done,
        Err(e) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(e);
        }
    };
    if status.success() {
        Ok(String::from_utf8_lossy(&out).into_owned())
    } else {
        Err(RunError::Exit(status.code(), String::from_utf8_lossy(&err).into_owned()))
    }
}

/// Runs two commands at once. When either fails, the other is killed and
/// reaped at once, and the first real failure is returned.
pub async fn output_pair<S: AsRef<OsStr>>(bin: &str, first: &[S], second: &[S], limit: usize) -> Result<(), RunError> {
    let (failed, _) = tokio::sync::watch::channel(false);
    let (a, b) = tokio::join!(
        output_or_cancel(bin, first, limit, failed.clone()),
        output_or_cancel(bin, second, limit, failed.clone())
    );
    match (a, b) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(RunError::Cancelled), Err(e)) | (Err(e), _) | (Ok(_), Err(e)) => Err(e),
    }
}

async fn output_or_cancel<S: AsRef<OsStr>>(
    bin: &str,
    args: &[S],
    limit: usize,
    failed: tokio::sync::watch::Sender<bool>,
) -> Result<String, RunError> {
    let mut sibling = failed.subscribe();
    let sibling_failed = async move {
        // Both senders live until both commands end, so the channel stays open.
        if sibling.wait_for(|f| *f).await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let result = output_until(bin, args, limit, None, sibling_failed).await;
    if result.is_err() {
        failed.send_replace(true);
    }
    result
}

/// How a processing child ended.
#[derive(Debug)]
pub enum Ended {
    Success,
    /// Unsuccessful exit; `None` when it died on a signal.
    Failed(Option<i32>),
    SpawnFailed(std::io::Error),
    /// Killed by us after the family's ceiling.
    TimedOut,
    /// Its session was invalidated; the child has been killed and reaped.
    Cancelled,
}

/// A processing FFmpeg child. `stderr_line` sees each stderr line (used for
/// `-progress pipe:2`); otherwise stderr is drained so the child never blocks.
/// After `kill_after` it is SIGKILLed and `on_timeout` runs before the reap.
pub async fn process<S: AsRef<OsStr>, F: Future<Output = ()>>(
    bin: &str,
    args: &[S],
    kill_after: Option<Duration>,
    mut stderr_line: impl FnMut(&str) + Send,
    on_timeout: impl FnOnce() -> F,
) -> Ended {
    if is_cancelled() {
        return Ended::Cancelled;
    }
    let mut child: Child = match command(bin, args).stdout(Stdio::null()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(e) => return Ended::SpawnFailed(e),
    };
    let stderr = child.stderr.take().expect("piped stderr");
    let drain = async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            stderr_line(&line);
        }
    };
    let wait = async {
        let (status, ()) = tokio::join!(child.wait(), drain);
        status
    };
    let deadline = async {
        match kill_after {
            Some(limit) => tokio::time::sleep(limit).await,
            None => std::future::pending().await,
        }
    };
    let outcome = tokio::select! {
        biased;
        () = cancelled() => Err(Ended::Cancelled),
        () = deadline => Err(Ended::TimedOut),
        result = wait => Ok(result),
    };
    let status = match outcome {
        Ok(status) => status,
        Err(reason) => {
            let _ = child.start_kill();
            if matches!(&reason, Ended::TimedOut) {
                on_timeout().await;
            }
            let _ = child.wait().await;
            return reason;
        }
    };
    match status {
        Ok(status) if status.success() => Ended::Success,
        Ok(status) => Ended::Failed(status.code()),
        Err(e) => Ended::SpawnFailed(e),
    }
}

/// The last `<key><digits>` value in an FFmpeg `-progress` text, e.g.
/// `out_time_ms=` (which FFmpeg writes in microseconds). 0 when absent.
pub fn last_progress_value(text: &str, key: &str) -> f64 {
    let mut last = 0.0;
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        let after = &rest[at + key.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            last = after[..digits].parse().unwrap_or(0.0);
        }
        rest = after;
    }
    last
}

/// Reads a `-progress` file; 0 when it does not exist or has no value yet.
pub async fn progress_file_value(path: &std::path::Path, key: &str) -> f64 {
    match tokio::fs::read(path).await {
        Ok(bytes) => last_progress_value(&String::from_utf8_lossy(&bytes), key),
        Err(_) => 0.0,
    }
}

/// `ffprobe -show_entries format=duration` as `parseFloat` reads it.
pub async fn probe_duration(media: &Media, path: &std::path::Path, limit: usize) -> Result<f64, RunError> {
    let out = output(
        &media.ffprobe,
        &[
            OsStr::new("-v"),
            OsStr::new("error"),
            OsStr::new("-show_entries"),
            OsStr::new("format=duration"),
            OsStr::new("-of"),
            OsStr::new("default=noprint_wrappers=1:nokey=1"),
            path.as_os_str(),
        ],
        limit,
        None,
    )
    .await?;
    Ok(crate::js::parse_float(out.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_progress_value_with_digits_wins() {
        let text = "out_time_ms=100\nprogress=continue\nout_time_ms=N/A\nout_time_ms=2500000\nprogress=end\n";
        assert_eq!(last_progress_value(text, "out_time_ms="), 2_500_000.0);
        assert_eq!(last_progress_value("out_time_us=7\n", "out_time_ms="), 0.0);
        assert_eq!(last_progress_value("", "out_time_us="), 0.0);
    }

    /// The hard ceiling kills a real FFmpeg child that would otherwise run forever.
    #[tokio::test]
    async fn a_processing_child_past_its_ceiling_is_killed() {
        let started = std::time::Instant::now();
        let mut timed_out = false;
        let ended = process(
            "ffmpeg",
            &["-loglevel", "error", "-re", "-f", "lavfi", "-i", "sine=frequency=440", "-f", "null", "-"],
            Some(Duration::from_millis(400)),
            |_| {},
            || {
                timed_out = true;
                async {}
            },
        )
        .await;
        assert!(matches!(ended, Ended::TimedOut), "{ended:?}");
        assert!(timed_out);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// A child that closes stdout and stderr but keeps running is still under
    /// the deadline and its sibling's failure.
    #[tokio::test]
    async fn a_child_that_closes_its_pipes_cannot_outlive_the_deadline_or_a_failed_sibling() {
        let detached = ["-c", "exec >&- 2>&-; sleep 30"];
        let started = std::time::Instant::now();
        let result = output("sh", &detached, CAPTURE_LIMIT, Some(Duration::from_millis(300))).await;
        assert!(matches!(result, Err(RunError::TimedOut)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
        let started = std::time::Instant::now();
        let pair = output_pair("sh", &detached, &["-c", "sleep 0.2; exit 3"], CAPTURE_LIMIT).await;
        assert!(matches!(pair, Err(RunError::Exit(Some(3), _))), "{pair:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
    }

    /// One trim part failing stops the other at once; it does not run to its
    /// natural end (here: never) or outlive the call.
    #[tokio::test]
    async fn a_failing_sibling_kills_and_reaps_the_other_command_promptly() {
        let marker = format!("comment=transgate-sibling-{}", uuid::Uuid::new_v4());
        let marker = marker.as_str();
        let endless = ["-loglevel", "error", "-re", "-f", "lavfi", "-i", "sine=frequency=440", "-metadata", marker, "-f", "null", "-"];
        let failing = ["-loglevel", "error", "-i", "/nonexistent/input.wav", "-f", "null", "-"];
        let started = std::time::Instant::now();
        let result = output_pair("ffmpeg", &endless, &failing, CAPTURE_LIMIT).await;
        assert!(matches!(result, Err(RunError::Exit(Some(_), _))), "the real failure is reported: {result:?}");
        assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
        let alive = std::fs::read_dir("/proc").unwrap().filter_map(|e| e.ok()).any(|e| {
            std::fs::read(e.path().join("cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains(marker))
        });
        assert!(!alive, "the endless sibling is gone when the pair returns");
        let both = output_pair("ffprobe", &["-version"], &["-version"], CAPTURE_LIMIT).await;
        assert!(both.is_ok(), "{both:?}");
    }

    #[tokio::test]
    async fn a_missing_binary_is_a_spawn_failure_and_bad_input_is_an_exit_failure() {
        let ended = process("transgate-no-such-ffmpeg", &["-version"], None, |_| {}, || async {}).await;
        assert!(matches!(ended, Ended::SpawnFailed(_)));
        let result = output("ffprobe", &["-v", "error", "/nonexistent/input.wav"], CAPTURE_LIMIT, None).await;
        assert!(matches!(result, Err(RunError::Exit(Some(_), _))), "{result:?}");
    }
}
