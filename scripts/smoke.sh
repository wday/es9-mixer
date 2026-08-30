#!/usr/bin/env bash
# Builds Node bindings for the WASM bridge and runs the end-to-end smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build -p es9-wasm --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --no-typescript --out-dir target/es9node \
  target/wasm32-unknown-unknown/debug/es9_wasm.wasm
node web/smoke.mjs
