#!/bin/bash
# Watchdog script: monitors neo-node sync progress, restarts if stalled
# Each restart processes a burst of blocks before the P2P pipeline stalls

NODE_BIN="/home/neo/git/neo-rs/target/release/neo-node"
NODE_CONFIG="/home/neo/git/neo-rs/config/mainnet-stateroot.toml"
NODE_LOG="/home/neo/git/neo-rs/logs/neo-node-stateroot.log"
STALL_THRESHOLD=30  # seconds without progress before restart
CHECK_INTERVAL=10   # seconds between checks

get_state_height() {
    python3 -c "
import http.client, json, gzip
try:
    c = http.client.HTTPConnection('127.0.0.1', 20332, timeout=5)
    c.request('POST', '/', json.dumps({'jsonrpc':'2.0','method':'getstateheight','params':[],'id':1}), {'Content-Type':'application/json'})
    r = c.getresponse().read()
    if r[:2] == b'\x1f\x8b': r = gzip.decompress(r)
    c.close()
    print(json.loads(r)['result']['localrootindex'])
except:
    print('-1')
" 2>/dev/null
}

restart_node() {
    pkill -9 -f "target/release/neo-node" 2>/dev/null
    sleep 2
    $NODE_BIN --config $NODE_CONFIG --state-root >> $NODE_LOG 2>&1 &
    echo "$(date '+%H:%M:%S') Restarted neo-node PID=$!"
    sleep 8  # wait for startup
}

echo "=== State Root Sync Watchdog ==="
echo "Stall threshold: ${STALL_THRESHOLD}s"

LAST_HEIGHT=-1
STALL_COUNT=0

while true; do
    HEIGHT=$(get_state_height)

    if [ "$HEIGHT" = "-1" ]; then
        echo "$(date '+%H:%M:%S') Node unreachable - restarting"
        restart_node
        LAST_HEIGHT=-1
        STALL_COUNT=0
        continue
    fi

    if [ "$HEIGHT" = "$LAST_HEIGHT" ]; then
        STALL_COUNT=$((STALL_COUNT + CHECK_INTERVAL))
        if [ $STALL_COUNT -ge $STALL_THRESHOLD ]; then
            echo "$(date '+%H:%M:%S') STALLED at $HEIGHT for ${STALL_COUNT}s - restarting"
            restart_node
            STALL_COUNT=0
        fi
    else
        if [ "$LAST_HEIGHT" != "-1" ] && [ "$HEIGHT" != "$LAST_HEIGHT" ]; then
            DELTA=$((HEIGHT - LAST_HEIGHT))
            RATE=$(echo "scale=1; $DELTA / $CHECK_INTERVAL" | bc 2>/dev/null || echo "?")
            # Only log periodically (every 500 blocks)
            if [ $((HEIGHT % 5000)) -lt $((LAST_HEIGHT % 5000)) ] || [ $((HEIGHT - LAST_HEIGHT)) -gt 1000 ]; then
                echo "$(date '+%H:%M:%S') height=$HEIGHT (+$DELTA, ${RATE}/s)"
            fi
        fi
        STALL_COUNT=0
    fi

    LAST_HEIGHT=$HEIGHT
    sleep $CHECK_INTERVAL
done
