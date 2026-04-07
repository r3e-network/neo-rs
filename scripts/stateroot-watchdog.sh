#!/bin/bash
# Watchdog script: monitors neo-node sync progress, restarts if stalled
# Each restart processes a burst of blocks before the P2P pipeline stalls

ROOT_DIR="${ROOT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
NODE_BIN="${NODE_BIN:-$ROOT_DIR/target/release/neo-node}"
NODE_CONFIG="${NODE_CONFIG:-$ROOT_DIR/neo_mainnet_node.toml}"
NODE_LOG="${NODE_LOG:-$ROOT_DIR/logs/neo-node-stateroot.log}"
LOCAL_RPC_URL="${LOCAL_RPC_URL:-http://127.0.0.1:10332}"
LOCAL_RPC_USER="${LOCAL_RPC_USER:-${NEO_RPC_USER:-}}"
LOCAL_RPC_PASS="${LOCAL_RPC_PASS:-${NEO_RPC_PASS:-}}"
NODE_EXTRA_ARGS="${NODE_EXTRA_ARGS:-}"
STALL_THRESHOLD=30  # seconds without progress before restart
CHECK_INTERVAL=10   # seconds between checks

get_state_height() {
    python3 - "$LOCAL_RPC_URL" "$LOCAL_RPC_USER" "$LOCAL_RPC_PASS" <<'PY'
import base64
import gzip
import http.client
import json
import sys
from urllib.parse import urlparse

try:
    parsed = urlparse(sys.argv[1])
    user = sys.argv[2]
    password = sys.argv[3]
    port = parsed.port or (443 if parsed.scheme == 'https' else 80)
    conn_cls = http.client.HTTPSConnection if parsed.scheme == 'https' else http.client.HTTPConnection
    headers = {'Content-Type': 'application/json', 'Accept-Encoding': 'gzip'}
    if user and password:
        token = base64.b64encode(f'{user}:{password}'.encode('utf-8')).decode('ascii')
        headers['Authorization'] = f'Basic {token}'
    c = conn_cls(parsed.hostname, port, timeout=5)
    c.request('POST', parsed.path or '/', json.dumps({'jsonrpc':'2.0','method':'getstateheight','params':[],'id':1}), headers)
    r = c.getresponse().read()
    if r[:2] == b'\x1f\x8b': r = gzip.decompress(r)
    c.close()
    print(json.loads(r)['result']['localrootindex'])
except:
    print('-1')
PY
}

restart_node() {
    pkill -9 -f "target/release/neo-node" 2>/dev/null
    sleep 2
    mkdir -p "$(dirname "$NODE_LOG")"
    # shellcheck disable=SC2086
    $NODE_BIN --config "$NODE_CONFIG" --state-root $NODE_EXTRA_ARGS >> "$NODE_LOG" 2>&1 &
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
