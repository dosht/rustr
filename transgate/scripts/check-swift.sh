#!/usr/bin/env bash
# D14: the generated Swift bindings and the iOS shell's non-UI half compile and run
# on Linux Swift against the host build of the core, in the pinned swift:6.2-noble
# image (the image P4 and the S1 harness use). SwiftUI files are not part of this
# build (they need Xcode; see transgate/apps/ios/README.md).
#
#   transgate/scripts/check-swift.sh
#
# Environment:
#   TRANSGATE_FFI_LIB_DIR  directory with libtransgate_ffi.so (default target/debug,
#                          built here with cargo)
#   SWIFT_IMAGE            the pinned image (default: the digest below)
#   SWIFTC                 use this local swiftc instead of Docker
#   TRANSGATE_SMOKE_API / _AUTH / _KEY / _EMAIL / _PASSWORD
#                          optional: a real host and a verified user: adds a password sign-in run
#                          (a local fake host for a stored-session run is always started)
#
# Prints P8_IOS_PASS <step names> on success; any other outcome exits non-zero.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

IMAGE=${SWIFT_IMAGE:-swift@sha256:c58ddae47db799aeaca66235c9af686b1fd3c5a5c845da9465f46fb1672c5db6}
ios=transgate/apps/ios
lib_dir=${TRANSGATE_FFI_LIB_DIR:-$root/target/debug}
sources=("$ios/Generated/TransgateCore.swift" "$ios"/Transgate/Core/*.swift "$ios/smoke/UiContract.swift" "$ios/smoke/Smoke.swift")

if [[ -z "${TRANSGATE_FFI_LIB_DIR:-}" ]]; then
  cargo build --locked -p transgate-ffi >&2
fi
[[ -f "$lib_dir/libtransgate_ffi.so" ]] || { echo "FAIL: $lib_dir/libtransgate_ffi.so not found" >&2; exit 1; }

out=$(mktemp -d)
put_pid=
cleanup() {
  [[ -n "$put_pid" ]] && kill "$put_pid" 2>/dev/null || true
  rm -rf "$out"
}
trap cleanup EXIT

# A local host for the smoke: the PUT receiver (/put/ok answers 200, anything else 500)
# and the three calls a restored session makes, with the wire shapes of
# transgate_api::public (the same ones web/e2e/api-keys.spec.ts answers). Nothing
# leaves this machine.
host_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 -B - "$host_port" > "$out/host.log" 2>&1 <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

USER = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b"
PROFILE = {
    "account": USER, "email": "ios-smoke@example.test", "full_name": None, "display_name": "ios-smoke", "joined_on": None,
    "capabilities": {
        "ai": False, "ai_chat": False, "youtube": False, "microphone": False, "google_drive": False, "onedrive": False,
        "show_original": False, "mobile_ux_jobs": True, "mobile_expanded_player": False, "mobile_ux_ai": True,
        "billing_history": True, "customer_lookup": False,
    },
}

class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body=None):
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        if data:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?")[0]
        print("GET", path, flush=True)
        if path == "/api/session":
            self.reply(200, PROFILE)
        elif path == "/api/jobs":
            self.reply(200, {"jobs": [], "next_cursor": None})
        else:
            self.reply(404, {"code": "not_found", "error": "Not found"})

    def do_POST(self):
        remaining = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(remaining)
        print("POST", self.path, flush=True)
        self.reply(204 if self.path.startswith("/auth/v1/logout") else 404)

    def do_PUT(self):
        remaining = int(self.headers.get("Content-Length", "0"))
        while remaining > 0:
            remaining -= len(self.rfile.read(min(remaining, 65536)))
        self.reply(200 if self.path == "/put/ok" else 500)

    def log_message(self, *args):
        pass

HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY
put_pid=$!
export TRANSGATE_SMOKE_HOST="http://127.0.0.1:$host_port"
for _ in $(seq 50); do
  python3 -c "import socket; socket.create_connection(('127.0.0.1', $host_port), 0.2)" 2>/dev/null && break
  sleep 0.1
done

common=(-swift-version 5 -parse-as-library
  -Xcc -fmodule-map-file="$ios/Generated/TransgateCoreFFI.modulemap" -I "$ios/Generated"
  -ltransgate_ffi -Xlinker --allow-shlib-undefined)

if [[ -n "${SWIFTC:-}" ]]; then
  "$SWIFTC" "${common[@]}" -L "$lib_dir" -o "$out/smoke" "${sources[@]}" >&2
  LD_LIBRARY_PATH="$lib_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$out/smoke"
  exit
fi

docker image inspect "$IMAGE" >/dev/null 2>&1 || { echo "FAIL: pinned Swift image $IMAGE is not cached (docker pull it, or set SWIFTC)" >&2; exit 41; }

# The library is built on this machine. If it needs a newer glibc than the image
# has (the image is Ubuntu 24.04, glibc 2.39), run the Swift binary with this
# machine's libc instead: the toolchain and its runtime stay the image's.
need=$(objdump -T "$lib_dir/libtransgate_ffi.so" | grep -o 'GLIBC_[0-9.]*' | sort -V -u | tail -1 | sed 's/GLIBC_//')
runner='/s1/out/smoke'
mounts=()
if [[ $(printf '%s\n2.39\n' "$need" | sort -V | tail -1) != 2.39 ]]; then
  echo "note: libtransgate_ffi.so needs glibc $need (image has 2.39); running with the host libc" >&2
  mounts=(-v /usr/lib:/hostlib:ro)
  runner='/hostlib/ld-linux-x86-64.so.2 --library-path /hostlib:/usr/lib/swift/linux:/usr/lib/x86_64-linux-gnu:/s1/lib /s1/out/smoke'
fi

env_args=()
for name in TRANSGATE_SMOKE_HOST TRANSGATE_SMOKE_API TRANSGATE_SMOKE_AUTH TRANSGATE_SMOKE_KEY TRANSGATE_SMOKE_EMAIL TRANSGATE_SMOKE_PASSWORD; do
  [[ -n "${!name:-}" ]] && env_args+=(-e "$name=${!name}")
done

docker run --rm --pull=never --network host "${mounts[@]}" "${env_args[@]}" -e "RUNNER=$runner" \
  -v "$root:/s1/repo:ro" -v "$lib_dir:/s1/lib:ro" -v "$out:/s1/out" -w /s1/repo \
  "$IMAGE" sh -c '
    set -e
    swiftc --version >&2
    swiftc "$@" -L /s1/lib -o /s1/out/smoke
    exec $RUNNER
  ' sh "${common[@]}" "${sources[@]}"
