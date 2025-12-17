#!/usr/bin/env bash
set -euo pipefail

python3 "$(dirname "$0")/scan_nonprod_markers.py" "$@"

