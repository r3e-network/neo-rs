#!/usr/bin/env bash
set -euo pipefail

# Compatibility wrapper for older runbooks. The canonical protocol gate is the
# Neo v3.10.0 validator, followed by strict MainNet parity checks.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Neo-RS protocol consistency validation ==="
bash "$SCRIPT_DIR/validate-v310-consistency.sh" --network mainnet "$@"
bash "$SCRIPT_DIR/mainnet-parity-check.sh"
