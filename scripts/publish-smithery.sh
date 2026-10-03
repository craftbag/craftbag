#!/usr/bin/env bash
# Upload a packed MCPB to Smithery. Soft-skips when SMITHERY_API_KEY is unset.
# Publish with the REST API. The CLI stdio upload returns "No values to set".
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="${VERSION:-}"
if [ -z "$version" ]; then
  echo "VERSION is required (for example 0.2.0)" >&2
  exit 1
fi
version="${version#v}"

if [ -z "${SMITHERY_API_KEY:-}" ]; then
  echo "OK: SMITHERY_API_KEY is unset"
  exit 0
fi

bundle="${MCPB:-$root/target/mcpb/craftbag-mcp-${version}.mcpb}"
if [ ! -f "$bundle" ]; then
  echo "MCPB not found: ${bundle}" >&2
  exit 1
fi

qualified="${SMITHERY_SERVER:-craftbag/craftbag-mcp}"
enc="$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1], safe=""))' "$qualified")"

curl -fsS -X PUT "https://api.smithery.ai/servers/${enc}" \
  -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "{}" >/dev/null

payload="$(python3 - "$version" <<'PY'
import json, sys
version = sys.argv[1]
print(json.dumps({
    "type": "stdio",
    "runtime": "node",
    "displayName": "craftbag",
    "description": "MCP server to list, load, explain, and validate Agent Skills.",
    "serverCard": {
        "serverInfo": {"name": "craftbag-mcp", "version": version},
        "tools": [],
        "prompts": [],
        "resources": [],
    },
}))
PY
)"

curl -fsS -X PUT "https://api.smithery.ai/servers/${enc}/releases" \
  -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
  -H "Accept: application/json" \
  -F "payload=${payload}" \
  -F "bundle=@${bundle};type=application/octet-stream"

curl -fsS -X PATCH "https://api.smithery.ai/servers/${enc}" \
  -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
  -H "Content-Type: application/json" \
  -d '{
    "displayName": "craftbag",
    "description": "MCP server to list, load, explain, and validate Agent Skills.",
    "homepage": "https://docs.rs/craftbag",
    "repositoryUrl": "https://github.com/craftbag/craftbag",
    "license": "Apache-2.0 OR MIT",
    "unlisted": false
  }' >/dev/null

echo "OK: published ${qualified}@${version}"
