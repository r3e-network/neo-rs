#!/usr/bin/env bash
set -euo pipefail

# Simple RocksDB backup helper.
# Usage: ./scripts/backup-rocksdb.sh <rocksdb_path> [backup_dir]
# - rocksdb_path: path to the RocksDB data directory (e.g., /data/testnet)
# - backup_dir: directory to write the tarball (default: ./backups)

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: $0 <rocksdb_path> [backup_dir]" >&2
  exit 1
fi

DB_PATH="$1"
BACKUP_DIR="${2:-backups}"

if [[ ! -d "$DB_PATH" ]]; then
  echo "RocksDB path not found: $DB_PATH" >&2
  exit 1
fi

mkdir -p "$BACKUP_DIR"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
BASENAME="$(basename "$DB_PATH")"
ARCHIVE="${BACKUP_DIR}/neo-rocksdb-${BASENAME}-${TIMESTAMP}.tar.gz"

echo "Creating backup: $ARCHIVE"
tar -czf "$ARCHIVE" -C "$(dirname "$DB_PATH")" "$BASENAME"
echo "Backup complete."
