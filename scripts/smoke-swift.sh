#!/usr/bin/env bash
# Compiles the generated Swift bindings, the iOS container (TasksModel) and
# apps/ios/smoke with a Linux (or macOS) Swift toolchain and runs them against
# the host build of the core. SwiftUI files are not part of this build.
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/gen-bindings.sh >/dev/null
out=target/swift-smoke
mkdir -p "$out"
"${SWIFTC:-swiftc}" -swift-version 5 -parse-as-library -o "$out/smoke" \
  -Xcc -fmodule-map-file=apps/ios/Generated/TaskrCoreFFI.modulemap \
  -I apps/ios/Generated -L target/debug -ltaskr_ffi \
  apps/ios/Generated/TaskrCore.swift \
  apps/ios/Taskr/TasksModel.swift \
  apps/ios/smoke/main.swift
LD_LIBRARY_PATH="$PWD/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$out/smoke"
