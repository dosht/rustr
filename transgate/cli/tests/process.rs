//! The real `transgate` binary, run as a process against a loopback host: sign in
//! (password, and Google through the RFC 8252 loopback listener), upload, list
//! jobs, fetch a transcript and a translation, export, delete, and the D13 exit
//! codes. Output is compared with the catalogs through `translate()`, never with
//! hand-written English.
mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use assert_cmd::Command;
use support::{EMAIL, GOOD_CODE, Host, host, signed_in_vault};
use transgate_core::presentation::app::{AuthFailure, balance_text};
use transgate_core::presentation::job_list::status_label;
use transgate_core::presentation::msg::{Msg, common};
use transgate_core::presentation::transcript::{NO_LINES, loading_message};
use transgate_core::presentation::upload_queue::Alert;
use transgate_core::jobs::domain::JobStatus;

fn say(locale: &str, msg: &Msg) -> String {
    transgate_i18n::translate(locale, msg).expect("the catalog has the key")
}

fn cli(host: &Host, vault: &Path) -> Command {
    let mut command = Command::cargo_bin("transgate").unwrap();
    command.env_clear().envs(std::env::var_os("PATH").map(|p| ("PATH", p)));
    command.env("TRANSGATE_HOST", &host.origin).env("TRANSGATE_SESSION_FILE", vault).env("TRANSGATE_LOCALE", "en").args(["--timeout", "20"]);
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn signed_in() -> (Host, tempfile::TempDir, PathBuf) {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let vault = signed_in_vault(dir.path(), &host.origin);
    (host, dir, vault)
}

// WEB-049/050: password sign-in, the stored session, sign-out.
#[test]
fn password_sign_in_stores_the_session_and_sign_out_forgets_it() {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("session.json");

    let login = cli(&host, &vault).args(["login", "--email", EMAIL, "--password-stdin"]).write_stdin("correct\n").assert().success();
    assert_eq!(text(&login.get_output().stdout), format!("{EMAIL}\n"));
    assert!(std::fs::read_to_string(&vault).unwrap().contains("transgate.session"), "the session reached the vault");

    let whoami = cli(&host, &vault).arg("whoami").assert().success();
    let out = text(&whoami.get_output().stdout);
    assert_eq!(out.lines().next(), Some(EMAIL));
    assert!(out.contains(&say("en", &balance_text(Some(75)))), "{out}");

    cli(&host, &vault).arg("logout").assert().success();
    assert!(!std::fs::read_to_string(&vault).unwrap().contains("transgate.session\":"), "sign-out cleared the session");
    cli(&host, &vault).arg("whoami").assert().code(3);
}

// D13: authentication failures exit 3 with the translated reason, in the chosen locale.
#[test]
fn a_wrong_password_exits_3_with_the_catalog_message() {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("session.json");
    let message = AuthFailure::InvalidCredentials.message();
    for locale in ["en", "tr"] {
        let run = cli(&host, &vault).args(["--locale", locale, "login", "--email", EMAIL, "--password-stdin"]).write_stdin("nope\n").assert().code(3);
        assert_eq!(text(&run.get_output().stderr).trim(), say(locale, &message));
    }
    assert_ne!(say("tr", &message), say("en", &message), "the Turkish catalog really differs");
    assert!(!vault.exists() || !std::fs::read_to_string(&vault).unwrap().contains("transgate.session"));
}

// D13: a host that does not answer exits 4.
#[test]
fn an_unreachable_host_exits_4() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("session.json");
    let mut command = Command::cargo_bin("transgate").unwrap();
    command.env_clear().env("TRANSGATE_HOST", "http://127.0.0.1:9").env("TRANSGATE_SESSION_FILE", &vault).env("TRANSGATE_LOCALE", "en");
    let run = command.args(["--timeout", "5", "login", "--email", EMAIL, "--password-stdin"]).write_stdin("correct\n").assert().code(4);
    assert_eq!(text(&run.get_output().stderr).trim(), say("en", &AuthFailure::Network.message()));
}

// D13: no stored session exits 3 and says to sign in.
#[test]
fn a_command_without_a_session_exits_3() {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let run = cli(&host, &dir.path().join("none.json")).arg("jobs").assert().code(3);
    assert_eq!(text(&run.get_output().stderr).trim(), say("en", &common("Please sign in and try again.")));
}

// D13: usage errors exit 2.
#[test]
fn usage_errors_exit_2() {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("s.json");
    cli(&host, &vault).args(["upload", "a.mp3", "--language", "Klingon"]).assert().code(2);
    cli(&host, &vault).args(["upload", "a.mp3", "--translate-to", "tr"]).assert().code(2);
    cli(&host, &vault).args(["--locale", "xx", "jobs"]).assert().code(2);
    cli(&host, &vault).arg("frobnicate").assert().code(2);
    cli(&host, &vault).args(["login", "--google", "--email", EMAIL]).assert().code(2);
    cli(&host, &vault).args(["login", "--password-stdin"]).write_stdin("x\n").assert().code(2);
    let mut no_host = Command::cargo_bin("transgate").unwrap();
    no_host.env_clear().arg("jobs").assert().code(2);
    let missing = dir.path().join("missing.mp3");
    let (host, _dir2, vault2) = signed_in();
    cli(&host, &vault2).args(["upload", missing.to_str().unwrap()]).assert().code(2);
}

// WEB-049: Google sign-in over the RFC 8252 loopback redirect, `--no-browser` prints the URL.
#[test]
fn google_sign_in_exchanges_the_loopback_callback_code() {
    for (code, expected) in [(GOOD_CODE, 0), ("stale", 3)] {
        let host = host();
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("session.json");
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("transgate"))
            .env_clear()
            .env("TRANSGATE_HOST", &host.origin)
            .env("TRANSGATE_SESSION_FILE", &vault)
            .env("TRANSGATE_LOCALE", "en")
            .args(["--timeout", "20", "login", "--google", "--no-browser"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut url = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut url).unwrap();
        assert!(url.starts_with(&format!("{}/auth/v1/authorize?provider=google", host.origin)), "{url}");
        let redirect = url.split("redirect_to=").nth(1).unwrap().split('&').next().unwrap().replace("%3A", ":").replace("%2F", "/");
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/home"), "{redirect}");
        let address = redirect.trim_start_matches("http://").trim_end_matches("/home");

        // The browser's return: a favicon first (ignored), then the callback.
        for target in ["/favicon.ico".to_string(), format!("/home?code={code}")] {
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            write!(stream, "GET {target} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
            let mut answer = String::new();
            stream.read_to_string(&mut answer).unwrap();
            assert!(answer.starts_with(if target.contains("code") { "HTTP/1.1 200" } else { "HTTP/1.1 404" }), "{target}: {answer}");
        }
        let finished = child.wait_with_output().unwrap();
        assert_eq!(finished.status.code(), Some(expected), "{}", text(&finished.stderr));
        let vault_text = std::fs::read_to_string(&vault).unwrap_or_default();
        assert_eq!(vault_text.contains("transgate.session\":"), expected == 0, "{vault_text}");
        if expected == 0 {
            assert_eq!(text(&finished.stdout), EMAIL.to_string() + "\n");
        } else {
            assert_eq!(text(&finished.stderr).trim(), say("en", &AuthFailure::InvalidOrExpiredCode.message()));
        }
    }
}

// WEB-061/062: list jobs.
#[test]
fn jobs_are_listed_newest_first_as_text_and_json() {
    let (host, _dir, vault) = signed_in();
    let run = cli(&host, &vault).arg("jobs").assert().success();
    let out = text(&run.get_output().stdout);
    let rows: Vec<Vec<&str>> = out.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows.iter().map(|r| r[0]).collect::<Vec<_>>(), ["3", "2", "1"]);
    assert_eq!(rows[2][1], "talk", "the extension is dropped, as the list shows it");
    assert_eq!(rows[2][2], say("en", &status_label(JobStatus::Completed)));
    assert!(!rows[2][3].is_empty());

    let limited = cli(&host, &vault).args(["jobs", "--limit", "1"]).assert().success();
    assert_eq!(text(&limited.get_output().stdout).lines().count(), 1);

    let json = cli(&host, &vault).args(["jobs", "--json"]).assert().success();
    let rows: serde_json::Value = serde_json::from_slice(&json.get_output().stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap()).collect::<Vec<_>>(), [3, 2, 1]);
}

// WEB-061/062: a transcript, a translation, and the job that is not ready.
#[test]
fn a_transcript_and_a_translation_are_fetched_and_a_running_job_is_refused() {
    let (host, _dir, vault) = signed_in();
    let run = cli(&host, &vault).args(["transcript", "1"]).assert().success();
    let out = text(&run.get_output().stdout);
    assert!(out.contains("Hello there") && out.contains("Hi, \"Ada\""), "{out}");
    assert!(out.contains(&format!("{} 1: ", say("en", &common("Speaker")))), "the numbered speaker is labelled with the translated word: {out}");
    assert!(out.contains("[00:00:00 - 00:00:02]"), "{out}");

    let turkish = cli(&host, &vault).args(["transcript", "3", "--translation", "Turkish"]).assert().success();
    let out = text(&turkish.get_output().stdout);
    assert!(out.contains("Merhaba") && !out.contains("Hello there"), "{out}");
    let by_code = cli(&host, &vault).args(["transcript", "3", "--translation", "tr"]).assert().success();
    assert_eq!(text(&by_code.get_output().stdout), out);

    let missing = cli(&host, &vault).args(["transcript", "3", "--translation", "German"]).assert().code(1);
    assert_eq!(text(&missing.get_output().stderr).trim(), say("en", &NO_LINES));

    let running = cli(&host, &vault).args(["transcript", "2"]).assert().code(1);
    assert_eq!(text(&running.get_output().stderr).trim(), say("en", &loading_message(JobStatus::Running, false).unwrap()));

    let json = cli(&host, &vault).args(["transcript", "1", "--json"]).assert().success();
    let lines: serde_json::Value = serde_json::from_slice(&json.get_output().stdout).unwrap();
    assert_eq!(lines.as_array().unwrap().len(), 2);

    let show = cli(&host, &vault).args(["show", "1"]).assert().success();
    assert!(text(&show.get_output().stdout).starts_with("talk.mp3\n"), "{}", text(&show.get_output().stdout));
}

// WEB-065/066: export saves the zip the host built.
#[test]
fn export_saves_the_zip_for_both_download_kinds() {
    let (host, dir, vault) = signed_in();
    let out_dir = dir.path().join("exports");
    let first = cli(&host, &vault).args(["export", "1", "--out", out_dir.to_str().unwrap()]).assert().success();
    let saved = out_dir.join("Job Name_transcript.zip");
    assert_eq!(text(&first.get_output().stdout).trim(), saved.display().to_string());
    assert_eq!(std::fs::read(&saved).unwrap(), b"PK-job-1");
    std::fs::remove_file(&saved).unwrap();
    cli(&host, &vault).args(["export", "1", "--all-formats", "--out", out_dir.to_str().unwrap()]).assert().success();
    let kinds: Vec<String> = host.state.lock().unwrap().exports.iter().map(|e| e["kind"]["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["wysiwyg", "all_formats"]);

    let blocked = dir.path().join("a-file");
    std::fs::write(&blocked, "x").unwrap();
    let refused = cli(&host, &vault).args(["export", "1", "--out", blocked.join("inside").to_str().unwrap()]).assert().code(1);
    assert!(!text(&refused.get_output().stderr).trim().is_empty());
}

#[test]
fn delete_removes_the_job_through_core_and_the_host() {
    let (host, _dir, vault) = signed_in();
    let run = cli(&host, &vault).args(["delete", "1"]).assert().success();
    assert_eq!(text(&run.get_output().stdout), "1\n");
    assert_eq!(host.state.lock().unwrap().deleted, [1]);
    let rest = cli(&host, &vault).arg("jobs").assert().success();
    assert_eq!(text(&rest.get_output().stdout).lines().map(|l| l.split('\t').next().unwrap().to_string()).collect::<Vec<_>>(), ["3", "2"]);
}

// WEB-053/054: upload; the options travel before the draft, the file is PUT with its type.
#[test]
fn upload_creates_the_draft_puts_the_file_and_starts_it() {
    let (host, dir, vault) = signed_in();
    let file = dir.path().join("talk.mp3");
    std::fs::write(&file, b"audio-bytes").unwrap();
    let run = cli(&host, &vault).args(["upload", file.to_str().unwrap(), "--language", "tr", "--speakers"]).assert().success();
    let ids: Vec<i64> = text(&run.get_output().stdout).lines().map(|l| l.parse().unwrap()).collect();
    let state = host.state.lock().unwrap();
    assert_eq!(ids, [101], "the new job is printed");
    assert_eq!(state.puts.len(), 1);
    assert_eq!((state.puts[0].1, state.puts[0].2.as_deref()), (11, Some("audio/mpeg")));
    assert_eq!(state.started, [vec![101]]);
    let options = serde_json::to_string(&state.options).unwrap();
    assert!(options.contains("\"language\":\"Turkish\""), "{options}");
    assert!(state.jobs.iter().find(|j| j.id.0 == 101).is_some_and(|j| j.speakers), "the speaker toggle travels with the draft");
}

#[test]
fn a_translation_upload_sends_both_languages() {
    let (host, dir, vault) = signed_in();
    let file = dir.path().join("a.m4a");
    std::fs::write(&file, b"xyz").unwrap();
    cli(&host, &vault).args(["upload", file.to_str().unwrap(), "--language", "English", "--translate-to", "Turkish"]).assert().success();
    let options = serde_json::to_string(&host.state.lock().unwrap().options).unwrap();
    assert!(options.contains("\"translateTo\":[\"Turkish\"]"), "{options}");
}

// UPL-007/JOB-006: the host's insufficient-balance refusal is shown through core's alert.
#[test]
fn an_insufficient_balance_refusal_exits_1_with_the_alert_text() {
    let (host, dir, vault) = signed_in();
    host.state.lock().unwrap().insufficient = true;
    let file = dir.path().join("talk.mp3");
    std::fs::write(&file, b"abc").unwrap();
    let run = cli(&host, &vault).args(["upload", file.to_str().unwrap()]).assert().code(1);
    let err = text(&run.get_output().stderr);
    assert!(err.contains(&say("en", &Alert::Balance.title())), "{err}");
    for paragraph in Alert::Balance.paragraphs() {
        assert!(err.contains(&say("en", &paragraph)), "{err}");
    }
    assert!(host.state.lock().unwrap().started.is_empty());
}

// WEB-057/058: a YouTube link is added and resolved.
#[test]
fn a_youtube_link_is_added_and_transcribed() {
    let (host, _dir, vault) = signed_in();
    let run = cli(&host, &vault).args(["youtube", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"]).assert().success();
    assert_eq!(text(&run.get_output().stdout).trim(), "101");
    assert_eq!(host.state.lock().unwrap().jobs.iter().find(|j| j.id.0 == 101).map(|j| j.status), Some(JobStatus::Completed));
    let bad = cli(&host, &vault).args(["youtube", "not a url"]).assert().code(1);
    assert!(!text(&bad.get_output().stderr).trim().is_empty());
}

#[test]
fn upload_batch_leaves_restored_drafts_and_their_options_untouched() {
    let (host, dir, vault) = signed_in();
    let old = transgate_core::presentation::fixtures::job(90, JobStatus::Draft, "old.mp3");
    host.state.lock().unwrap().jobs.push(old.clone());
    let file = dir.path().join("new.mp3");
    std::fs::write(&file, b"audio-bytes").unwrap();
    let run = cli(&host, &vault)
        .timeout(std::time::Duration::from_secs(8))
        .args(["upload", file.to_str().unwrap(), "--language", "tr", "--speakers"])
        .assert().success();
    assert_eq!(text(&run.get_output().stdout).trim(), "101");
    let state = host.state.lock().unwrap();
    assert_eq!(state.jobs.iter().find(|job| job.id == old.id), Some(&old));
    assert_eq!(state.started, [vec![101]]);
    assert!(state.deleted.is_empty());
    assert!(state.options.iter().all(|options| !options["jobIds"].as_array().unwrap().contains(&serde_json::json!(90))));
}

#[test]
fn youtube_batch_leaves_restored_drafts_and_their_options_untouched() {
    let (host, _dir, vault) = signed_in();
    let old = transgate_core::presentation::fixtures::job(90, JobStatus::Draft, "old.mp3");
    host.state.lock().unwrap().jobs.push(old.clone());
    let run = cli(&host, &vault)
        .timeout(std::time::Duration::from_secs(8))
        .args(["youtube", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "--language", "tr"])
        .assert().success();
    assert_eq!(text(&run.get_output().stdout).trim(), "101");
    let state = host.state.lock().unwrap();
    assert_eq!(state.jobs.iter().find(|job| job.id == old.id), Some(&old));
    assert!(state.deleted.is_empty());
    assert!(state.options.iter().all(|options| !options["jobIds"].as_array().unwrap().contains(&serde_json::json!(90))));
}

#[test]
fn concurrent_commands_rotate_a_shared_session_only_once() {
    let (host, _dir, path) = signed_in();
    host.state.lock().unwrap().single_use_refresh = true;
    let vault = transgate_native::Vault::file(&path).for_host(&transgate_native::HostConfig::from_origin(&host.origin, None));
    let mut session: serde_json::Value = serde_json::from_str(&vault.get("transgate.session").unwrap()).unwrap();
    session["expires_at"] = serde_json::json!(1);
    vault.set("transgate.session", &session.to_string());
    let launch = || {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("transgate"));
        command.env("TRANSGATE_HOST", &host.origin).env("TRANSGATE_SESSION_FILE", &path).args(["--timeout", "10", "jobs"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    };
    let first = launch();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while host.state.lock().unwrap().refreshes.is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let second = launch();
    for child in [first, second] {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", text(&output.stderr));
    }
    assert_eq!(host.state.lock().unwrap().refreshes, ["ref"]);
    let stored: serde_json::Value = serde_json::from_str(&vault.get("transgate.session").unwrap()).unwrap();
    assert_eq!(stored["refresh_token"], "rotated-ref");
}

#[test]
fn switching_hosts_never_reuses_access_or_refresh_credentials() {
    for expired in [false, true] {
        let (source, _dir, path) = signed_in();
        let vault = transgate_native::Vault::file(&path).for_host(&transgate_native::HostConfig::from_origin(&source.origin, None));
        if expired {
            let mut session: serde_json::Value = serde_json::from_str(&vault.get("transgate.session").unwrap()).unwrap();
            session["expires_at"] = serde_json::json!(1);
            vault.set("transgate.session", &session.to_string());
        }
        let destination = host();
        cli(&destination, &path).arg("jobs").assert().code(3);
        assert!(destination.state.lock().unwrap().refreshes.is_empty());
        assert!(vault.get("transgate.session").is_some());
    }
}

#[test]
fn contended_session_write_fails_promptly_and_login_can_retry() {
    let host = host();
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("session.json");
    let lock_path = dir.path().join("session.json.lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let held = options.open(lock_path).unwrap();
    held.lock().unwrap();
    for google in [false, true] {
        let mut command = cli(&host, &vault);
        command.timeout(std::time::Duration::from_secs(2)).arg("login");
        if google { command.args(["--google", "--no-browser"]); }
        else { command.args(["--email", EMAIL, "--password-stdin"]); }
        let refused = command.write_stdin("correct\n").assert().code(1);
        assert!(text(&refused.get_output().stderr).contains("writing the session file"));
    }
    drop(held);
    cli(&host, &vault).args(["login", "--email", EMAIL, "--password-stdin"]).write_stdin("correct\n").assert().success();
}
