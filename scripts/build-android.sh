#!/usr/bin/env bash
# Cross-compiles the core for Android and refreshes the Kotlin bindings.
# Needs: Android NDK (ANDROID_NDK_HOME), cargo-ndk, and the Rust targets
#   rustup target add aarch64-linux-android x86_64-linux-android
#   cargo install cargo-ndk
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/gen-bindings.sh
cargo ndk -t arm64-v8a -t x86_64 -o apps/android/app/src/main/jniLibs \
  build -p taskr-ffi --release
