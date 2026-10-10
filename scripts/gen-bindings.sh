#!/usr/bin/env bash
# Regenerates the Kotlin and Swift bindings from the compiled FFI library.
# Runs on Linux; no Android SDK or Xcode needed.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build -p taskr-ffi
lib=target/debug/libtaskr_ffi.so
[[ "$(uname)" == "Darwin" ]] && lib=target/debug/libtaskr_ffi.dylib

bindgen() { cargo run -q -p taskr-ffi --features cli --bin uniffi-bindgen -- "$@"; }
bindgen generate --library "$lib" --language kotlin --no-format \
  --out-dir apps/android/app/src/main/java
bindgen generate --library "$lib" --language swift --no-format \
  --out-dir apps/ios/Generated
echo "bindings written to apps/android and apps/ios/Generated"
