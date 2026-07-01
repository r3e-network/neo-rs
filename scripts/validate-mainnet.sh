#!/usr/bin/env bash
set -euo pipefail

# Legacy entrypoint retained for operator muscle memory. The old version only
# checked process health and a single block hash; use the strict v3.10
# consistency and MainNet parity gates instead.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Neo-RS MainNet strict validation ==="
bash "$SCRIPT_DIR/validate-v310-consistency.sh" --network mainnet "$@"
bash "$SCRIPT_DIR/mainnet-parity-check.sh"
