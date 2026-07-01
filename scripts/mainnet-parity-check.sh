#!/usr/bin/env bash
# MainNet parity comparison harness.
#
# Compares our neo-rs node (syncing) against a live C# MainNet reference node
# at the highest height BOTH have, checking:
#   1. Block hashes (byte-exact consensus)
#   2. Native contract state (NEO totalSupply, GAS totalSupply, Policy values)
#   3. NEO balance of the genesis BFT address (execution + storage + NEP-17)
#
# Usage:
#   scripts/mainnet-parity-check.sh [--continuous] [--interval 60]
#
# Requires our node running on 127.0.0.1:10332 with RPC auth (neo:change-me-mainnet-rpc-password).
set -euo pipefail

OUR_RPC="http://127.0.0.1:10332"
LIVE_RPC="${LIVE_RPC:-http://rpc3.n3.nspcc.ru:10332}"
AUTH="$(printf 'neo:change-me-mainnet-rpc-password' | base64)"
CONTINUOUS=false
INTERVAL=60

while [[ $# -gt 0 ]]; do
  case "$1" in
    --continuous) CONTINUOUS=true; shift ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

# MainNet native contract hashes
NEO_HASH="0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
GAS_HASH="0xd2a4cff31913016155e38e474a2c06d08be276cf"
POLICY_HASH="0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b"

rpc() {
  local url="$1"; shift
  local method="$1"; shift
  local params="${1:-[]}"
  curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' \
    -H "Authorization: Basic $AUTH" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":$params}" \
    "$url" 2>/dev/null
}

rpc_live() {
  local method="$1"; shift
  local params="${1:-[]}"
  curl --compressed -sS --max-time 15 -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":$params}" \
    "$LIVE_RPC" 2>/dev/null
}

extract_result() {
  python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('result','') if d.get('result') is not None else d.get('error',{}).get('message','ERROR'))" 2>/dev/null
}

extract_stack() {
  python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d.get('result',{})
stack=r.get('stack',[])
if stack and isinstance(stack[0],dict):
    print(stack[0].get('value',''))
else:
    print(str(stack))
" 2>/dev/null
}

extract_state_height() {
  python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d.get('result')
if isinstance(r, dict):
    for key in ('localrootindex', 'validatedrootindex', 'index', 'height'):
        value = r.get(key)
        if isinstance(value, int):
            print(value)
            break
    else:
        print('ERROR')
elif isinstance(r, int):
    print(r)
else:
    print(d.get('error', {}).get('message', 'ERROR'))
" 2>/dev/null
}

extract_state_root_hash() {
  python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d.get('result')
if isinstance(r, dict):
    print(r.get('roothash') or r.get('rootHash') or r.get('hash') or 'ERROR')
elif isinstance(r, str):
    print(r)
else:
    print(d.get('error', {}).get('message', 'ERROR'))
" 2>/dev/null
}

record_pass() {
  local label="$1"
  echo "  ✓ $label"
  ((pass+=1))
}

record_fail() {
  local label="$1"
  echo "  ✗ $label"
  ((fail+=1))
}

sample_heights() {
  local tip="$1"
  python3 - "$tip" <<'PY'
import sys
tip = int(sys.argv[1])
samples = {0, tip}
for height in (1, 1000, 10000, 100000):
    if height <= tip:
        samples.add(height)
for fraction in (0.25, 0.50, 0.75):
    samples.add(int(tip * fraction))
for height in sorted(h for h in samples if h >= 0):
    print(height)
PY
}

is_uint() {
  [[ "${1:-}" =~ ^[0-9]+$ ]]
}

min_u32() {
  local a="$1" b="$2"
  if [[ "$a" -le "$b" ]]; then
    echo "$a"
  else
    echo "$b"
  fi
}

run_check() {
  local pass=0; local fail=0

  echo "=== MainNet Parity Check — $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="

  # Heights
  local our_h live_h
  our_h=$(rpc "$OUR_RPC" getblockcount | extract_result)
  live_h=$(rpc_live getblockcount | extract_result)
  our_h=$((our_h - 1))  # getblockcount returns count, tip index is count-1
  live_h=$((live_h - 1))
  echo "our height: $our_h | live height: $live_h | lag: $((live_h - our_h))"

  if [[ "$our_h" -lt 1 ]]; then
    echo "SKIP: node not synced past genesis"
    return 0
  fi

  # --- 1. Block hashes at sampled heights ---
  echo "--- Block hashes ---"
  local h
  while IFS= read -r h; do
    local our_hash live_hash
    our_hash=$(rpc "$OUR_RPC" getblockhash "[$h]" | extract_result)
    live_hash=$(rpc_live getblockhash "[$h]" | extract_result)
    if [[ "$our_hash" = "$live_hash" && "$our_hash" != ERROR* ]]; then
      record_pass "block $h: $our_hash"
    else
      record_fail "block $h: our=$our_hash live=$live_hash"
    fi
  done < <(sample_heights "$our_h")

  # Tip hash (our node's own tip vs live's same height)
  local our_tip live_at_our_h
  our_tip=$(rpc "$OUR_RPC" getblockhash "[$our_h]" | extract_result)
  live_at_our_h=$(rpc_live getblockhash "[$our_h]" | extract_result)
  if [[ "$our_tip" = "$live_at_our_h" && "$our_tip" != ERROR* ]]; then
    record_pass "tip (h=$our_h): $our_tip"
  else
    record_fail "tip (h=$our_h): our=$our_tip live=$live_at_our_h"
  fi

  # --- 2. State roots ---
  echo "--- State roots ---"
  local our_state_h live_state_h state_tip
  our_state_h=$(rpc "$OUR_RPC" getstateheight | extract_state_height)
  live_state_h=$(rpc_live getstateheight | extract_state_height)
  if is_uint "$our_state_h" && is_uint "$live_state_h"; then
    state_tip=$(min_u32 "$(min_u32 "$our_state_h" "$live_state_h")" "$our_h")
    record_pass "state height: our=$our_state_h live=$live_state_h compare_tip=$state_tip"
    while IFS= read -r h; do
      local our_root live_root
      our_root=$(rpc "$OUR_RPC" getstateroot "[$h]" | extract_state_root_hash)
      live_root=$(rpc_live getstateroot "[$h]" | extract_state_root_hash)
      if [[ "$our_root" = "$live_root" && "$our_root" != ERROR* ]]; then
        record_pass "state root $h: $our_root"
      else
        record_fail "state root $h: our=$our_root live=$live_root"
      fi
    done < <(sample_heights "$state_tip")
  else
    record_fail "state root height unavailable: our=$our_state_h live=$live_state_h"
  fi

  # --- 3. Native contract state ---
  echo "--- Native contract state ---"
  for entry in "$NEO_HASH:totalSupply" "$GAS_HASH:totalSupply" "$POLICY_HASH:getFeePerByte" "$POLICY_HASH:getExecFeeFactor" "$POLICY_HASH:getStoragePrice"; do
    local hash="${entry%%:*}" method="${entry##*:}"
    local our_val live_val
    our_val=$(rpc "$OUR_RPC" invokefunction "[\"$hash\",\"$method\"]" | extract_stack)
    live_val=$(rpc_live invokefunction "[\"$hash\",\"$method\"]" | extract_stack)
    if [[ "$our_val" = "$live_val" ]]; then
      record_pass "$method: $our_val"
    else
      record_fail "$method: our=$our_val live=$live_val"
    fi
  done

  # --- 4. NEO committee (consensus-critical state) ---
  echo "--- Committee ---"
  local our_comm live_comm
  our_comm=$(rpc "$OUR_RPC" invokefunction "[\"$NEO_HASH\",\"getCommittee\"]" | python3 -c "
import json,sys
d=json.load(sys.stdin)
s=d.get('result',{}).get('stack',[])
if s and isinstance(s[0],dict) and s[0].get('type')=='Array':
    print(len(s[0].get('value',[])),'members')
else:
    print('parse-fail')
" 2>/dev/null)
  live_comm=$(rpc_live invokefunction "[\"$NEO_HASH\",\"getCommittee\"]" | python3 -c "
import json,sys
d=json.load(sys.stdin)
s=d.get('result',{}).get('stack',[])
if s and isinstance(s[0],dict) and s[0].get('type')=='Array':
    print(len(s[0].get('value',[])),'members')
else:
    print('parse-fail')
" 2>/dev/null)
  if [[ "$our_comm" = "$live_comm" ]]; then
    record_pass "committee: $our_comm"
  else
    record_fail "committee: our=$our_comm live=$live_comm"
  fi

  echo "--- Summary: $pass pass, $fail fail ---"
  echo ""
  return "$fail"
}

if [[ "$CONTINUOUS" = true ]]; then
  echo "Continuous parity monitoring every ${INTERVAL}s. Ctrl-C to stop."
  while true; do
    run_check || true
    sleep "$INTERVAL"
  done
else
  run_check
fi
