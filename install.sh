#!/usr/bin/env bash
# Build and install:  ts (switch)  tn (new)  tk (kill)  — one binary, three names.
set -euo pipefail
cd "$(dirname "$0")"

BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

cargo build --release
mkdir -p "$BIN_DIR"
install -m755 target/release/ts "$BIN_DIR/ts"
ln -sf ts "$BIN_DIR/tn"
ln -sf ts "$BIN_DIR/tk"

echo "installed ts, tn, tk -> $BIN_DIR"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: $BIN_DIR is not on your PATH" ;;
esac
