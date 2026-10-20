#!/usr/bin/env bash
# Cross-compiles the core for Android and refreshes the Kotlin bindings.
# Needs: Android NDK (ANDROID_NDK_HOME), cargo-ndk, and the Rust targets
#   rustup target add aarch64-linux-android x86_64-linux-android
#   cargo install cargo-ndk
set -euo pipefail
cd "$(dirname "$0")/.."

# TASKR_ANDROID_ABIS limits the targets (Waydroid only needs x86_64).
# TASKR_ANDROID_BUILD_STD=1 compiles std from rust-src, for distro rustc builds
# that have no Android std (no rustup).
./scripts/gen-bindings.sh

targets=()
for abi in ${TASKR_ANDROID_ABIS:-arm64-v8a x86_64}; do targets+=(-t "$abi"); done
build_std=()
if [[ "${TASKR_ANDROID_BUILD_STD:-}" == "1" ]]; then
  export RUSTC_BOOTSTRAP=1
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/android-buildstd}"
  build_std=(-Zbuild-std=std,panic_abort)
fi
cargo ndk "${targets[@]}" -o apps/android/app/src/main/jniLibs \
  build "${build_std[@]}" -p taskr-ffi --release
