#!/usr/bin/env bash
# Build libpragmatic_ffi, then run the Node binding's smoke test.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"

cargo build -p pragmatic-ffi --manifest-path "$ROOT/Cargo.toml"
[ -d node_modules ] || npm install
node tests/smoke.js
