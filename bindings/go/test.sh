#!/usr/bin/env bash
# Build libpragmatic_ffi, then run the Go binding's tests.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"

cargo build -p pragmatic-ffi --manifest-path "$ROOT/Cargo.toml"
go test ./...
