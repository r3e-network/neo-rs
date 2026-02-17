#!/usr/bin/env bash
set -euo pipefail

ROOT="/Users/jinghuiliao/git/neo-rs"
cd "$ROOT"

ZIP_PATH="data/bootstrap/chain.0.acc.zip"
UNZIP_DIR="data/bootstrap"
LOG_DIR="logs"
IMPORT_LOG="$LOG_DIR/acc-import.log"

mkdir -p "$UNZIP_DIR" "$LOG_DIR"

URL="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.path')"
EXPECTED_MD5="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.md5')"

echo "[$(date '+%F %T')] bootstrap-start url=$URL"

# 1) Ensure package is fully downloaded and checksum-valid.
./scripts/download_acc_resume.sh "$URL" "$ZIP_PATH" "$EXPECTED_MD5"

# 2) Stop any running node/watchdog before import.
pkill -f 'scripts/neo_node_watchdog.sh' || true
pkill -f 'target/release/neo-node --config neo_testnet_node.toml' || true

# 3) Extract .acc payload.
unzip -o "$ZIP_PATH" -d "$UNZIP_DIR" >/dev/null
ACC_PATH="$(find "$UNZIP_DIR" -maxdepth 1 -type f -name '*.acc' | head -n 1)"
if [[ -z "$ACC_PATH" ]]; then
  echo "[$(date '+%F %T')] bootstrap-error no .acc found in $UNZIP_DIR" >&2
  exit 1
fi
echo "[$(date '+%F %T')] acc-file=$ACC_PATH"

# 4) Import into rocksdb state.
./target/release/neo-node --config neo_testnet_node.toml --import-acc "$ACC_PATH" --import-only \
  >"$IMPORT_LOG" 2>&1

# 5) Restart watchdog for tail sync.
nohup bash ./scripts/neo_node_watchdog.sh >/dev/null 2>&1 &
echo "[$(date '+%F %T')] bootstrap-complete watchdog-pid=$!"
