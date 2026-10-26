#!/usr/bin/env bash
source "$(dirname "$0")/gate-lib.sh"
check 'Discovery and gate regression tests' python3 -B -m unittest discover -s scripts -p 'test_*.py'
check 'Workspace tests' cargo test --locked --workspace
check 'Workspace Clippy' cargo clippy --locked --workspace --all-targets -- -D warnings
check 'Book listings' ./scripts/build-book.sh --check
check 'Generated Kotlin and Swift bindings' ./scripts/gen-bindings.sh --check
check 'Generated design tokens' python3 -B scripts/gen-tokens.py --check
check 'Scenario evidence' python3 -B scripts/check-prd.py scenarios
gate_finish
