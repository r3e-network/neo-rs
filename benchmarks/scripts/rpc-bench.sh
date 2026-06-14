#!/usr/bin/env bash
#
# rpc-bench.sh — run the bench-client load generator against several running
# nodes and print a side-by-side comparison table.
#
# Usage:
#   rpc-bench.sh [--scenario block-read|count|header-read] [--duration 30]
#                [--concurrency 64] [--max-height 500000] [--warmup 3]
#                name1=URL1 name2=URL2 ...
#
# Example:
#   rpc-bench.sh --scenario block-read --duration 30 --concurrency 64 \
#       neo-rs=http://127.0.0.1:10332 \
#       neo-cli=http://127.0.0.1:20332 \
#       neo-go=http://127.0.0.1:30332
#
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$HERE/lib.sh"

SCENARIO="block-read" DURATION=30 CONCURRENCY=64 MAXH=500000 WARMUP=3
TARGETS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --scenario) SCENARIO="$2"; shift 2;;
    --duration) DURATION="$2"; shift 2;;
    --concurrency) CONCURRENCY="$2"; shift 2;;
    --max-height) MAXH="$2"; shift 2;;
    --warmup) WARMUP="$2"; shift 2;;
    *=*) TARGETS+=("$1"); shift;;
    *) die "unknown arg: $1";;
  esac
done
[ "${#TARGETS[@]}" -ge 1 ] || die "give at least one name=URL target"

BIN="$HERE/../bench-client/target/release/neo-bench"
if [ ! -x "$BIN" ]; then
  log "building bench-client (release)..."
  ( cd "$HERE/../bench-client" && cargo build --release )
fi

RESULTS="$HERE/../results"
mkdir -p "$RESULTS"
JSON_OUT="$RESULTS/rpc-${SCENARIO}.jsonl"
: > "$JSON_OUT"

for t in "${TARGETS[@]}"; do
  NAME="${t%%=*}"; URL="${t#*=}"
  if [ -z "$(block_count "$URL")" ]; then
    warn "[$NAME] $URL not responding — skipping"
    continue
  fi
  log "[$NAME] $SCENARIO x${CONCURRENCY} for ${DURATION}s -> $URL"
  "$BIN" --url "$URL" --scenario "$SCENARIO" --duration "$DURATION" \
         --concurrency "$CONCURRENCY" --max-height "$MAXH" --warmup "$WARMUP" \
         --label "$NAME" --json | tee -a "$JSON_OUT"
done

echo
echo "================ rpc-bench: $SCENARIO (c=$CONCURRENCY, ${DURATION}s) ================"
python3 - "$JSON_OUT" <<'PY'
import sys, json
rows = []
for line in open(sys.argv[1]):
    line = line.strip()
    if line.startswith("{"):
        try: rows.append(json.loads(line))
        except Exception: pass
if not rows:
    print("  (no results)"); sys.exit()
hdr = f"{'impl':<10}{'req/s':>12}{'p50 ms':>10}{'p95 ms':>10}{'p99 ms':>10}{'errors':>9}"
print(hdr); print("-"*len(hdr))
base = max(r["rps"] for r in rows)
for r in sorted(rows, key=lambda r: -r["rps"]):
    rel = f"({r['rps']/base*100:.0f}%)"
    print(f"{r['label']:<10}{r['rps']:>12.0f}{r['p50_ms']:>10.2f}{r['p95_ms']:>10.2f}{r['p99_ms']:>10.2f}{r['errors']:>9}  {rel}")
PY
echo "==================================================================="
echo "raw: $JSON_OUT"
