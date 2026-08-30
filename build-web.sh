#!/usr/bin/env bash
# Builds the WASM bridge and its JS bindings into web/pkg.
#
# The frontend runs entirely in the browser against the offline mock, so this needs no
# hardware and no Windows toolchain.
set -euo pipefail

PROFILE="${1:-dev}"
FLAGS=()
OUT="target/wasm32-unknown-unknown/debug/es9_wasm.wasm"
if [ "$PROFILE" = "release" ]; then
  FLAGS+=(--release)
  OUT="target/wasm32-unknown-unknown/release/es9_wasm.wasm"
fi

cargo build -p es9-wasm --target wasm32-unknown-unknown "${FLAGS[@]}"
wasm-bindgen --target web --no-typescript --out-dir web/pkg "$OUT"
echo "built web/pkg from $OUT"
