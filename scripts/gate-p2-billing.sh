#!/usr/bin/env bash
source "$(dirname "$0")/gate-lib.sh"
if [[ -z "${BILLING_TEST_DATABASE_URL:-}" ]]; then
  printf '%s\n' 'FAIL: BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database for P2 billing proof.' >&2
  exit 1
fi
check 'Billing boundary, two consumers and database tests' python3 -B scripts/check-prd.py modules --module billing
check 'All 123 billing rules and named executable evidence' python3 -B scripts/check-prd.py billing
check 'Workspace consumers build' cargo build --locked --workspace --all-targets
gate_finish
