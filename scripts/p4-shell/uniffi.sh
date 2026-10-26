#!/usr/bin/env bash
# Generate real Swift bindings from the built cdylib, compile and run the foreign
# listener harness. Lead runs this only after source integration on owned fixtures.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${TASKR_P4_API:?real disposable taskr API root required}"
: "${TASKR_P4_EMAIL:?existing enrolled fixture owner email required}"
: "${TASKR_P4_WORK_ID:?owned completed work UUID required}"
: "${TASKR_P4_PROVIDER_CONTROL:?controlled loopback provider origin required}"
cargo build --locked -p taskr-ffi --features cli
lib=target/debug/libtaskr_ffi.so
[[ "$(uname)" == Darwin ]] && lib=target/debug/libtaskr_ffi.dylib
out=target/p4-shell-uniffi
mkdir -p "$out/generated"
target/debug/uniffi-bindgen generate --library "$lib" --language swift --no-format --out-dir "$out/generated"
"${SWIFTC:-swiftc}" -swift-version 5 -parse-as-library -o "$out/listener" \
  -Xcc -fmodule-map-file="$out/generated/TaskrCoreFFI.modulemap" \
  -I "$out/generated" -L target/debug -ltaskr_ffi \
  "$out/generated/TaskrCore.swift" apps/ios/p4-summary/main.swift
LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
DYLD_LIBRARY_PATH="$PWD/target/debug${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}" \
  "$out/listener"
