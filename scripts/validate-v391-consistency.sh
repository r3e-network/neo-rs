#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "scripts/validate-v391-consistency.sh is deprecated; use scripts/validate-v310-consistency.sh" >&2
exec "$SCRIPT_DIR/validate-v310-consistency.sh" "$@"
