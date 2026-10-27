#!/usr/bin/env bash
# The macOS half of the Transgate iOS build (P8-IOS). Everything before this step runs on
# Linux: scripts/gen-bindings.sh (bindings, --check) and transgate/scripts/check-swift.sh.
# Needs: Xcode, xcodegen (brew install xcodegen) and the Rust targets
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
#   (x86_64 only if you run the simulator on an Intel Mac; drop it from `targets` otherwise)
# Produces transgate/apps/ios/TransgateCore.xcframework and Transgate.xcodeproj.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname)" == "Darwin" ]] || { echo "build-transgate-ios.sh must run on macOS"; exit 1; }
command -v xcodegen >/dev/null || { echo "xcodegen is missing: brew install xcodegen"; exit 1; }

./scripts/gen-bindings.sh

targets=(aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios)
for target in "${targets[@]}"; do
  cargo build --locked -p transgate-ffi --release --target "$target"
done

ios=transgate/apps/ios
out=$ios/build
rm -rf "$out" "$ios/TransgateCore.xcframework"
mkdir -p "$out/headers" "$out/sim"
cp "$ios/Generated/TransgateCoreFFI.h" "$out/headers/"
cp "$ios/Generated/TransgateCoreFFI.modulemap" "$out/headers/module.modulemap"
lipo -create \
  target/aarch64-apple-ios-sim/release/libtransgate_ffi.a \
  target/x86_64-apple-ios/release/libtransgate_ffi.a \
  -output "$out/sim/libtransgate_ffi.a"

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libtransgate_ffi.a -headers "$out/headers" \
  -library "$out/sim/libtransgate_ffi.a" -headers "$out/headers" \
  -output "$ios/TransgateCore.xcframework"

(cd "$ios" && xcodegen generate)
echo "open $ios/Transgate.xcodeproj  (set TRANSGATE_API_URL, TRANSGATE_AUTH_URL, TRANSGATE_AUTH_KEY, TRANSGATE_WEB_URL in the target's build settings)"
