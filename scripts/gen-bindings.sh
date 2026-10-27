#!/usr/bin/env bash
# Regenerates Kotlin and Swift bindings for taskr-ffi and transgate-ffi, or checks
# without changing them. Runs on Linux; no Android SDK or Xcode needed. --check
# exits 1 on drift. The Transgate bindings come from a library built from the core
# view models (UniFFI remote types), so a core VM change that the FFI does not
# follow fails to build before it can drift.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-write}
if [[ $# -gt 1 || ( "$mode" != write && "$mode" != --check ) ]]; then
  echo "usage: $0 [--check]" >&2
  exit 2
fi

ext=so
[[ "$(uname)" == "Darwin" ]] && ext=dylib
result=0

# generate <crate> <library stem> <kotlin out> <swift out> <kotlin package dir under the out dir>
generate() {
  local crate=$1 stem=$2 kotlin_out=$3 swift_out=$4 package_dir=$5
  local kotlin=$kotlin_out swift=$swift_out generated=""
  if [[ "$mode" == --check ]]; then
    generated=$(mktemp -d)
    kotlin="$generated/kotlin"
    swift="$generated/swift"
  fi
  cargo build --locked -p "$crate"
  local lib="target/debug/lib${stem}.${ext}"
  bindgen() { cargo run -q --locked -p "$crate" --features cli --bin uniffi-bindgen -- "$@"; }
  bindgen generate --library "$lib" --language kotlin --no-format --out-dir "$kotlin"
  bindgen generate --library "$lib" --language swift --no-format --out-dir "$swift"
  if [[ "$mode" == --check ]]; then
    diff -ru "$kotlin_out/$package_dir" "$kotlin/$package_dir" || result=1
    diff -ru "$swift_out" "$swift" || result=1
    rm -rf "$generated"
  else
    echo "$crate bindings written to $kotlin_out and $swift_out"
  fi
}

generate taskr-ffi taskr_ffi apps/android/app/src/main/java apps/ios/Generated dev/taskr/core
generate transgate-ffi transgate_ffi transgate/apps/android/app/src/main/java transgate/apps/ios/Generated dev/transgate/core

if [[ "$mode" == --check ]]; then
  if [[ "$result" -eq 0 ]]; then
    echo "bindings match Kotlin and Swift sources"
  else
    echo "bindings differ; run ./scripts/gen-bindings.sh and review the changes" >&2
  fi
  exit "$result"
fi
