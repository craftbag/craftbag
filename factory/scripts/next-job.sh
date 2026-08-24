#!/usr/bin/env bash
# Read factory STATE and print one JSON job. Under 10s.
# Exit 0 = job printed. Exit 2 = not-ready (lease or human gate).
# Exit 1 = hard fail.
set -euo pipefail

echo "PLAN: pick next factory job"

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
  echo "FAIL: not a git checkout"
  echo "DONE: ok=false error=no-git"
  exit 1
fi

LEDGER_WT="${FACTORY_LEDGER_WORKTREE:-}"
if [[ -z "$LEDGER_WT" && -f "$ROOT/factory/STATE.json" ]]; then
  LEDGER_WT="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("ledger_worktree") or "")' "$ROOT/factory/STATE.json" 2>/dev/null || true)"
fi
if [[ -z "$LEDGER_WT" ]]; then
  LEDGER_WT="${HOME}/craftbag-ledger"
fi

STATE="$LEDGER_WT/factory/STATE.json"
if [[ ! -f "$STATE" ]]; then
  echo "FAIL: STATE missing at $STATE"
  echo "DONE: ok=false error=no-state"
  echo "NEXT: fetch origin factory/ledger and add a worktree"
  exit 1
fi

echo "DO: read $STATE"
python3 - "$STATE" <<'PY'
import json, os, sys
from datetime import datetime, timezone

path = sys.argv[1]
state = json.load(open(path))
now = datetime.now(timezone.utc)

gate = state.get("human_gate")
if gate:
    print(f"WAIT: human_gate kind={gate.get('kind')} {gate.get('message')}", flush=True)
    print("DONE: ok=false error=human_gate")
    print(f"NEXT: human-gate:{gate.get('kind')}")
    sys.exit(2)

lease = state.get("lease")
if lease and os.environ.get("FACTORY_REAP_LEASE") != "1":
    exp = lease.get("expires_at") or ""
    try:
        exp_dt = datetime.fromisoformat(exp.replace("Z", "+00:00"))
    except ValueError:
        exp_dt = now
    if now < exp_dt:
        print(f"WAIT: lease held job={lease.get('job_id')} until {exp}", flush=True)
        print("DONE: ok=false error=lease_held")
        print("NEXT: reap-lease")
        sys.exit(2)
    print(f"OK: lease expired job={lease.get('job_id')}", flush=True)

src = state.get("next_work_source") or "bootstrap"
cursor = state.get("pr_plan_cursor") or "pr-2"
job = {
    "job_id": f"{src}-{cursor}",
    "work_source": src,
    "pr_plan_cursor": cursor,
    "session_branch": state.get("session_branch"),
    "ready_pr": state.get("ready_pr"),
    "stealth_mode": state.get("stealth_mode", True),
    "app_merge_attribution": (state.get("ci_status") or {}).get("app_merge_attribution", False),
    "mpi_next": (state.get("mpi") or {}).get("next_perspective"),
    "expand_cursor": state.get("expand_cursor", 0),
}
print("OK: picked " + src, flush=True)
print(json.dumps(job), flush=True)
print("DONE: ok=true source=" + src)
print("NEXT: spawn one child for this job")
PY
