#!/usr/bin/env bash
#
# resource-sample.sh — sample RSS + CPU% of a process for a duration.
# Useful to characterise a node while a workload runs against it (CPU-bound vs
# I/O-bound) and to record peak memory.
#
# Usage: resource-sample.sh --pid 12345 [--duration 30] [--interval 1] [--name neo-rs]
#
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$HERE/lib.sh"

PID="" DURATION=30 INTERVAL=1 NAME="node"
while [ $# -gt 0 ]; do
  case "$1" in
    --pid) PID="$2"; shift 2;;
    --duration) DURATION="$2"; shift 2;;
    --interval) INTERVAL="$2"; shift 2;;
    --name) NAME="$2"; shift 2;;
    *) die "unknown arg: $1";;
  esac
done
[ -n "$PID" ] || die "need --pid"
kill -0 "$PID" 2>/dev/null || die "no such process: $PID"

RESULTS="$HERE/../results"; mkdir -p "$RESULTS"
OUT="$RESULTS/resource-${NAME}.csv"
echo "t_s,cpu_pct,rss_kb" > "$OUT"

log "sampling pid $PID ($NAME) for ${DURATION}s @ ${INTERVAL}s"
peak_rss=0; cpu_sum=0; n=0
for (( t=0; t<DURATION; t+=INTERVAL )); do
  kill -0 "$PID" 2>/dev/null || { warn "process gone"; break; }
  read -r cpu rss < <(ps -o %cpu=,rss= -p "$PID" 2>/dev/null | awk '{print $1, $2}')
  cpu="${cpu:-0}"; rss="${rss:-0}"
  echo "$t,$cpu,$rss" >> "$OUT"
  [ "${rss%.*}" -gt "$peak_rss" ] && peak_rss="${rss%.*}"
  cpu_sum=$(python3 -c "print($cpu_sum + $cpu)")
  n=$((n+1))
  sleep "$INTERVAL"
done

avg_cpu=$(python3 -c "print(f'{$cpu_sum/$n:.1f}' if $n else '0')")
echo
echo "================ resources: $NAME ================"
printf "  avg CPU  : %s %%\n" "$avg_cpu"
printf "  peak RSS : %s MB\n" "$(python3 -c "print(f'{$peak_rss/1024:.0f}')")"
echo "  samples  : $OUT"
echo "================================================="
