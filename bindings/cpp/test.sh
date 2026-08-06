#!/usr/bin/env bash
# Build libpragmatic_ffi, compile the C++ smoke test against it, run it.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"

cargo build -p pragmatic-ffi --manifest-path "$ROOT/Cargo.toml"

case "$(uname -s)" in
    Darwin) RPATH_FLAG="-Wl,-rpath,$ROOT/target/debug" ;;
    *) RPATH_FLAG="-Wl,-rpath=$ROOT/target/debug" ;;
esac

mkdir -p build
c++ -std=c++17 -Wall -Wextra -Werror \
    -I "$ROOT/crates/pragmatic-ffi/include" \
    tests/smoke.cpp \
    -L "$ROOT/target/debug" -lpragmatic_ffi "$RPATH_FLAG" \
    -o build/smoke

./build/smoke
