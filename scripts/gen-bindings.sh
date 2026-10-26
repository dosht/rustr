#!/usr/bin/env bash
# Regenerates Kotlin and Swift bindings, or checks without changing them.
# Runs on Linux; no Android SDK or Xcode needed. --check exits 1 on drift.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-write}
if [[ $# -gt 1 || ( "$mode" != write && "$mode" != --check ) ]]; then
  echo "usage: $0 [--check]" >&2
  exit 2
fi
if [[ "$mode" == --check ]]; then
  generated=$(mktemp -d)
  trap 'rm -rf "$generated"' EXIT
  kotlin="$generated/kotlin"
  swift="$generated/swift"
else
  kotlin=apps/android/app/src/main/java
  swift=apps/ios/Generated
fi

cargo build --locked -p taskr-ffi
lib=target/debug/libtaskr_ffi.so
[[ "$(uname)" == "Darwin" ]] && lib=target/debug/libtaskr_ffi.dylib

bindgen() { cargo run -q --locked -p taskr-ffi --features cli --bin uniffi-bindgen -- "$@"; }
bindgen generate --library "$lib" --language kotlin --no-format \
  --out-dir "$kotlin"
bindgen generate --library "$lib" --language swift --no-format \
  --out-dir "$swift"
if [[ "$mode" == --check ]]; then
  result=0
  diff -ru apps/android/app/src/main/java/dev/taskr/core "$kotlin/dev/taskr/core" || result=1
  diff -ru apps/ios/Generated "$swift" || result=1
  if [[ "$result" -eq 0 ]]; then
    echo "bindings match Kotlin and Swift sources"
  else
    echo "bindings differ; run ./scripts/gen-bindings.sh and review the changes" >&2
  fi
  exit "$result"
fi
echo "bindings written to apps/android and apps/ios/Generated"
