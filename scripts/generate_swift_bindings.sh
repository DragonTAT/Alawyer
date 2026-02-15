#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CORE_DIR="$ROOT_DIR/alawyer-core"
OUT_DIR="$ROOT_DIR/Alawyer/Sources/Generated"

export PATH="/opt/homebrew/bin:$PATH"

cd "$CORE_DIR"
cargo build --lib
cargo run --features bindgen-cli --bin uniffi-bindgen -- \
  generate \
  --library \
  --metadata-no-deps \
  --language swift \
  --out-dir "$OUT_DIR" \
  --crate alawyer_core \
  target/debug/libalawyer_core.dylib

mkdir -p "$OUT_DIR/ffi"
cp "$OUT_DIR/alawyer_coreFFI.h" "$OUT_DIR/ffi/alawyer_coreFFI.h"
cp "$OUT_DIR/alawyer_coreFFI.modulemap" "$OUT_DIR/ffi/module.modulemap"

echo "Swift bindings generated at $OUT_DIR"
