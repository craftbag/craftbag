#!/usr/bin/env bash
# Stamp the MCPB version into a temp tree and zip it.
# The committed mcpb/ tree stays unchanged.
# VERSION overrides the root Cargo.toml version.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [ "${1:-}" = "--self-test" ]; then
  version="0.0.0-test"
  stage="$(mktemp -d "${TMPDIR:-/tmp}/craftbag-mcpb.XXXXXX")"
  trap 'rm -rf "$stage"' EXIT
  VERSION="$version" OUT="$stage/bundle.mcpb" bash "$0"
  python3 - "$stage/bundle.mcpb" "$version" <<'PY' || exit 1
import json, sys, zipfile
path, version = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(path) as zf:
    names = set(zf.namelist())
    manifest = json.loads(zf.read("manifest.json"))
    launcher = zf.read("server/run.mjs").decode()
need = {"manifest.json", "server/run.mjs", "package.json"}
missing = need - names
if missing:
    raise SystemExit(f"missing {sorted(missing)}")
if manifest["version"] != version:
    raise SystemExit(f"version {manifest['version']}")
if manifest["server"]["mcp_config"]["command"] != "craftbag-mcp":
    raise SystemExit("command drifted")
if "npx" in launcher:
    raise SystemExit("launcher must not call npx")
print("DONE: ok=true")
PY
  exit 0
fi

version="${VERSION:-}"
if [ -z "$version" ]; then
  version="$(python3 - <<'PY'
from pathlib import Path
import re
text = Path("Cargo.toml").read_text(encoding="utf-8")
match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
if not match:
    raise SystemExit("could not parse version from Cargo.toml")
print(match.group(1))
PY
)"
fi

if ! command -v zip >/dev/null 2>&1; then
  echo "zip is required to pack the MCPB archive" >&2
  exit 1
fi

stage="$(mktemp -d "${TMPDIR:-/tmp}/craftbag-mcpb.XXXXXX")"
cleanup() { rm -rf "$stage"; }
trap cleanup EXIT
mkdir -p "$stage/server"
cp mcpb/server/run.mjs "$stage/server/run.mjs"
cp mcpb/.mcpbignore "$stage/.mcpbignore"
python3 - "$stage" "$version" <<'PY'
import json
import pathlib
import sys

stage = pathlib.Path(sys.argv[1])
version = sys.argv[2]
manifest = json.loads(pathlib.Path("mcpb/manifest.json").read_text(encoding="utf-8"))
manifest["version"] = version
package = json.loads(pathlib.Path("mcpb/package.json").read_text(encoding="utf-8"))
package["version"] = version
(stage / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
(stage / "package.json").write_text(json.dumps(package, indent=2) + "\n", encoding="utf-8")
desc = manifest["description"]
if len(desc) > 100:
    raise SystemExit(f"manifest description is {len(desc)} chars")
if manifest["server"]["mcp_config"]["command"] != "craftbag-mcp":
    raise SystemExit("mcp_config command must stay craftbag-mcp")
PY

out="${OUT:-$root/target/mcpb/craftbag-mcp-${version}.mcpb}"
mkdir -p "$(dirname "$out")"
rm -f "$out"
(
  cd "$stage"
  zip -q -r "$out" manifest.json package.json server .mcpbignore
)
echo "OK: packed ${out}"
