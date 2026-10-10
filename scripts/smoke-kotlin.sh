#!/usr/bin/env bash
# Compiles the generated Kotlin bindings plus apps/android/smoke on the JVM and
# runs them against the host build of the core. Needs kotlinc, java and the
# JNA jar (JNA_JAR, e.g. jna-5.18.1.jar from Maven Central).
set -euo pipefail
cd "$(dirname "$0")/.."
: "${JNA_JAR:?set JNA_JAR to a jna-5.x.jar}"

./scripts/gen-bindings.sh >/dev/null
out=target/kotlin-smoke
mkdir -p "$out"
kotlinc -nowarn -include-runtime -cp "$JNA_JAR" -d "$out/smoke.jar" \
  apps/android/app/src/main/java/dev/taskr/core/taskr_ffi.kt \
  apps/android/smoke/BindingsSmoke.kt
java -Djna.library.path="$PWD/target/debug" -cp "$out/smoke.jar:$JNA_JAR" BindingsSmokeKt
