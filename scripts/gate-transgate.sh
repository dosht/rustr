#!/usr/bin/env bash
source "$(dirname "$0")/gate-lib.sh"
check 'Rules and parity evidence' python3 -B scripts/check-prd.py ledgers
check 'Transgate server tests' cargo test --locked -p transgate-server
check 'Local runtime HTTP and shutdown' python3 -B scripts/smoke-transgate-runtime.py
check 'Transgate web component tests' npm --prefix transgate/web run test:ci
check 'Transgate desktop and phone flows' npm --prefix transgate/web run e2e
check 'Docker runtime and health' python3 -B scripts/smoke-transgate-runtime.py --docker
check 'Migration parity on a supplied snapshot' required_script transgate/scripts/check-migration.sh
check 'Android signed-in flow on Waydroid' required_script transgate/scripts/check-android.sh
check 'Linux Swift bindings and views compile' required_script transgate/scripts/check-swift.sh
gate_finish
