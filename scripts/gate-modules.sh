#!/usr/bin/env bash
source "$(dirname "$0")/gate-lib.sh"
check 'Module boundaries, two consumers and tests' python3 -B scripts/check-prd.py modules
check 'Workspace consumers build' cargo build --locked --workspace --all-targets
gate_finish
