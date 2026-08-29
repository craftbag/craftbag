#!/usr/bin/env bash
# Publish workspace crates in dependency order.
# Skip a crate/version that is already on crates.io. Retry HTTP 429.
# New crate names are spaced 10 minutes (crates.io first-publish limit).
set -euo pipefail

UA="craftbag-publish/0.1 (https://github.com/craftbag/craftbag)"
CRATES=(craftbag craftbag-cli craftbag-mcp)

crate_version() {
  local manifest="$1"
  python3 -c '
import sys
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if line.startswith("version") and "=" in line:
        print(line.split("=", 1)[1].strip().strip("\""))
        break
else:
    sys.exit("missing version in " + sys.argv[1])
' "$manifest"
}

manifest_for() {
  case "$1" in
    craftbag) echo "Cargo.toml" ;;
    craftbag-cli) echo "crates/craftbag-cli/Cargo.toml" ;;
    craftbag-mcp) echo "crates/craftbag-mcp/Cargo.toml" ;;
    *) echo "unknown crate $1" >&2; return 1 ;;
  esac
}

crates_io_get() {
  local path="$1"
  local tmp code
  tmp=$(mktemp)
  code=$(curl -sS -o "$tmp" -w "%{http_code}" -A "$UA" \
    "https://crates.io/api/v1/crates/${path}")
  rm -f "$tmp"
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    429) return 2 ;;
    *)
      echo "FAIL: crates.io GET ${path} HTTP ${code}"
      return 3
      ;;
  esac
}

already_on_crates_io() {
  crates_io_get "${1}/${2}"
}

crate_name_on_crates_io() {
  crates_io_get "${1}"
}

# 0 = sleep 600s after a successful first upload of this name.
# Version bumps of a name that already exists do not space.
should_space_new_crate() {
  local name_existed="$1" has_next="$2"
  [[ "$has_next" == "1" && "$name_existed" == "0" ]]
}

if [[ "${1:-}" == "--self-test" ]]; then
  echo "PLAN: publish-crates self-test"
  fail=0
  [[ "${#CRATES[@]}" -eq 3 ]] || fail=1
  [[ "${CRATES[0]}" == "craftbag" ]] || fail=1
  for c in "${CRATES[@]}"; do
    m=$(manifest_for "$c") || fail=1
    v=$(crate_version "$m") || fail=1
    [[ -n "$v" ]] || fail=1
  done
  lib_v=$(crate_version Cargo.toml)
  cli_v=$(crate_version crates/craftbag-cli/Cargo.toml)
  mcp_v=$(crate_version crates/craftbag-mcp/Cargo.toml)
  [[ "$lib_v" == "$cli_v" && "$lib_v" == "$mcp_v" ]] || fail=1
  should_space_new_crate 0 1 || fail=1
  should_space_new_crate 1 1 && fail=1
  should_space_new_crate 0 0 && fail=1
  should_space_new_crate 1 0 && fail=1
  if [[ "$fail" -ne 0 ]]; then
    echo "FAIL: crate order, versions, or name-spacing"
    echo "DONE: ok=false error=self-test"
    exit 1
  fi
  echo "OK: order craftbag then cli then mcp; versions match ${lib_v}"
  echo "OK: space only a new crate name before the next crate"
  echo "DONE: ok=true"
  exit 0
fi

echo "PLAN: publish ${CRATES[*]}"
if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "FAIL: CARGO_REGISTRY_TOKEN is unset"
  echo "DONE: ok=false error=missing-token"
  exit 1
fi

last_index=$((${#CRATES[@]} - 1))
for i in "${!CRATES[@]}"; do
  crate="${CRATES[$i]}"
  manifest=$(manifest_for "$crate")
  version=$(crate_version "$manifest")
  echo "DO: ${crate} ${version}"
  name_existed=0
  name_retries=0
  while true; do
    set +e
    crate_name_on_crates_io "$crate"
    nst=$?
    set -e
    if [[ "$nst" -eq 0 ]]; then
      name_existed=1
      break
    fi
    if [[ "$nst" -eq 1 ]]; then
      name_existed=0
      break
    fi
    if [[ "$nst" -eq 3 ]]; then
      echo "DONE: ok=false error=crates-io-get"
      exit 1
    fi
    name_retries=$((name_retries + 1))
    if [[ "$name_retries" -gt 6 ]]; then
      echo "FAIL: crates.io 429 on name pre-check for ${crate}"
      echo "DONE: ok=false error=rate-limit"
      exit 1
    fi
    echo "WAIT: crates.io 429 name pre-check, retry ${name_retries}"
    sleep 60
  done
  retries=0
  while true; do
    set +e
    already_on_crates_io "$crate" "$version"
    st=$?
    set -e
    if [[ "$st" -eq 0 ]]; then
      echo "OK: ${crate} ${version} already on crates.io"
      break
    fi
    if [[ "$st" -eq 3 ]]; then
      echo "DONE: ok=false error=crates-io-get"
      exit 1
    fi
    if [[ "$st" -eq 2 ]]; then
      retries=$((retries + 1))
      if [[ "$retries" -gt 6 ]]; then
        echo "FAIL: crates.io 429 on pre-check for ${crate}"
        echo "DONE: ok=false error=rate-limit"
        exit 1
      fi
      echo "WAIT: crates.io 429 pre-check, retry ${retries}"
      sleep 60
      continue
    fi
    set +e
    pub_out=$(cargo publish -p "$crate" --locked 2>&1)
    pub_st=$?
    set -e
    printf '%s\n' "$pub_out"
    if [[ "$pub_st" -eq 0 ]]; then
      echo "OK: published ${crate} ${version}"
      has_next=0
      if [[ "$i" -lt "$last_index" ]]; then
        has_next=1
      fi
      if should_space_new_crate "$name_existed" "$has_next"; then
        echo "WAIT: 600s before the next new crate name"
        sleep 600
      elif [[ "$has_next" -eq 1 ]]; then
        echo "OK: skip 600s wait; crate name already on crates.io"
      fi
      break
    fi
    if printf '%s\n' "$pub_out" | grep -qiE 'already exist|already uploaded'; then
      echo "OK: ${crate} ${version} already on crates.io"
      break
    fi
    retries=$((retries + 1))
    if [[ "$retries" -gt 6 ]]; then
      echo "FAIL: cargo publish ${crate}"
      echo "DONE: ok=false error=publish"
      exit 1
    fi
    echo "WAIT: cargo publish failed, retry ${retries}"
    sleep 60
  done
done

echo "DONE: ok=true"
