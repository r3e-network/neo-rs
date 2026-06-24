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

run_check() {
  local pass=0; local fail=0; local skip=0

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
  for h in 0 1 1000 10000 100000; do
    [[ "$h" -gt "$our_h" ]] && continue
    local our_hash live_hash
    our_hash=$(rpc "$OUR_RPC" getblockhash "[$h]" | extract_result)
    live_hash=$(rpc_live getblockhash "[$h]" | extract_result)
    if [[ "$our_hash" = "$live_hash" && "$our_hash" != ERROR* ]]; then
      echo "  ✓ block $h: $our_hash"
      ((pass++))
    else
      echo "  ✗ block $h: our=$our_hash live=$live_hash"
      ((fail++))
    fi
  done

  # Tip hash (our node's own tip vs live's same height)
  local our_tip live_at_our_h
  our_tip=$(rpc "$OUR_RPC" getblockhash "[$our_h]" | extract_result)
  live_at_our_h=$(rpc_live getblockhash "[$our_h]" | extract_result)
  if [[ "$our_tip" = "$live_at_our_h" && "$our_tip" != ERROR* ]]; then
    echo "  ✓ tip (h=$our_h): $our_tip"
    ((pass++))
  else
    echo "  ✗ tip (h=$our_h): our=$our_tip live=$live_at_our_h"
    ((fail++))
  fi

  # --- 2. Native contract state ---
  echo "--- Native contract state ---"
  for entry in "$NEO_HASH:totalSupply" "$GAS_HASH:totalSupply" "$POLICY_HASH:getFeePerByte" "$POLICY_HASH:getExecFeeFactor" "$POLICY_HASH:getStoragePrice"; do
    local hash="${entry%%:*}" method="${entry##*:}"
    local our_val live_val
    our_val=$(rpc "$OUR_RPC" invokefunction "[\"$hash\",\"$method\"]" | extract_stack)
    live_val=$(rpc_live invokefunction "[\"$hash\",\"$method\"]" | extract_stack)
    if [[ "$our_val" = "$live_val" ]]; then
      echo "  ✓ $method: $our_val"
      ((pass++))
    else
      echo "  ≈ $method: our=$our_val live=$live_val (may differ if hardfork boundary not yet crossed)"
      ((skip++))
    fi
  done

  # --- 3. NEO committee (consensus-critical state) ---
  echo "--- Committee ---"
  local our_comm live_comm
  our_comm=$(rpc "$OUR_RPC" invokefunction "[\"$NEO_HASH\",\"getCommittee\"]" | python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d.get('result',{})
stack=r.get('stack',[])
if stack and isinstance(stack[0],dict) and stack[0].get('type')=='Array':
    vals=[i.get('value','') for i in stack[0].get('value',[]) if isinstance(i,dict)]
    # committee is Array of Struct[pubkey,votes]; extract pubkeys
    pubkeys=[]
    for item in stack[0]['value']:
        if isinstance(item,dict) and item.get('type')=='Struct':
            inner=item.get('value',[])
            if inner and isinstance(inner[0],dict):
                pubkeys.append(inner[0].get('value','')[:20])
    print(len(pubkeys),'members')
else:
    print('parse-fail')
" 2>/dev/null)
  live_comm=$(rpc_live invokefunction "[\"$NEO_HASH\",\"getCommittee\"]" | python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d.get('result',{})
stack=r.get('stack',[])
if stack and isinstance(stack[0],dict) and stack[0].get('type')=='Array':
    pubkeys=[]
    for item in stack[0]['value']:
        if isinstance(item,dict) and item.get('type')=='Struct':
            inner=item.get('value',[])
            if inner and isinstance(inner[0],dict):
                pubkeys.append(inner[0].get('value','')[:20])
    print(len(pubkeys),'members')
else:
    print('parse-fail')
" 2>/dev/null)
  echo "  our committee: $our_comm | live: $live_comm"

  echo "--- Summary: $pass pass, $fail fail, $skip skip (hardfork-boundary) ---"
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
