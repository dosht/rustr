#!/usr/bin/env bash
# Builds the core for the browser and writes the JS glue into apps/web.
#
# Needs the wasm32-unknown-unknown target and a wasm-bindgen CLI whose version
# matches the wasm-bindgen crate (pinned in Cargo.toml):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.128
# Without rustup, point TASKR_WASM_SYSROOT at a sysroot that contains the
# wasm32 std, and WASM_BINDGEN at the CLI binary.
set -euo pipefail
cd "$(dirname "$0")/.."

profile="${1:-release}"
out=apps/web/src/lib/core/pkg
bindgen="${WASM_BINDGEN:-wasm-bindgen}"

if [[ -n "${TASKR_WASM_SYSROOT:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS:-} --sysroot ${TASKR_WASM_SYSROOT}"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/wasm-sysroot}"
fi

cargo test -q -p taskr-wasm >/dev/null
cargo build -p taskr-wasm --target wasm32-unknown-unknown --profile "$profile"

target_dir="${CARGO_TARGET_DIR:-target}"
profile_dir="$profile"
[[ "$profile" == "dev" ]] && profile_dir=debug
"$bindgen" --target web --out-dir "$out" \
  "$target_dir/wasm32-unknown-unknown/$profile_dir/taskr_wasm.wasm"
echo "wasm core written to $out"
