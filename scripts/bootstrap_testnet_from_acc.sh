#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ZIP_PATH="${ZIP_PATH:-data/bootstrap/chain.0.acc.zip}"
UNZIP_DIR="${UNZIP_DIR:-data/bootstrap}"
LOG_DIR="${LOG_DIR:-logs}"
IMPORT_LOG="$LOG_DIR/acc-import.log"
NODE_BIN="${NODE_BIN:-$ROOT/target/release/neo-node}"
NODE_CONFIG="${NODE_CONFIG:-neo_testnet_node.toml}"

mkdir -p "$UNZIP_DIR" "$LOG_DIR"

if [[ ! -x "$NODE_BIN" ]]; then
  if [[ -x "$ROOT/target/debug/neo-node" ]]; then
    NODE_BIN="$ROOT/target/debug/neo-node"
  else
    echo "[$(date '+%F %T')] bootstrap-error neo-node binary not found (tried release and debug)" >&2
    exit 1
  fi
fi

URL="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.path')"
EXPECTED_MD5="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.md5')"

echo "[$(date '+%F %T')] bootstrap-start url=$URL"

# 1) Ensure package is fully downloaded and checksum-valid.
./scripts/download_acc_resume.sh "$URL" "$ZIP_PATH" "$EXPECTED_MD5"

# 2) Stop any running node/watchdog before import.
pkill -f 'scripts/neo_node_watchdog.sh' || true
pkill -f 'neo-node --config neo_testnet_node.toml' || true

# 3) Extract .acc payload.
if command -v unzip >/dev/null 2>&1; then
  unzip -o "$ZIP_PATH" -d "$UNZIP_DIR" >/dev/null
elif command -v python3 >/dev/null 2>&1; then
  python3 - "$ZIP_PATH" "$UNZIP_DIR" <<'PY'
import pathlib
import sys
import zipfile

zip_path = pathlib.Path(sys.argv[1])
out_dir = pathlib.Path(sys.argv[2])
out_dir.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(zip_path) as zf:
    zf.extractall(out_dir)
PY
else
  echo "[$(date '+%F %T')] bootstrap-error neither unzip nor python3 is available for extracting $ZIP_PATH" >&2
  exit 1
fi

ACC_PATH="$(find "$UNZIP_DIR" -maxdepth 1 -type f -name '*.acc' | head -n 1)"
if [[ -z "$ACC_PATH" ]]; then
  echo "[$(date '+%F %T')] bootstrap-error no .acc found in $UNZIP_DIR" >&2
  exit 1
fi
echo "[$(date '+%F %T')] acc-file=$ACC_PATH"

# 4) Import into rocksdb state.
"$NODE_BIN" --config "$NODE_CONFIG" --import-acc "$ACC_PATH" --import-only \
  >"$IMPORT_LOG" 2>&1

# 5) Restart watchdog for tail sync.
nohup bash ./scripts/neo_node_watchdog.sh >/dev/null 2>&1 &
echo "[$(date '+%F %T')] bootstrap-complete watchdog-pid=$!"
