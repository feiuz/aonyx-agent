#!/usr/bin/env bash
# Build the aonyx agent and stage it as the Tauri sidecar (externalBin) for the
# host target triple, plus next to the desktop dev binary so `tauri dev` resolves
# it too. CI must run this (per target) before `tauri build`. (ADR-016 / W1.)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
EXT=""
case "$TRIPLE" in *windows*) EXT=".exe" ;; esac

echo "Building aonyx (--features api,rag) for $TRIPLE ..."
( cd "$ROOT" && cargo build --release -p aonyx-agent --features api,rag )

BIN="$ROOT/target/release/aonyx$EXT"
DEST="$ROOT/desktop/src-tauri/binaries"
mkdir -p "$DEST"
cp "$BIN" "$DEST/aonyx-$TRIPLE$EXT"
echo "staged -> $DEST/aonyx-$TRIPLE$EXT"

# Dev convenience: place it next to the desktop dev binary too (current_exe dir).
DEV="$ROOT/desktop/src-tauri/target/debug"
if [ -d "$DEV" ]; then
  cp "$BIN" "$DEV/aonyx$EXT"
  echo "staged -> $DEV/aonyx$EXT"
fi
