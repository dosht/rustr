#!/usr/bin/env bash
# S1: first coherent integration snapshot and the P4 post-cleanup re-proof (integration.md §5a).
# Lead-authorized only. Every step reads the FROZEN copy, never the moving working tree.
#
#   scripts/p5-web/s1.sh freeze                 # copies the tree, prints S=$HOME/.cache/rustr-integration/s1-<id>
#   scripts/p5-web/s1.sh gate     $S            # stale checks that the published sources allow
#   scripts/p5-web/s1.sh build    $S            # host, taskr host, taskr ffi+bindgen, taskr wasm pkg
#   scripts/p5-web/s1.sh p5       $S            # lead-authorized P5 client/wasm proof (after build, before P4 runs)
#   scripts/p5-web/s1.sh stage    $S            # immutable bin/, swift/, pkg tar, sha256
#   scripts/p5-web/s1.sh transgate $S           # run5 scenarios (endpoint-only migration)
#   scripts/p5-web/s1.sh shell    $S            # taskr wasm Playwright 4/4 (desktop + Pixel 7)
#   scripts/p5-web/s1.sh swift    $S            # generated Linux Swift listener (pinned swift image)
#
# Owned PG/loopback only; no pulls (images by local id/digest), no live providers.
set -uo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
SWIFT_IMAGE=swift@sha256:c58ddae47db799aeaca66235c9af686b1fd3c5a5c845da9465f46fb1672c5db6  # swift:6.2-noble amd64 used by P4
NODE_IMAGE=node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205
clean() { env -i PATH="$PATH" HOME="$HOME" RUST_BACKTRACE=1 CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" "$@"; }
step() { echo "== $1 $(date -Is)"; }

phase=${1:?phase}; S=${2:-}
case "$phase" in
freeze)
  # /tmp is a small tmpfs on this workstation; keep complete build snapshots on disk.
  S="${XDG_CACHE_HOME:-$HOME/.cache}/rustr-integration/s1-$(date +%Y%m%d%H%M%S)"
  mkdir -p "$S"/{logs,bin,swift}
  step freeze | tee "$S/logs/freeze.log"
  rsync -a --exclude /target --exclude node_modules --exclude /.git --exclude test-results --exclude 'playwright-report' \
        --exclude 'p4-playwright-report' --exclude dist --exclude .env --exclude '.env.*' \
        --exclude /apps/web/src/lib/core/pkg --exclude /transgate/web/src/lib/core/pkg \
        "$REPO"/ "$S/src/" >> "$S/logs/freeze.log" 2>&1 || exit 1
  (cd "$REPO" && git rev-parse HEAD && git status --porcelain) > "$S/logs/git-state.txt"
  (cd "$S/src" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$S/source.sha256"
  sha256sum "$S/source.sha256" | tee -a "$S/logs/freeze.log"
  # Freeze installed taskr tooling too: a symlink back to the moving checkout is not a fixed input.
  cp -a "$REPO/apps/web/node_modules" "$S/src/apps/web/node_modules" || exit 1
  sha256sum "$REPO/apps/web/package-lock.json" "$S/src/apps/web/package-lock.json" | tee -a "$S/logs/freeze.log"
  {
    rustc --version --verbose
    cargo --version --verbose
    wasm-bindgen --version
    node --version
    sha256sum "$(command -v rustc)" "$(command -v cargo)" "$(command -v wasm-bindgen)"
    docker image inspect "$NODE_IMAGE" "$SWIFT_IMAGE" postgres:17-alpine \
      --format '{{.Id}} {{json .RepoDigests}}'
  } > "$S/logs/toolchain.log" 2>&1 || exit 1
  echo "S=$S"
  ;;
gate)
  cd "$S/src" || exit 1
  step gate | tee "$S/logs/gate.log"
  r=0
  clean cargo metadata --locked --format-version 1 > /dev/null 2>> "$S/logs/gate.log" || { echo "FAIL lock coherence" | tee -a "$S/logs/gate.log"; r=1; }
  clean python3 -B scripts/gen-tokens.py --check >> "$S/logs/gate.log" 2>&1 || { echo "FAIL tokens --check" | tee -a "$S/logs/gate.log"; r=1; }
  for f in fonts/provenance.json assets/provenance.json; do
    python3 - "$S/src/transgate/export/$f" >> "$S/logs/gate.log" 2>&1 <<'EOF' || { echo "FAIL provenance $f" | tee -a "$S/logs/gate.log"; r=1; }
import hashlib, json, pathlib, sys
doc = pathlib.Path(sys.argv[1]); data = json.loads(doc.read_text())
def walk(v):
    if isinstance(v, dict):
        if 'sha256' in v and any(k in v for k in ('file', 'path', 'name')):
            yield v
        for x in v.values(): yield from walk(x)
    elif isinstance(v, list):
        for x in v: yield from walk(x)
entries = list(walk(data)); assert entries, 'no sha256 entries in ' + str(doc)
for e in entries:
    name = e.get('file') or e.get('path') or e.get('name')
    p = doc.parent / pathlib.Path(name).name
    got = hashlib.sha256(p.read_bytes()).hexdigest()
    assert got == e['sha256'], f'{p}: {got} != {e["sha256"]}'
print(f'{doc}: {len(entries)} hashes match')
EOF
  done
  # D20 seam: new app assets live under /_app/*; public /assets/* stays P6's. Static check of the current
  # host/web production source (tests may legitimately assert /assets is NOT served): no app-owned /assets
  # route, Vite output or reference, and /_app present.
  d20=$(grep -rnE --exclude=tests.rs --exclude='*.test.ts' --exclude='*.spec.ts' --exclude='*.stories.ts' \
          "(['\"\`(]|^)/assets(/|['\"\`])|assetsDir|join\(\"assets\"\)" transgate/server/src/web.rs transgate/server/src/web \
          transgate/web/vite.config.ts transgate/web/index.html transgate/web/public transgate/web/src 2>/dev/null \
          | grep -v '_app' || true)
  if [ -n "$d20" ]; then echo "FAIL D20 /assets references (review; P6-owned public paths are not app assets):" | tee -a "$S/logs/gate.log"; echo "$d20" | tee -a "$S/logs/gate.log"; r=1; fi
  for source in transgate/server/src/web.rs transgate/web/vite.config.ts; do
    grep -qE "['\"]/?_app" "$source" \
      || { echo "FAIL D20: _app missing from $source" | tee -a "$S/logs/gate.log"; r=1; }
  done
  # Node is the already-acquired official digest, never a pull or runtime toolchain substitution.
  if ! docker image inspect "$NODE_IMAGE" > /dev/null 2>&1; then
    echo "BLOCKED-GATE pinned Node24 image is not cached" | tee -a "$S/logs/gate.log"
    [ "$r" -ne 0 ] || r=78
  else
    if [ ! -f transgate/web/package-lock.json ]; then
      echo "BLOCKED-GATE transgate/web/package-lock.json absent" | tee -a "$S/logs/gate.log"
      [ "$r" -ne 0 ] || r=78
    else
      docker run --rm --pull=never --user "$(id -u):$(id -g)" \
        --label "rustr.s1.node=$(basename "$S")" -e HOME=/tmp -e PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
        -v "$S/src/transgate/web:/work" -w /work "$NODE_IMAGE" npm ci --no-audit --no-fund \
        > "$S/logs/npm-ci.log" 2>&1 || { echo "FAIL npm ci" | tee -a "$S/logs/gate.log"; r=1; }
    fi
    if [ ! -f scripts/p5-locales/validate.mjs ]; then
      echo "BLOCKED-GATE locale validator source absent" | tee -a "$S/logs/gate.log"
      [ "$r" -ne 0 ] || r=78
    else
      docker run --rm --pull=never --network none --user "$(id -u):$(id -g)" \
        --label "rustr.s1.node=$(basename "$S")" -v "$S/src:/work:ro" -w /work \
        "$NODE_IMAGE" node scripts/p5-locales/validate.mjs > "$S/logs/locales.log" 2>&1 \
        || { echo "FAIL locale validator" | tee -a "$S/logs/gate.log"; r=1; }
    fi
  fi
  # Regenerate in a separate complete source copy. A stale result must never overwrite the
  # captured S1 contract. Reuse only the build cache; source and generated output are distinct.
  if [ ! -f transgate/web/src/lib/core/fixtures.json ] || [ ! -f transgate/web/src/lib/core/fixtures.generated.ts ] || [ ! -d transgate/web/src/lib/core/types ]; then
    echo "BLOCKED-GATE published core TS/fixtures absent" | tee -a "$S/logs/gate.log"
    [ "$r" -ne 0 ] || r=78
  else
    G="$S/generator-src"
    mkdir "$G" || exit 1
    rsync -a --exclude /target --exclude node_modules \
      --exclude /transgate/web/src/lib/core/types --exclude /transgate/web/src/lib/core/fixtures.json \
      --exclude /transgate/web/src/lib/core/fixtures.generated.ts \
      "$S/src/" "$G/" >> "$S/logs/gate.log" 2>&1 || exit 1
    (cd "$G" && clean CARGO_TARGET_DIR="$S/src/target" \
      cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::generate) \
      > "$S/logs/generated-contract.log" 2>&1
    g=$?
    if [ "$g" -ne 0 ]; then
      echo "FAIL core exporter exit $g" | tee -a "$S/logs/gate.log"; r=1
    else
      diff -ru transgate/web/src/lib/core/types "$G/transgate/web/src/lib/core/types" \
        > "$S/logs/generated-types.diff" \
        || { echo "FAIL generated TS drift" | tee -a "$S/logs/gate.log"; r=1; }
      cmp transgate/web/src/lib/core/fixtures.json "$G/transgate/web/src/lib/core/fixtures.json" \
        > "$S/logs/generated-fixtures.diff" \
        || { echo "FAIL generated fixture drift" | tee -a "$S/logs/gate.log"; r=1; }
      cmp transgate/web/src/lib/core/fixtures.generated.ts "$G/transgate/web/src/lib/core/fixtures.generated.ts" \
        > "$S/logs/generated-fixtures-module.diff" \
        || { echo "FAIL generated fixture module drift" | tee -a "$S/logs/gate.log"; r=1; }
    fi
  fi
  sha256sum --check --quiet "$S/source.sha256" > "$S/logs/gate-source-integrity.log" 2>&1 \
    || { echo "FAIL frozen source changed during gate" | tee -a "$S/logs/gate.log"; r=1; }
  if [ "$r" -eq 0 ]; then
    sha256sum "$S/source.sha256" > "$S/logs/gate.passed"
    echo "gate ok" | tee -a "$S/logs/gate.log"
  fi
  exit "$r"
  ;;
build)
  cd "$S/src" || exit 1
  step build | tee "$S/logs/build.log"
  [ -f "$S/logs/gate.passed" ] || { echo "BLOCKED: no completed S1 stale gate"; exit 78; }
  clean cargo build --locked --message-format=json-render-diagnostics -p transgate-server -p taskr-server -p billing-postgres \
    --bin transgate-server --bin taskr-server --bin billing-provision > "$S/logs/native-build.jsonl" 2>> "$S/logs/build.log" \
    || { echo "FAIL native build" | tee -a "$S/logs/build.log"; exit 20; }
  clean cargo build --locked -p taskr-ffi --features cli >> "$S/logs/build.log" 2>&1 || { echo "FAIL taskr-ffi" | tee -a "$S/logs/build.log"; exit 21; }
  cp -r apps/web/src/lib/core/types "$S/logs/taskr-types.before"
  clean TASKR_WASM_BUILD_STD=1 ./scripts/build-web.sh >> "$S/logs/build.log" 2>&1 || { echo "FAIL taskr wasm" | tee -a "$S/logs/build.log"; exit 22; }
  diff -r "$S/logs/taskr-types.before" apps/web/src/lib/core/types > "$S/logs/taskr-types.diff" \
    && echo "taskr generated types unchanged" | tee -a "$S/logs/build.log" \
    || { echo "FAIL taskr generated TS drift (see taskr-types.diff)" | tee -a "$S/logs/build.log"; exit 23; }
  echo "build ok" | tee -a "$S/logs/build.log"
  ;;
p5)
  # Arch's distro rustc ships no wasm32 std (only x86_64 under lib/rustlib), so both wasm steps use the
  # repository's established build-std path (scripts/build-web.sh TASKR_WASM_BUILD_STD=1). No getrandom cfg.
  cd "$S/src" || exit 1
  r=0
  step "p5 publication-integrity boundary" | tee "$S/logs/p5-integrity-test.log"
  clean cargo test --locked -p transgate-core --features ts --lib \
    presentation::fixtures::web::export_integrity_preserves_outputs_on_rejection -- --exact \
    >> "$S/logs/p5-integrity-test.log" 2>&1
  g=$?; echo "integrity test exit $g" | tee -a "$S/logs/p5-integrity-test.log"; [ "$g" -eq 0 ] || r=1
  step "p5 composed AppState regressions" | tee "$S/logs/p5-app-test.log"
  clean cargo test --locked -p transgate-core --features ts --lib presentation::app::tests \
    >> "$S/logs/p5-app-test.log" 2>&1
  a=$?; echo "app tests exit $a" | tee -a "$S/logs/p5-app-test.log"
  [ "$a" -eq 0 ] || r=1
  step "p5 transgate-client test" | tee "$S/logs/p5-client-test.log"
  echo "+ cargo test --locked -p transgate-client" >> "$S/logs/p5-client-test.log"
  clean cargo test --locked -p transgate-client >> "$S/logs/p5-client-test.log" 2>&1
  c=$?; echo "client test exit $c" | tee -a "$S/logs/p5-client-test.log"; [ $c -eq 0 ] || r=1
  step "p5 transgate-wasm check" | tee "$S/logs/p5-wasm-check.log"
  echo "+ RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR=target/wasm-buildstd cargo check --locked -Zbuild-std=std,panic_abort -p transgate-wasm --target wasm32-unknown-unknown" >> "$S/logs/p5-wasm-check.log"
  clean RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR=target/wasm-buildstd \
    cargo check --locked -Zbuild-std=std,panic_abort -p transgate-wasm --target wasm32-unknown-unknown >> "$S/logs/p5-wasm-check.log" 2>&1
  c=$?; echo "wasm check exit $c" | tee -a "$S/logs/p5-wasm-check.log"; [ $c -eq 0 ] || r=1
  step "p5 build-transgate-wasm" | tee "$S/logs/p5-wasm-build.log"
  echo "+ TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh  (wasm-bindgen $(wasm-bindgen --version))" >> "$S/logs/p5-wasm-build.log"
  clean TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh >> "$S/logs/p5-wasm-build.log" 2>&1
  c=$?; echo "wasm build exit $c" | tee -a "$S/logs/p5-wasm-build.log"; [ $c -eq 0 ] || r=1
  if [ $c -eq 0 ]; then
    (cd transgate/web/src/lib/core/pkg && sha256sum *) | tee "$S/logs/p5-wasm-pkg.sha256"
    tar -C transgate/web/src/lib/core -cf "$S/transgate-wasm-pkg.tar" pkg
  fi
  exit $r
  ;;
stage)
  set -e
  cd "$S/src" || exit 1
  for b in transgate-server taskr-server billing-provision; do install -m 0555 "target/debug/$b" "$S/bin/$b"; done
  mkdir -p "$S/root/target" && ln -sfn ../../bin "$S/root/target/debug"
  install -m 0444 target/debug/libtaskr_ffi.so "$S/swift/libtaskr_ffi.so"
  clean target/debug/uniffi-bindgen generate --library "$S/swift/libtaskr_ffi.so" --language swift --no-format --out-dir "$S/swift/generated" > "$S/logs/bindgen.log" 2>&1 \
    || { echo "FAIL bindgen"; exit 30; }
  install -m 0444 apps/ios/p4-summary/main.swift "$S/swift/main.swift"
  tar -C apps/web/src/lib/core -cf "$S/taskr-wasm-pkg.tar" pkg types
  chmod -R a-w "$S/bin" "$S/swift" apps/web/src/lib/core/pkg transgate/web/src/lib/core/pkg
  chmod a-w "$S/taskr-wasm-pkg.tar" "$S/transgate-wasm-pkg.tar"
  (cd "$S" && sha256sum bin/* swift/libtaskr_ffi.so swift/generated/* swift/main.swift \
    taskr-wasm-pkg.tar transgate-wasm-pkg.tar \
    src/apps/web/src/lib/core/pkg/* src/transgate/web/src/lib/core/pkg/*) | tee "$S/artifacts.sha256"
  ;;
transgate)
  cd "$S/src" || exit 1
  (cd "$S" && sha256sum -c artifacts.sha256 --quiet) || { echo "FAIL artifact hash"; exit 40; }
  clean systemd-inhibit --what=sleep:idle --why="rustr S1 transgate re-proof" \
    python3 -B scripts/p4-ai/smoke-transgate.py --root "$S/root" --postgres-image postgres:17-alpine --log-dir "$S/logs/transgate" \
    > "$S/logs/transgate-run.log" 2>&1
  c=$?; echo "transgate exit $c" | tee -a "$S/logs/transgate-run.log"; exit "$c"
  ;;
shell)
  cd "$S/src" || exit 1
  (cd "$S" && sha256sum -c artifacts.sha256 --quiet) || { echo "FAIL artifact hash"; exit 40; }
  clean systemd-inhibit --what=sleep:idle --why="rustr S1 taskr shell" \
    python3 -B scripts/p4-shell/runtime.py --bin-dir "$S/bin" --log-dir "$S/logs/shell" \
    -- sh -c 'cd apps/web && npx playwright test --config playwright.p4.config.ts --output ../../../logs/shell-test-results' \
    > "$S/logs/shell-run.log" 2>&1
  c=$?; echo "shell exit $c" | tee -a "$S/logs/shell-run.log"; exit "$c"
  ;;
swift)
  cd "$S/src" || exit 1
  (cd "$S" && sha256sum -c artifacts.sha256 --quiet) || { echo "FAIL artifact hash"; exit 40; }
  docker image inspect "$SWIFT_IMAGE" > /dev/null || { echo "FAIL swift image not cached"; exit 41; }
  mkdir -p "$S/swift-build"
  clean S="$S" SWIFT_IMAGE="$SWIFT_IMAGE" systemd-inhibit --what=sleep:idle --why="rustr S1 swift" \
    python3 -B scripts/p4-shell/runtime.py --bin-dir "$S/bin" --log-dir "$S/logs/swift" -- sh -c '
      docker run --rm --pull=never --network host --label rustr.s1.swift=1 \
        -v "$S/swift:/s1/swift:ro" -v "$S/swift-build:/s1/out" \
        -e TASKR_P4_API -e TASKR_P4_EMAIL -e TASKR_P4_WORK_ID -e TASKR_P4_PROVIDER_CONTROL -e TASKR_P4_FIXTURE_DATABASE_URL -e TASKR_P4_WEB_URL \
        "$SWIFT_IMAGE" sh -c "swiftc --version && swiftc -swift-version 5 -parse-as-library -o /s1/out/listener \
          -Xcc -fmodule-map-file=/s1/swift/generated/TaskrCoreFFI.modulemap -I /s1/swift/generated -L /s1/swift -ltaskr_ffi \
          /s1/swift/generated/TaskrCore.swift /s1/swift/main.swift && LD_LIBRARY_PATH=/s1/swift /s1/out/listener"' \
    > "$S/logs/swift-run.log" 2>&1
  c=$?; echo "swift exit $c" | tee -a "$S/logs/swift-run.log"; exit "$c"
  ;;
*) echo "unknown phase $phase" >&2; exit 2 ;;
esac
