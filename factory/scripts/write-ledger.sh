#!/usr/bin/env bash
# CAS write STATE + append LEDGER on factory/ledger.
# Exit 0 = wrote or crash-replay no-op. Exit 2 = lost race. Exit 1 = hard fail.
set -euo pipefail

echo "PLAN: write factory ledger"

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
  echo "FAIL: not a git checkout"
  echo "DONE: ok=false error=no-git"
  exit 1
fi

LEDGER_WT="${FACTORY_LEDGER_WORKTREE:-${HOME}/craftbag-ledger}"
STATE="$LEDGER_WT/factory/STATE.json"
LEDGER="$LEDGER_WT/factory/LEDGER.jsonl"

if [[ ! -d "$LEDGER_WT/.git" && ! -f "$LEDGER_WT/.git" ]]; then
  echo "FAIL: ledger worktree missing at $LEDGER_WT"
  echo "DONE: ok=false error=no-worktree"
  echo "NEXT: git fetch origin factory/ledger && git worktree add $LEDGER_WT factory/ledger"
  exit 1
fi

echo "DO: fetch origin factory/ledger"
git -C "$LEDGER_WT" fetch origin factory/ledger
git -C "$LEDGER_WT" checkout factory/ledger >/dev/null
git -C "$LEDGER_WT" merge --ff-only origin/factory/ledger >/dev/null

if [[ ! -f "$STATE" ]]; then
  echo "FAIL: STATE missing after fetch"
  echo "DONE: ok=false error=no-state"
  exit 1
fi

expected="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("ledger_sha") or "")' "$STATE")"
head="$(git -C "$LEDGER_WT" rev-parse HEAD)"
echo "DO: compare HEAD=$head ledger_sha=${expected:-empty}"
if [[ -n "$expected" && "$head" != "$expected" ]]; then
  echo "WAIT: lost race HEAD!=ledger_sha"
  echo "DONE: ok=false error=cas_mismatch"
  echo "NEXT: re-read STATE and retry once"
  exit 2
fi

# Optional incoming STATE/LEDGER line via env files.
NEW_STATE="${FACTORY_NEW_STATE:-}"
NEW_LINE="${FACTORY_LEDGER_LINE:-}"

if [[ -n "$NEW_STATE" && -f "$NEW_STATE" ]]; then
  last_id="$(python3 -c 'import json,sys; print((json.load(open(sys.argv[1])).get("last_job") or {}).get("id") or "")' "$STATE")"
  last_sha="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("last_sha") or "")' "$STATE")"
  new_id="$(python3 -c 'import json,sys; print((json.load(open(sys.argv[1])).get("last_job") or {}).get("id") or "")' "$NEW_STATE")"
  new_sha="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("last_sha") or "")' "$NEW_STATE")"
  if [[ -n "$last_id" && "$last_id" == "$new_id" && "$last_sha" == "$new_sha" ]]; then
    echo "OK: crash replay last_job.id+last_sha already on HEAD"
    echo "DONE: ok=true noop=1"
    echo "NEXT: next-cycle"
    exit 0
  fi
  cp "$NEW_STATE" "$STATE"
fi

if [[ -n "$NEW_LINE" ]]; then
  mkdir -p "$(dirname "$LEDGER")"
  printf '%s\n' "$NEW_LINE" >> "$LEDGER"
fi

python3 - "$STATE" "$head" <<'PY'
import json, sys
path, head = sys.argv[1], sys.argv[2]
state = json.load(open(path))
state["ledger_sha"] = head
json.dump(state, open(path, "w"), indent=2)
open(path, "a").write("\n")
PY

echo "DO: commit and push --force-with-lease"
git -C "$LEDGER_WT" add factory/STATE.json factory/LEDGER.jsonl
if git -C "$LEDGER_WT" diff --cached --quiet; then
  echo "OK: nothing to commit"
  echo "DONE: ok=true noop=1"
  echo "NEXT: next-cycle"
  exit 0
fi
git -C "$LEDGER_WT" commit -s -m "chore: factory ledger"
if ! git -C "$LEDGER_WT" push --force-with-lease="factory/ledger:${expected:-$head}" origin "factory/ledger"; then
  echo "WAIT: push lease rejected"
  echo "DONE: ok=false error=push_lease"
  echo "NEXT: re-read STATE and retry once"
  exit 2
fi

new_head="$(git -C "$LEDGER_WT" rev-parse HEAD)"
python3 - "$STATE" "$new_head" <<'PY'
import json, sys
path, head = sys.argv[1], sys.argv[2]
state = json.load(open(path))
state["ledger_sha"] = head
json.dump(state, open(path, "w"), indent=2)
open(path, "a").write("\n")
PY

echo "DONE: ok=true head=$new_head"
echo "NEXT: next-cycle"
exit 0
