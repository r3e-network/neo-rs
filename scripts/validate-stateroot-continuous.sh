#!/usr/bin/env bash
# Continuous state root validation: compares local neo-rs node against official Neo mainnet
# Usage: ./scripts/validate-stateroot-continuous.sh [batch_size] [sleep_between_batches]
set -euo pipefail

LOCAL_RPC="http://127.0.0.1:20332"
REF_RPC="http://seed1.neo.org:10332"
BATCH=${1:-1000}
SLEEP=${2:-5}
LOGFILE="/tmp/stateroot-validation.log"
LAST_VALIDATED_FILE="/tmp/stateroot-last-validated"

# Resume from last validated block
if [[ -f "$LAST_VALIDATED_FILE" ]]; then
    NEXT_BLOCK=$(cat "$LAST_VALIDATED_FILE")
else
    NEXT_BLOCK=0
fi

rpc() {
    local url="$1" method="$2" params="$3"
    curl -s --compressed --max-time 10 -X POST "$url" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[$params],\"id\":1}" 2>/dev/null
}

get_root() {
    local url="$1" idx="$2"
    rpc "$url" "getstateroot" "$idx" | python3 -c "
import sys,json
try:
    r=json.load(sys.stdin)
    print(r.get('result',{}).get('roothash','ERROR'))
except: print('ERROR')" 2>/dev/null
}

get_local_height() {
    rpc "$LOCAL_RPC" "getstateheight" "" | python3 -c "
import sys,json
try:
    r=json.load(sys.stdin)
    v=r.get('result',{}).get('localrootindex')
    print(v if v is not None else 0)
except: print(0)" 2>/dev/null
}

TOTAL_CHECKED=0
TOTAL_MATCH=0
TOTAL_MISMATCH=0
FIRST_MISMATCH=""
START_TIME=$(date +%s)

echo "$(date -Iseconds) Starting continuous state root validation from block $NEXT_BLOCK (batch=$BATCH)" | tee -a "$LOGFILE"

while true; do
    LOCAL_HEIGHT=$(get_local_height)

    if [[ "$NEXT_BLOCK" -ge "$LOCAL_HEIGHT" ]]; then
        echo "$(date -Iseconds) Waiting for sync... local=$LOCAL_HEIGHT next=$NEXT_BLOCK" | tee -a "$LOGFILE"
        sleep 30
        continue
    fi

    END_BLOCK=$((NEXT_BLOCK + BATCH))
    if [[ "$END_BLOCK" -gt "$LOCAL_HEIGHT" ]]; then
        END_BLOCK=$LOCAL_HEIGHT
    fi

    BATCH_MATCH=0
    BATCH_MISMATCH=0
    BATCH_START=$(date +%s)

    for ((i=NEXT_BLOCK; i<END_BLOCK; i++)); do
        local_root=$(get_root "$LOCAL_RPC" "$i")
        ref_root=$(get_root "$REF_RPC" "$i")

        TOTAL_CHECKED=$((TOTAL_CHECKED + 1))

        if [[ "$local_root" == "$ref_root" && "$local_root" != "ERROR" ]]; then
            BATCH_MATCH=$((BATCH_MATCH + 1))
            TOTAL_MATCH=$((TOTAL_MATCH + 1))
        else
            BATCH_MISMATCH=$((BATCH_MISMATCH + 1))
            TOTAL_MISMATCH=$((TOTAL_MISMATCH + 1))
            echo "$(date -Iseconds) MISMATCH block=$i local=$local_root ref=$ref_root" | tee -a "$LOGFILE"
            if [[ -z "$FIRST_MISMATCH" ]]; then
                FIRST_MISMATCH="$i"
            fi
            # Stop on too many mismatches
            if [[ "$TOTAL_MISMATCH" -ge 10 ]]; then
                echo "$(date -Iseconds) ABORT: 10+ mismatches found, first at block $FIRST_MISMATCH" | tee -a "$LOGFILE"
                exit 1
            fi
        fi
    done

    BATCH_END=$(date +%s)
    BATCH_DUR=$((BATCH_END - BATCH_START))
    BATCH_RATE=$(( (BATCH_MATCH + BATCH_MISMATCH) / (BATCH_DUR > 0 ? BATCH_DUR : 1) ))
    ELAPSED=$((BATCH_END - START_TIME))

    echo "$END_BLOCK" > "$LAST_VALIDATED_FILE"
    echo "$(date -Iseconds) Validated $NEXT_BLOCK-$END_BLOCK: ${BATCH_MATCH}/${BATCH_MATCH}+${BATCH_MISMATCH} match | total=${TOTAL_CHECKED} mismatches=${TOTAL_MISMATCH} | ${BATCH_RATE} blocks/s | elapsed=${ELAPSED}s" | tee -a "$LOGFILE"

    NEXT_BLOCK=$END_BLOCK
    sleep "$SLEEP"
done
