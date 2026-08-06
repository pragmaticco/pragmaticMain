#!/usr/bin/env bash
# Build libpragmatic_ffi, compile the Java binding + smoke test, run it.
# Needs JDK 22+ (java.lang.foreign).
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"

cargo build -p pragmatic-ffi --manifest-path "$ROOT/Cargo.toml"

case "$(uname -s)" in
    Darwin) LIB="$ROOT/target/debug/libpragmatic_ffi.dylib" ;;
    *) LIB="$ROOT/target/debug/libpragmatic_ffi.so" ;;
esac

mkdir -p build
javac -d build pragmatic/Pragmatic.java tests/Smoke.java
java --enable-native-access=ALL-UNNAMED \
    -Dpragmatic.library="$LIB" \
    -cp build Smoke
