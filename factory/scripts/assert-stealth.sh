#!/usr/bin/env bash
# Check that craftbag stays public-but-quiet.
# Usage: assert-stealth.sh OWNER/REPO
# Exit 0 quiet, 1 leak, 2 cannot query.
set -euo pipefail

echo "PLAN: assert stealth metadata for ${1:-missing}"

if [[ $# -ne 1 || "$1" != */* ]]; then
  echo "FAIL: usage: assert-stealth.sh OWNER/REPO"
  echo "DONE: ok=false error=usage"
  exit 2
fi

repo="$1"
owner="${repo%%/*}"
leaks=0

if ! command -v gh >/dev/null 2>&1; then
  echo "FAIL: gh not on PATH"
  echo "DONE: ok=false error=no-gh"
  exit 2
fi

echo "DO: query GitHub repo About"
if ! repo_json="$(gh repo view "$repo" --json description,homepageUrl,repositoryTopics,isPrivate 2>/dev/null)"; then
  echo "FAIL: gh repo view $repo"
  echo "DONE: ok=false error=gh-repo-view"
  exit 2
fi

check_empty() {
  local label="$1" value="$2"
  if [[ -n "$value" && "$value" != "null" ]]; then
    echo "FAIL: $label is set: $value"
    leaks=$((leaks + 1))
  else
    echo "OK: $label empty"
  fi
}

desc="$(printf '%s' "$repo_json" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("description") or "")')"
home="$(printf '%s' "$repo_json" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("homepageUrl") or "")')"
topics="$(printf '%s' "$repo_json" | python3 -c 'import json,sys; t=json.load(sys.stdin).get("repositoryTopics") or []; print(",".join(x.get("name","") if isinstance(x,dict) else str(x) for x in t))')"
private="$(printf '%s' "$repo_json" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("isPrivate"))')"

check_empty "description" "$desc"
check_empty "homepage" "$home"
check_empty "topics" "$topics"

if [[ "$private" == "True" || "$private" == "true" ]]; then
  echo "FAIL: repo is private (no free public Actions)"
  leaks=$((leaks + 1))
else
  echo "OK: repo is public"
fi

echo "DO: query org About if $owner is an org"
if org_json="$(gh api "orgs/$owner" --jq '{desc:.description,blog:.blog,twitter:.twitter_username,loc:.location,email:.email}' 2>/dev/null)"; then
  for key in desc blog twitter loc email; do
    val="$(printf '%s' "$org_json" | python3 -c "import json,sys; print(json.load(sys.stdin).get('$key') or '')")"
    check_empty "org.$key" "$val"
  done
else
  echo "OK: $owner is not an org (or no org read); skipped"
fi

echo "DO: scan local tree"
if [[ -f README.md ]]; then
  if grep -Eiq 'shields.io|img.shields|badge|\[ci\]|scorecard|bestpractices.dev' README.md; then
    echo "FAIL: README has badges or CI marketing"
    leaks=$((leaks + 1))
  fi
  if grep -Eiq 'agent skills|getting started|install|quickstart' README.md; then
    echo "FAIL: README looks like a product pitch"
    leaks=$((leaks + 1))
  else
    echo "OK: README has no pitch markers"
  fi
fi

if [[ -f .github/FUNDING.yml ]]; then
  echo "FAIL: .github/FUNDING.yml present"
  leaks=$((leaks + 1))
fi

if [[ -f Cargo.toml ]]; then
  kw="$(python3 - <<'PY'
import pathlib, re
t = pathlib.Path("Cargo.toml").read_text()
m = re.search(r"(?m)^keywords\s*=\s*\[([^\]]*)\]", t)
print((m.group(1) if m else "").strip())
PY
)"
  catg="$(python3 - <<'PY'
import pathlib, re
t = pathlib.Path("Cargo.toml").read_text()
m = re.search(r"(?m)^categories\s*=\s*\[([^\]]*)\]", t)
print((m.group(1) if m else "").strip())
PY
)"
  check_empty "Cargo.toml keywords" "$kw"
  check_empty "Cargo.toml categories" "$catg"
fi

if [[ "$leaks" -gt 0 ]]; then
  echo "DONE: ok=false leaks=$leaks"
  echo "NEXT: clear the FAIL fields; do not add topics or a pitch README"
  exit 1
fi

echo "DONE: ok=true leaks=0"
echo "NEXT: keep factory going; do not add discovery metadata"
exit 0
