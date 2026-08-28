#!/usr/bin/env bash
# Prepare an isolated project tree and a local VHS tape for the README GIF.
set -euo pipefail

echo "PLAN: build craftbag, copy demo workspace, write local tape"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEMO_ROOT="${DEMO_ROOT:-/tmp/cb-demo}"
DEMO_HOME="${DEMO_HOME:-/tmp/cb-demo-home}"
TAPE_OUT="${TAPE_OUT:-/tmp/craftbag-demo.tape}"
BIN="$ROOT/target/debug/craftbag"

echo "DO: cargo build -p craftbag --locked"
if [[ ! -x "$BIN" ]]; then
  (cd "$ROOT" && cargo build -p craftbag --locked)
else
  (cd "$ROOT" && cargo build -p craftbag --locked --quiet)
fi
echo "OK: $BIN"

echo "DO: copy demo workspace to $DEMO_ROOT"
rm -rf "$DEMO_ROOT" "$DEMO_HOME"
mkdir -p "$DEMO_HOME"
cp -R "$ROOT/demo/workspace/." "$DEMO_ROOT/"
echo "OK: workspace at $DEMO_ROOT"

echo "DO: write $TAPE_OUT"
sed -e "s|__REPO__|$ROOT|g" -e "s|__DEMO__|$DEMO_ROOT|g" -e "s|__BIN_DIR__|$(dirname "$BIN")|g" \
  "$ROOT/demo/demo.tape" >"$TAPE_OUT"
echo "OK: tape $TAPE_OUT"

echo "DONE: ok=true bin=$BIN demo=$DEMO_ROOT tape=$TAPE_OUT"
echo "NEXT: HOME=$DEMO_HOME PATH=$(dirname "$BIN"):\$PATH vhs $TAPE_OUT"
