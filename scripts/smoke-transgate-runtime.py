#!/usr/bin/env python3
"""Exercise the actual Transgate process or container; never contacts production."""
import argparse
import json
import os
from pathlib import Path
import re
import select
import signal
import socket
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def check_http(url):
    with urllib.request.urlopen(url + "/health", timeout=5) as response:
        assert response.status == 200
        assert response.headers.get_content_type() == "application/json"
        assert json.load(response) == {
            "service": "transgate-server", "version": VERSION,
            "status": "alive", "check": "process-liveness",
        }
    try:
        urllib.request.urlopen(url + "/api/billing/ready", timeout=5)
    except urllib.error.HTTPError as error:
        assert error.code == 503 and json.load(error)["ready"] is False
    else:
        raise AssertionError("unconfigured billing must not claim readiness")
    # Job routes mount only with billing and TRANSGATE_JOBS_ENABLED=1; liveness-only mode serves none.
    for path in ("/api/jobs/start", "/api/jobs"):
        try:
            urllib.request.urlopen(url + path, timeout=5)
        except urllib.error.HTTPError as error:
            assert error.code == 404, (path, error.code)
        else:
            raise AssertionError(path + " must not be served without billing and jobs configuration")


def local(binary=None):
    if binary is None:
        subprocess.run(["cargo", "build", "--locked", "-p", "transgate-server"], cwd=ROOT, check=True)
        metadata = json.loads(command("cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"))
        binary = Path(metadata["target_directory"]) / "debug/transgate-server"
    # This liveness-only smoke must never inherit real billing/provider configuration.
    runtime_env = {"PATH": os.environ["PATH"]}
    for stop_signal in (signal.SIGINT, signal.SIGTERM):
        process = subprocess.Popen([binary], env={**runtime_env, "TRANSGATE_BIND": "127.0.0.1:0"}, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 10
            started = None
            logs = []
            while time.monotonic() < deadline:
                if not select.select([process.stderr], [], [], 0.2)[0]:
                    continue
                line = process.stderr.readline()
                if not line:
                    raise AssertionError("server exited before listening: " + "".join(logs))
                logs.append(line)
                started = re.search(r"listening on (127\.0\.0\.1:\d+)", line)
                if started:
                    break
            assert started, "server did not become ready"
            check_http("http://" + started[1])
            process.send_signal(stop_signal)
            assert process.wait(timeout=10) == 0, process.stderr.read()
            print(f"PASS local HTTP and graceful {stop_signal.name}")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stderr.close()
    invalid = "invalid-private-config-value"
    result = subprocess.run([binary], env={**runtime_env, "TRANSGATE_BIND": invalid}, capture_output=True, text=True, timeout=10)
    assert result.returncode != 0 and "TRANSGATE_BIND" in result.stderr and invalid not in result.stderr
    with socket.socket() as occupied:
        occupied.bind(("127.0.0.1", 0))
        occupied.listen()
        address = f"127.0.0.1:{occupied.getsockname()[1]}"
        result = subprocess.run([binary], env={**runtime_env, "TRANSGATE_BIND": address}, capture_output=True, text=True, timeout=10)
        assert result.returncode != 0 and "binding TRANSGATE_BIND" in result.stderr
    print("PASS invalid/occupied bind fail explicitly without echoing invalid configuration")
    secret = "synthetic-private-value"
    jobs = {
        "TRANSGATE_BIND": "127.0.0.1:0", "TRANSGATE_JOBS_ENABLED": "1",
        "TRANSGATE_PUBLIC_URL": "https://transgate.example.test", "STORAGE_PROVIDER": "cloudflare",
        "SONIOX_MAX_DURATION_MINUTES": "200", "TRANSGATE_ARCHIVE_BUCKET": "archive", "TRANSGATE_STAGING_BUCKET": "staging",
        "TRANSCRIPT_API_KEY": secret, "YOUTUBE_API_KEY": secret, "CRON_SECRET": secret,
        "MEDIA_R2_ENDPOINT": "https://r2.example.test", "MEDIA_R2_BUCKET": "audio",
        "MEDIA_R2_ACCESS_KEY": secret, "MEDIA_R2_SECRET_KEY": secret,
        "SONIOX_API_KEY": secret, "SONIOX_PROJECT_ID": "synthetic",
        "LEGACY_SUPABASE_URL": "https://legacy.example.test", "LEGACY_SUPABASE_SERVICE_ROLE_KEY": secret,
    }
    for env, expected in (
        ({k: v for k, v in jobs.items() if k != "TRANSGATE_PUBLIC_URL"}, "TRANSGATE_PUBLIC_URL is required"),
        ({k: v for k, v in jobs.items() if k != "LEGACY_SUPABASE_SERVICE_ROLE_KEY"}, "LEGACY_SUPABASE_SERVICE_ROLE_KEY"),
        (jobs, "needs the billing configuration"),
    ):
        result = subprocess.run([binary], env={**runtime_env, **env}, capture_output=True, text=True, timeout=10)
        assert result.returncode != 0 and expected in result.stderr and secret not in result.stderr, result.stderr
    print("PASS jobs refuse incomplete configuration or missing billing before listening, without echoing values")


def docker():
    image = "rustr-transgate:foundation-smoke"
    name = "rustr-transgate-smoke-" + uuid.uuid4().hex[:12]
    subprocess.run(["docker", "build", "-f", "transgate/Dockerfile", "-t", image, "."], cwd=ROOT, check=True)
    created = False
    try:
        command("docker", "run", "--detach", "--name", name, "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges:true", "--publish", "127.0.0.1::8788", "--health-interval=1s", image)
        created = True
        inspected = json.loads(command("docker", "inspect", name))[0]
        assert inspected["Config"]["User"] == "65532:65532"
        assert inspected["HostConfig"]["ReadonlyRootfs"] is True
        port = inspected["NetworkSettings"]["Ports"]["8788/tcp"][0]["HostPort"]
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            state = json.loads(command("docker", "inspect", name))[0]["State"]
            if not state["Running"]:
                raise AssertionError("container stopped: " + command("docker", "logs", name))
            if state["Health"]["Status"] == "healthy":
                break
            time.sleep(0.25)
        else:
            raise AssertionError("container did not become healthy: " + json.dumps(state))
        check_http(f"http://127.0.0.1:{port}")
        command("docker", "stop", "--time", "10", name)
        state = json.loads(command("docker", "inspect", name))[0]["State"]
        assert state["ExitCode"] == 0 and not state["OOMKilled"], state
        print("PASS Docker non-root/read-only runtime, healthcheck, HTTP contract, 404 and graceful stop")
    finally:
        if created:
            subprocess.run(["docker", "rm", "--force", name], cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--docker", action="store_true", help="build and exercise the container instead of the host executable")
    parser.add_argument("--binary", type=Path, help="exercise this prebuilt snapshot without rebuilding (local only)")
    args = parser.parse_args()
    try:
        if args.docker and args.binary:
            parser.error("--binary cannot be used with --docker")
        docker() if args.docker else local(args.binary.resolve() if args.binary else None)
    except (OSError, AssertionError, subprocess.SubprocessError, ValueError) as error:
        print(f"FAIL runtime smoke: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
