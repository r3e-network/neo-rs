#!/usr/bin/env bash
# Thin wrapper around the Python state-root validator.
# Usage:
#   ./scripts/validate-stateroot-continuous.sh [batch_size] [poll_interval_seconds]
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VALIDATOR="$ROOT_DIR/scripts/continuous-stateroot-validation.py"

BATCH="${1:-500}"
POLL_INTERVAL="${2:-5}"

LOCAL_CONFIG="${LOCAL_CONFIG:-$ROOT_DIR/neo_mainnet_node.toml}"
LOCAL_RPC="${LOCAL_RPC:-}"
LOCAL_RPC_USER="${LOCAL_RPC_USER:-${NEO_RPC_USER:-}}"
LOCAL_RPC_PASS="${LOCAL_RPC_PASS:-${NEO_RPC_PASS:-}}"

REFERENCE_RPCS="${REFERENCE_RPCS:-}"
REFERENCE_RPC_USER="${REFERENCE_RPC_USER:-}"
REFERENCE_RPC_PASS="${REFERENCE_RPC_PASS:-}"

STATUS_FILE="${STATUS_FILE:-/tmp/stateroot-validation.json}"
RESUME_FILE="${RESUME_FILE:-/tmp/stateroot-last-validated}"

ARGS=(
    "--batch" "$BATCH"
    "--poll-interval" "$POLL_INTERVAL"
    "--status-file" "$STATUS_FILE"
    "--resume-file" "$RESUME_FILE"
)

if [[ -n "$LOCAL_RPC" ]]; then
    ARGS+=("--local" "$LOCAL_RPC")
elif [[ -f "$LOCAL_CONFIG" ]]; then
    ARGS+=("--local-config" "$LOCAL_CONFIG")
fi

if [[ -n "$LOCAL_RPC_USER" ]]; then
    ARGS+=("--local-user" "$LOCAL_RPC_USER")
fi

if [[ -n "$LOCAL_RPC_PASS" ]]; then
    ARGS+=("--local-pass" "$LOCAL_RPC_PASS")
fi

if [[ -n "$REFERENCE_RPCS" ]]; then
    ARGS+=("--reference" "$REFERENCE_RPCS")
fi

if [[ -n "$REFERENCE_RPC_USER" ]]; then
    ARGS+=("--reference-user" "$REFERENCE_RPC_USER")
fi

if [[ -n "$REFERENCE_RPC_PASS" ]]; then
    ARGS+=("--reference-pass" "$REFERENCE_RPC_PASS")
fi

exec python3 "$VALIDATOR" "${ARGS[@]}"
