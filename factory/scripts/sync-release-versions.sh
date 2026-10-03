#!/usr/bin/env bash
# After release-please bumps Cargo.toml versions, align path-deps and
# the excluded fuzz lock. Does not re-resolve the whole graph.
set -euo pipefail

ROOT="${SYNC_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$ROOT"

echo "PLAN: align crate versions and refresh locks"
python3 - <<'PY'
import pathlib
import re

root = pathlib.Path(".")
text = (root / "Cargo.toml").read_text(encoding="utf-8")
match = re.search(r'(?m)^version = "([^"]+)"', text)
if not match:
    raise SystemExit("FAIL: root package version missing")
version = match.group(1)
print("DO: version", version)
for rel in ("crates/craftbag-cli/Cargo.toml", "crates/craftbag-mcp/Cargo.toml"):
    path = root / rel
    body = path.read_text(encoding="utf-8")
    updated = re.sub(
        r'(?m)^version = "[^"]+"',
        f'version = "{version}"',
        body,
        count=1,
    )
    updated = re.sub(
        r'craftbag = \{ version = "[^"]+"',
        f'craftbag = {{ version = "{version}"',
        updated,
        count=1,
    )
    if updated != body:
        path.write_text(updated, encoding="utf-8")
        print("DO: wrote", rel)
PY

echo "DO: cargo check -p craftbag"
cargo check -p craftbag
echo "DO: cargo check -p craftbag-cli"
cargo check -p craftbag-cli
echo "DO: cargo check -p craftbag-mcp"
cargo check -p craftbag-mcp
echo "DO: cargo metadata --locked"
cargo metadata --locked --format-version 1 >/dev/null
if [[ -f fuzz/Cargo.toml ]]; then
  echo "DO: cargo check --manifest-path fuzz/Cargo.toml"
  if ! cargo check --manifest-path fuzz/Cargo.toml; then
    echo "DO: retry fuzz lock refresh with nightly"
    rustup toolchain install nightly --profile minimal
    cargo +nightly check --manifest-path fuzz/Cargo.toml
  fi
fi
echo "DONE: locks match crate versions"
