#!/usr/bin/env bash
# Compare transaction execution notifications between Rust and C# for a block range.
#
# This script:
#   1. Cleans the Rust data directory (fresh import from block 0)
#   2. Imports blocks with NEO_TRACE_BLOCK to capture notification traces
#   3. Runs the Python comparison script against C# reference RPC
#
# Usage:
#   ./scripts/compare-block-notifications.sh 125000-125100
#   ./scripts/compare-block-notifications.sh 125000-125100 --verbose
#   ./scripts/compare-block-notifications.sh 125000             # single block
#
# Environment overrides:
#   NODE_BIN       Path to neo-node binary (default: auto-detect release/debug)
#   NODE_CONFIG    Node config file (default: neo_mainnet_node.toml)
#   ACC_FILE       Path to .acc block file (default: auto-detect)
#   DATA_DIR       MDBX data directory to clean (default: ./data/mainnet)
#   CSHARP_RPC     C# reference RPC (default: http://seed1.neo.org:10332)
#   TRACE_LOG      Output trace log path (default: /tmp/neo_trace_RANGE.log)
#   SKIP_IMPORT    Set to 1 to skip import and just run comparison on existing log
#   PYTHON         Python interpreter (default: python3)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# ── Parse arguments ───────────────────────────────────────────────────────────

if [[ $# -lt 1 ]] || [[ "$1" == "-h" ]] || [[ "$1" == "--help" ]]; then
    echo "Usage: $0 BLOCK_RANGE [--verbose] [--json]" >&2
    echo "" >&2
    echo "  BLOCK_RANGE  Single block (125000) or range (125000-125100)" >&2
    echo "  --verbose    Show all transactions, not just divergent ones" >&2
    echo "  --json       Output JSON instead of text" >&2
    echo "" >&2
    echo "Environment overrides:" >&2
    echo "  NODE_BIN       Path to neo-node binary" >&2
    echo "  NODE_CONFIG    Node config file (default: neo_mainnet_node.toml)" >&2
    echo "  ACC_FILE       Path to .acc block file" >&2
    echo "  DATA_DIR       MDBX data directory to clean" >&2
    echo "  CSHARP_RPC     C# reference RPC (default: http://seed1.neo.org:10332)" >&2
    echo "  TRACE_LOG      Output trace log path" >&2
    echo "  SKIP_IMPORT    Set to 1 to skip import and just compare" >&2
    exit 1
fi

RANGE="$1"
shift
EXTRA_ARGS=("$@")

# Parse range into start and end
if [[ "$RANGE" == *-* ]]; then
    RANGE_START="${RANGE%%-*}"
    RANGE_END="${RANGE##*-}"
else
    RANGE_START="$RANGE"
    RANGE_END="$RANGE"
fi

# Validate numeric
if ! [[ "$RANGE_START" =~ ^[0-9]+$ ]] || ! [[ "$RANGE_END" =~ ^[0-9]+$ ]]; then
    echo "ERROR: Block range must be numeric (got: $RANGE)" >&2
    exit 1
fi

if [[ "$RANGE_START" -gt "$RANGE_END" ]]; then
    echo "ERROR: Range start ($RANGE_START) must be <= range end ($RANGE_END)" >&2
    exit 1
fi

STOP_HEIGHT=$((RANGE_END + 1))

# ── Configuration ─────────────────────────────────────────────────────────────

PYTHON="${PYTHON:-python3}"
NODE_CONFIG="${NODE_CONFIG:-neo_mainnet_node.toml}"
CSHARP_RPC="${CSHARP_RPC:-http://seed1.neo.org:10332}"
TRACE_LOG="${TRACE_LOG:-/tmp/neo_trace_${RANGE_START}-${RANGE_END}.log}"
SKIP_IMPORT="${SKIP_IMPORT:-0}"

# Auto-detect node binary
if [[ -n "${NODE_BIN:-}" ]]; then
    : # use provided
elif [[ -x "$ROOT/target/release/neo-node" ]]; then
    NODE_BIN="$ROOT/target/release/neo-node"
elif [[ -x "$ROOT/target/debug/neo-node" ]]; then
    NODE_BIN="$ROOT/target/debug/neo-node"
else
    echo "ERROR: neo-node binary not found. Build with: cargo build --release -p neo-node" >&2
    exit 1
fi

# Auto-detect .acc file
if [[ -n "${ACC_FILE:-}" ]]; then
    : # use provided
else
    ACC_FILE="$(find "$ROOT/data" "$ROOT" -maxdepth 3 -name '*.acc' -type f 2>/dev/null | head -n 1 || true)"
    if [[ -z "$ACC_FILE" ]]; then
        echo "ERROR: No .acc file found. Set ACC_FILE=/path/to/chain.0.acc" >&2
        exit 1
    fi
fi

# Auto-detect data directory from config
if [[ -n "${DATA_DIR:-}" ]]; then
    : # use provided
else
    DATA_DIR="$(grep -E '^\s*path\s*=' "$ROOT/$NODE_CONFIG" 2>/dev/null \
        | head -1 \
        | sed 's/.*=\s*"//' \
        | sed 's/".*//' \
        | sed 's|^\./||')" || true
    if [[ -z "$DATA_DIR" ]]; then
        DATA_DIR="data/mainnet"
    fi
    # Make absolute if relative
    if [[ "$DATA_DIR" != /* ]]; then
        DATA_DIR="$ROOT/$DATA_DIR"
    fi
fi

# ── Print configuration ──────────────────────────────────────────────────────

echo "======================================================================"
echo "  Neo-RS Transaction Notification Comparison"
echo "======================================================================"
echo "  Block range:    $RANGE_START - $RANGE_END"
echo "  Stop height:    $STOP_HEIGHT"
echo "  Node binary:    $NODE_BIN"
echo "  Node config:    $NODE_CONFIG"
echo "  ACC file:       $ACC_FILE"
echo "  Data directory: $DATA_DIR"
echo "  C# RPC:         $CSHARP_RPC"
echo "  Trace log:      $TRACE_LOG"
echo "  Skip import:    $SKIP_IMPORT"
echo "======================================================================"
echo ""

# ── Step 1: Import with tracing ──────────────────────────────────────────────

if [[ "$SKIP_IMPORT" != "1" ]]; then
    echo "[$(date '+%F %T')] Step 1: Cleaning data directory..."
    if [[ -d "$DATA_DIR" ]]; then
        rm -rf "$DATA_DIR"
        echo "  Removed: $DATA_DIR"
    else
        echo "  Already clean: $DATA_DIR"
    fi
    mkdir -p "$(dirname "$DATA_DIR")"

    echo "[$(date '+%F %T')] Step 2: Importing blocks with tracing..."
    echo "  NEO_TRACE_BLOCK=${RANGE_START}-${RANGE_END}"
    echo "  NEO_IMPORT_STOP_HEIGHT=${STOP_HEIGHT}"
    echo "  Logging to: $TRACE_LOG"
    echo ""

    # Run import. The node logs to stderr via tracing-subscriber.
    # Capture stderr (where tracing output goes) to the trace log.
    set +e
    NEO_TRACE_BLOCK="${RANGE_START}-${RANGE_END}" \
    NEO_IMPORT_STOP_HEIGHT="${STOP_HEIGHT}" \
    RUST_LOG="${RUST_LOG:-neo=warn}" \
        "$NODE_BIN" --config "$NODE_CONFIG" --import-acc "$ACC_FILE" --import-only \
        2>&1 | tee "$TRACE_LOG"
    IMPORT_EXIT=$?
    set -e

    echo ""
    echo "[$(date '+%F %T')] Import finished (exit code: $IMPORT_EXIT)"

    # Count trace lines
    EXEC_LINES=$(grep -c "TRACE: tx execution result" "$TRACE_LOG" 2>/dev/null || echo "0")
    NOTIF_LINES=$(grep -c "TRACE: notification" "$TRACE_LOG" 2>/dev/null || echo "0")
    echo "  Trace lines: $EXEC_LINES execution results, $NOTIF_LINES notifications"

    if [[ "$EXEC_LINES" -eq 0 ]]; then
        echo ""
        echo "WARNING: No trace lines found in log. Possible causes:"
        echo "  - The block range has no transactions"
        echo "  - The ACC file does not contain blocks up to $RANGE_END"
        echo "  - The NEO_TRACE_BLOCK env var was not picked up"
        echo ""
    fi
else
    echo "[$(date '+%F %T')] Skipping import (SKIP_IMPORT=1)"
    echo "  Using existing trace log: $TRACE_LOG"
    if [[ ! -f "$TRACE_LOG" ]]; then
        echo "ERROR: Trace log not found: $TRACE_LOG" >&2
        exit 1
    fi
    EXEC_LINES=$(grep -c "TRACE: tx execution result" "$TRACE_LOG" 2>/dev/null || echo "0")
    NOTIF_LINES=$(grep -c "TRACE: notification" "$TRACE_LOG" 2>/dev/null || echo "0")
    echo "  Trace lines: $EXEC_LINES execution results, $NOTIF_LINES notifications"
fi

echo ""

# ── Step 3: Run comparison ────────────────────────────────────────────────────

echo "[$(date '+%F %T')] Step 3: Comparing with C# reference..."
echo ""

COMPARE_SCRIPT="$ROOT/scripts/find-first-tx-divergence.py"
if [[ ! -f "$COMPARE_SCRIPT" ]]; then
    echo "ERROR: Comparison script not found: $COMPARE_SCRIPT" >&2
    exit 1
fi

set +e
"$PYTHON" "$COMPARE_SCRIPT" \
    --block "${RANGE_START}-${RANGE_END}" \
    --rust-log "$TRACE_LOG" \
    --csharp-rpc "$CSHARP_RPC" \
    "${EXTRA_ARGS[@]}"
COMPARE_EXIT=$?
set -e

echo ""
if [[ "$COMPARE_EXIT" -eq 0 ]]; then
    echo "[$(date '+%F %T')] RESULT: No divergences found in blocks $RANGE_START-$RANGE_END"
else
    echo "[$(date '+%F %T')] RESULT: Divergences detected in blocks $RANGE_START-$RANGE_END (exit code: $COMPARE_EXIT)"
fi

exit "$COMPARE_EXIT"
