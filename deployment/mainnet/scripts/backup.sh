#!/bin/bash
# Neo-RS Backup Script
# Creates backups of blockchain data and configuration

set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/opt/neo-node/backups}"
DATA_DIR="${DATA_DIR:-/opt/neo-node/data}"
CONFIG_DIR="${CONFIG_DIR:-/opt/neo-node/config}"
RETENTION_DAYS="${RETENTION_DAYS:-7}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_NAME="neo-node-backup-${TIMESTAMP}"

log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1"
}

create_backup_dir() {
    mkdir -p "$BACKUP_DIR/$BACKUP_NAME"
    log "Created backup directory: $BACKUP_DIR/$BACKUP_NAME"
}

backup_config() {
    log "Backing up configuration..."
    cp -r "$CONFIG_DIR" "$BACKUP_DIR/$BACKUP_NAME/"
    log "Configuration backup complete"
}

backup_data() {
    log "Backing up blockchain data..."
    log "WARNING: This may take a long time depending on data size"

    # Create tar archive with compression
    tar -czf "$BACKUP_DIR/$BACKUP_NAME/data.tar.gz" \
        -C "$DATA_DIR" . \
        --exclude='*.log' \
        --exclude='*.tmp'

    log "Data backup complete"
}

calculate_checksum() {
    log "Calculating checksums..."
    cd "$BACKUP_DIR/$BACKUP_NAME"
    sha256sum data.tar.gz > checksums.sha256
    log "Checksums saved"
}

create_manifest() {
    log "Creating backup manifest..."
    cat > "$BACKUP_DIR/$BACKUP_NAME/manifest.txt" <<EOF
Neo-RS Backup Manifest
======================
Backup ID: $BACKUP_NAME
Timestamp: $(date)
Hostname: $(hostname)
Data Size: $(du -sh "$DATA_DIR" | cut -f1)
Backup Size: $(du -sh "$BACKUP_DIR/$BACKUP_NAME" | cut -f1)

Contents:
- Blockchain data (data.tar.gz)
- Configuration files (config/)
- Checksums (checksums.sha256)

Restore Instructions:
1. Stop neo-node service
2. Extract data.tar.gz to data directory
3. Restore config files
4. Start neo-node service
EOF
    log "Manifest created"
}

cleanup_old_backups() {
    log "Cleaning up old backups (retention: $RETENTION_DAYS days)..."
    find "$BACKUP_DIR" -maxdepth 1 -type d -name "neo-node-backup-*" -mtime +$RETENTION_DAYS -exec rm -rf {} \;
    log "Old backups cleaned"
}

verify_backup() {
    log "Verifying backup integrity..."
    cd "$BACKUP_DIR/$BACKUP_NAME"
    sha256sum -c checksums.sha256
    log "Backup verification successful"
}

main() {
    log "Starting Neo-RS backup..."

    # Check if service is running
    if ! systemctl is-active --quiet neo-node; then
        log "WARNING: neo-node service is not running"
    fi

    create_backup_dir
    backup_config
    backup_data
    calculate_checksum
    create_manifest
    verify_backup
    cleanup_old_backups

    log "Backup complete: $BACKUP_DIR/$BACKUP_NAME"
    log "Backup size: $(du -sh "$BACKUP_DIR/$BACKUP_NAME" | cut -f1)"
}

main "$@"
