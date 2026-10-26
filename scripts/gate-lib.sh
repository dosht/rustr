#!/usr/bin/env bash
# Sourced by the three PRD gates. Every check runs; any failure makes the gate fail.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
gate_failures=0
check() {
  local label=$1
  shift
  printf '\n== %s ==\n' "$label"
  if "$@"; then
    printf 'PASS: %s\n' "$label"
  else
    local code=$?
    printf 'FAIL: %s (exit %s)\n' "$label" "$code" >&2
    gate_failures=$((gate_failures + 1))
  fi
}
required_script() {
  local path=$1
  shift
  if [[ ! -x "$path" ]]; then
    printf 'Not implemented: executable %s\n' "$path" >&2
    return 1
  fi
  "$path" "$@"
}
gate_finish() {
  printf '\nGate: %s failed checks\n' "$gate_failures"
  [[ "$gate_failures" -eq 0 ]]
}
