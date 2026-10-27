#!/usr/bin/env bash
# Cross-compiles transgate-ffi for Android into transgate/apps/android/app/src/main/jniLibs
# (the Kotlin bindings come from scripts/gen-bindings.sh). Needs the Android NDK
# (ANDROID_NDK_HOME), cargo-ndk and the Rust Android targets:
#   rustup target add aarch64-linux-android x86_64-linux-android
#   cargo install cargo-ndk
# TRANSGATE_ANDROID_ABIS limits the targets (Waydroid only needs x86_64).
# TRANSGATE_ANDROID_BUILD_STD=1 compiles std from rust-src, for distro rustc builds that
# have no Android std (no rustup), as Arch's does.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${ANDROID_HOME:=$HOME/Android/Sdk}"
: "${ANDROID_NDK_HOME:=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)}"
export ANDROID_HOME ANDROID_NDK_HOME

targets=()
for abi in ${TRANSGATE_ANDROID_ABIS:-arm64-v8a x86_64}; do targets+=(-t "$abi"); done
build_std=()
if [[ "${TRANSGATE_ANDROID_BUILD_STD:-}" == "1" ]]; then
  export RUSTC_BOOTSTRAP=1
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/android-buildstd}"
  build_std=(-Zbuild-std=std,panic_abort)
fi
cargo ndk "${targets[@]}" -o transgate/apps/android/app/src/main/jniLibs \
  build "${build_std[@]}" -p transgate-ffi --release
