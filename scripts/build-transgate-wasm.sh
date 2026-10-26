#!/usr/bin/env bash
# Builds transgate-wasm for the browser and writes the JS glue into
# transgate/web/src/lib/core/pkg (lead/integration runs only). Generated TS
# types and fixtures.json come from core's single generator
# (`presentation::fixtures::web::generate`, feature `ts`), run separately.
#
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli matching the
# workspace pin (=0.2.128). As in build-web.sh: TASKR_WASM_SYSROOT selects a
# sysroot with the wasm32 std, TASKR_WASM_BUILD_STD=1 builds std from rust-src
# (distro rustc), WASM_BINDGEN names the CLI binary.
set -euo pipefail
cd "$(dirname "$0")/.."

profile="${1:-release}"
out=transgate/web/src/lib/core/pkg
bindgen="${WASM_BINDGEN:-wasm-bindgen}"

if [[ -n "${TASKR_WASM_SYSROOT:-}" ]]; then
  export RUSTFLAGS="${RUSTFLAGS:-} --sysroot ${TASKR_WASM_SYSROOT}"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/wasm-sysroot}"
fi

build_std=()
if [[ "${TASKR_WASM_BUILD_STD:-}" == "1" ]]; then
  export RUSTC_BOOTSTRAP=1
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/wasm-buildstd}"
  build_std=(-Zbuild-std=std,panic_abort)
fi

cargo build --locked "${build_std[@]}" -p transgate-wasm --target wasm32-unknown-unknown --profile "$profile"

target_dir="${CARGO_TARGET_DIR:-target}"
profile_dir="$profile"
[[ "$profile" == "dev" ]] && profile_dir=debug
"$bindgen" --target web --out-dir "$out" \
  "$target_dir/wasm32-unknown-unknown/$profile_dir/transgate_wasm.wasm"
echo "transgate wasm written to $out"
