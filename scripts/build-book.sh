#!/usr/bin/env bash
# Builds book/build/core-and-shells.{epub,pdf}. Needs node, pandoc 3.8+ and a
# Chromium (Playwright's is found automatically, or set CHROME_PATH).
set -euo pipefail
cd "$(dirname "$0")/../book"
[[ -d node_modules ]] || npm install --no-fund --no-audit
node tools/build.mjs "$@"
