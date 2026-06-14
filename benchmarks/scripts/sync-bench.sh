#!/usr/bin/env bash
#
# sync-bench.sh — measure block-import / sync throughput for one Neo node.
#
# Launches a node command (which should import an .acc file or sync), polls its
# RPC height over time, and reports blocks/sec, total wall-time, peak RSS and
# startup latency. This is the fairest cross-implementation metric: it exercises
# the whole consensus hot path (deserialize -> verify -> VM -> state write) with
# no network variance when fed an offline .acc.
#
# Usage:
#   sync-bench.sh --name neo-rs \
#                 --cmd "target/release/neo-node --config config/mainnet.toml --import chain.acc" \
#                 --rpc http://127.0.0.1:10332 \
#                 --target-height 500000 \
#                 [--timeout 7200] [--poll 2]
#
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$HERE/lib.sh"

NAME="" CMD="" RPC="http://127.0.0.1:10332" TARGET=0 TIMEOUT=7200 POLL=2
while [ $# -gt 0 ]; do
  case "$1" in
    --name) NAME="$2"; shift 2;;
    --cmd) CMD="$2"; shift 2;;
    --rpc) RPC="$2"; shift 2;;
    --target-height) TARGET="$2"; shift 2;;
    --timeout) TIMEOUT="$2"; shift 2;;
    --poll) POLL="$2"; shift 2;;
    *) die "unknown arg: $1";;
  esac
done
[ -n "$NAME" ] && [ -n "$CMD" ] && [ "$TARGET" -gt 0 ] || die "need --name, --cmd, --target-height"

RESULTS="$HERE/../results"
mkdir -p "$RESULTS"
OUT="$RESULTS/sync-${NAME}.csv"
echo "elapsed_s,height,rss_kb" > "$OUT"

log "launching [$NAME]: $CMD"
# shellcheck disable=SC2086
$CMD >"$RESULTS/sync-${NAME}.log" 2>&1 &
NODE_PID=$!
trap 'kill "$NODE_PID" 2>/dev/null || true' EXIT

START_MS="$(now_ms)"
READY_MS=0
PEAK_RSS=0
LAST_H=0

while :; do
  # node died?
  if ! kill -0 "$NODE_PID" 2>/dev/null; then
    warn "[$NAME] process exited before reaching target (see sync-${NAME}.log)"
    break
  fi

  ELAPSED=$(( ($(now_ms) - START_MS) / 1000 ))
  [ "$ELAPSED" -ge "$TIMEOUT" ] && { warn "[$NAME] timeout"; break; }

  H="$(block_count "$RPC")"
  RSS="$(peak_rss_kb "$NODE_PID")"
  [ "${RSS:-0}" -gt "$PEAK_RSS" ] && PEAK_RSS="$RSS"

  if [ -n "$H" ]; then
    [ "$READY_MS" -eq 0 ] && READY_MS="$(now_ms)" && log "[$NAME] RPC ready after $(( (READY_MS-START_MS) ))ms"
    echo "$ELAPSED,$H,$RSS" >> "$OUT"
    LAST_H="$H"
    if [ "$H" -ge "$TARGET" ]; then
      log "[$NAME] reached target height $TARGET"
      break
    fi
  fi
  sleep "$POLL"
done

END_MS="$(now_ms)"
WALL_S=$(python3 -c "print(($END_MS-$START_MS)/1000)")
STARTUP_S=$(python3 -c "print(($READY_MS-$START_MS)/1000 if $READY_MS else -1)")
BPS=$(python3 -c "print(f'{$LAST_H/(($END_MS-$START_MS)/1000):.1f}' if $LAST_H else '0')")

echo
echo "================ sync-bench: $NAME ================"
printf "  blocks imported : %s\n" "$LAST_H"
printf "  wall time       : %s s\n" "$WALL_S"
printf "  throughput      : %s blocks/s\n" "$BPS"
printf "  startup->ready  : %s s\n" "$STARTUP_S"
printf "  peak RSS        : %s MB\n" "$(python3 -c "print(f'{$PEAK_RSS/1024:.0f}')")"
echo "  samples         : $OUT"
echo "=================================================="

# Append a one-line summary for results aggregation.
SUMMARY="$RESULTS/sync-summary.csv"
[ -f "$SUMMARY" ] || echo "name,blocks,wall_s,blocks_per_s,startup_s,peak_rss_mb" > "$SUMMARY"
echo "$NAME,$LAST_H,$WALL_S,$BPS,$STARTUP_S,$(python3 -c "print(f'{$PEAK_RSS/1024:.0f}')")" >> "$SUMMARY"
