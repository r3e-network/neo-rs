#!/usr/bin/env bash
# Shared helpers for the cross-implementation benchmark scripts.
set -euo pipefail

# now_ms — milliseconds since epoch (portable across macOS/Linux).
now_ms() { python3 -c 'import time;print(int(time.time()*1000))'; }

# rpc_call <url> <method> [params-json]  -> raw JSON response on stdout.
rpc_call() {
  local url="$1" method="$2" params="${3:-[]}"
  curl -s --max-time 10 -X POST "$url" \
    -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":$params}"
}

# block_count <url> -> integer height, or empty string if unreachable.
block_count() {
  local url="$1" resp
  resp="$(rpc_call "$url" getblockcount 2>/dev/null || true)"
  printf '%s' "$resp" | python3 -c 'import sys,json
try:
    print(json.load(sys.stdin)["result"])
except Exception:
    print("")' 2>/dev/null || printf ''
}

# wait_for_rpc <url> <timeout_s> -> 0 when the node answers, 1 on timeout.
wait_for_rpc() {
  local url="$1" timeout="${2:-120}" start
  start="$(date +%s)"
  while :; do
    [ -n "$(block_count "$url")" ] && return 0
    [ "$(( $(date +%s) - start ))" -ge "$timeout" ] && return 1
    sleep 1
  done
}

# peak_rss_kb <pid> — print the current RSS (KB) of a process tree (pid + children).
peak_rss_kb() {
  local pid="$1"
  # sum RSS of the pid and any descendants
  ps -o rss= -p "$pid" 2>/dev/null | awk '{s+=$1} END{print s+0}'
}

log()  { printf '\033[0;36m[bench]\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[0;33m[bench]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[0;31m[bench] %s\033[0m\n' "$*" >&2; exit 1; }
