#!/usr/bin/env bash
# The macOS half of the iOS build. Everything before this step runs on Linux.
# Needs: Xcode, xcodegen (brew install xcodegen) and the Rust targets
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname)" == "Darwin" ]] || { echo "build-ios.sh must run on macOS"; exit 1; }

./scripts/gen-bindings.sh

for t in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
  cargo build -p taskr-ffi --release --target "$t"
done

out=apps/ios/build
rm -rf "$out" apps/ios/TaskrCore.xcframework
mkdir -p "$out/headers" "$out/sim"
cp apps/ios/Generated/TaskrCoreFFI.h "$out/headers/"
cp apps/ios/Generated/TaskrCoreFFI.modulemap "$out/headers/module.modulemap"
lipo -create \
  target/aarch64-apple-ios-sim/release/libtaskr_ffi.a \
  target/x86_64-apple-ios/release/libtaskr_ffi.a \
  -output "$out/sim/libtaskr_ffi.a"

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libtaskr_ffi.a -headers "$out/headers" \
  -library "$out/sim/libtaskr_ffi.a" -headers "$out/headers" \
  -output apps/ios/TaskrCore.xcframework

(cd apps/ios && xcodegen generate)
echo "open apps/ios/Taskr.xcodeproj"
