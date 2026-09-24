#!/bin/bash
# Neo-RS Migration Script: C# Neo Node to Rust Neo-RS
# Migrates blockchain data from C# implementation to neo-rs

set -euo pipefail

CSHARP_DATA_DIR="${CSHARP_DATA_DIR:-}"
NEORS_DATA_DIR="${NEORS_DATA_DIR:-/opt/neo-node/data/mainnet}"
BACKUP_DIR="${BACKUP_DIR:-/opt/neo-node/backups/migration-$(date +%Y%m%d_%H%M%S)}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;36m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_prerequisites() {
    log_info "Checking prerequisites..."

    if [[ -z "$CSHARP_DATA_DIR" ]]; then
        log_error "CSHARP_DATA_DIR not set"
        echo "Usage: CSHARP_DATA_DIR=/path/to/csharp/data $0"
        exit 1
    fi

    if [[ ! -d "$CSHARP_DATA_DIR" ]]; then
        log_error "C# data directory not found: $CSHARP_DATA_DIR"
        exit 1
    fi

    log_info "Prerequisites OK"
}

stop_csharp_node() {
    log_info "Checking C# node status..."

    if systemctl is-active --quiet neo-cli 2>/dev/null; then
        log_warn "C# neo-cli service is running"
        read -p "Stop C# node? (yes/no): " confirm
        if [[ "$confirm" == "yes" ]]; then
            systemctl stop neo-cli
            log_info "C# node stopped"
        else
            log_error "Cannot migrate while C# node is running"
            exit 1
        fi
    else
        log_info "C# node is not running"
    fi
}

backup_csharp_data() {
    log_info "Backing up C# data..."
    mkdir -p "$BACKUP_DIR"

    tar -czf "$BACKUP_DIR/csharp-data-backup.tar.gz" -C "$CSHARP_DATA_DIR" .
    log_info "Backup created: $BACKUP_DIR/csharp-data-backup.tar.gz"
}

analyze_csharp_data() {
    log_info "Analyzing C# blockchain data..."

    # Check for LevelDB data
    if [[ -d "$CSHARP_DATA_DIR/Data_LevelDB_*" ]]; then
        log_info "Found LevelDB data"
        export CSHARP_DB_TYPE="leveldb"
        export CSHARP_DB_PATH=$(find "$CSHARP_DATA_DIR" -type d -name "Data_LevelDB_*" | head -1)
    else
        log_error "No recognized database format found"
        exit 1
    fi

    log_info "Database type: $CSHARP_DB_TYPE"
    log_info "Database path: $CSHARP_DB_PATH"
}

migrate_data() {
    log_info "Migrating blockchain data..."
    log_warn "NOTE: Direct migration from C# LevelDB to Rust RocksDB is not yet implemented"
    log_info "Alternative migration strategies:"
    echo ""
    echo "Option 1: Fast sync from snapshot (RECOMMENDED)"
    echo "  - Download latest mainnet snapshot"
    echo "  - Extract to neo-rs data directory"
    echo "  - Much faster than full sync"
    echo ""
    echo "Option 2: Sync from network"
    echo "  - Start neo-rs node from genesis"
    echo "  - Will take 2-3 days to fully sync"
    echo ""
    echo "Option 3: Export/Import (future feature)"
    echo "  - Export C# chain to portable format"
    echo "  - Import into neo-rs"
    echo "  - Not yet implemented"
    echo ""

    read -p "Use snapshot download? (yes/no): " use_snapshot
    if [[ "$use_snapshot" == "yes" ]]; then
        download_snapshot
    else
        log_info "Will sync from network on first start"
    fi
}

download_snapshot() {
    log_info "Downloading mainnet snapshot..."

    local snapshot_url="https://sync.ngd.network/mainnet-snapshot-latest.tar.gz"
    local snapshot_file="/tmp/mainnet-snapshot.tar.gz"

    wget -O "$snapshot_file" "$snapshot_url" || {
        log_error "Failed to download snapshot"
        return 1
    }

    log_info "Extracting snapshot..."
    mkdir -p "$NEORS_DATA_DIR"
    tar -xzf "$snapshot_file" -C "$NEORS_DATA_DIR"
    rm "$snapshot_file"

    log_info "Snapshot extracted successfully"
}

migrate_wallet() {
    log_info "Checking for wallet files..."

    local wallet_files=$(find "$CSHARP_DATA_DIR" -name "*.json" -o -name "*.db3" 2>/dev/null || true)

    if [[ -n "$wallet_files" ]]; then
        log_warn "Found wallet files in C# data directory"
        log_warn "IMPORTANT: neo-rs uses different wallet format"
        echo ""
        echo "Wallet migration steps:"
        echo "1. Export private keys from C# wallet"
        echo "2. Import into neo-rs wallet using: neo-node wallet import"
        echo "3. Secure backup of both wallet files"
        echo ""
        log_warn "Do NOT delete C# wallet until verified in neo-rs"
    fi
}

verify_migration() {
    log_info "Verification steps..."
    echo ""
    echo "After starting neo-rs, verify:"
    echo "1. Block height matches expected mainnet height"
    echo "2. Peer connections established (>10 peers)"
    echo "3. RPC endpoint responding correctly"
    echo "4. If consensus node: verify validator status"
    echo ""
    echo "Verification commands:"
    echo "  ./scripts/healthcheck.sh"
    echo "  curl http://localhost:10332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}'"
}

create_migration_report() {
    log_info "Creating migration report..."

    cat > "$BACKUP_DIR/migration-report.txt" <<EOF
Neo-RS Migration Report
=======================
Date: $(date)
Source: C# Neo Node
Target: neo-rs v0.17.0

C# Data:
- Directory: $CSHARP_DATA_DIR
- Database: $CSHARP_DB_TYPE
- Backup: $BACKUP_DIR/csharp-data-backup.tar.gz

Migration Method:
- Snapshot download and extract

Next Steps:
1. Start neo-rs: systemctl start neo-node
2. Monitor sync: journalctl -u neo-node -f
3. Verify: ./scripts/healthcheck.sh
4. Keep C# backup for 30 days minimum

Rollback Procedure:
1. Stop neo-rs: systemctl stop neo-node
2. Restore C# data from backup
3. Start C# node: systemctl start neo-cli
EOF

    log_info "Migration report: $BACKUP_DIR/migration-report.txt"
}

main() {
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}Neo-RS Migration: C# to Rust${NC}"
    echo -e "${BLUE}================================================${NC}"
    echo ""

    check_prerequisites
    stop_csharp_node
    backup_csharp_data
    analyze_csharp_data
    migrate_data
    migrate_wallet
    verify_migration
    create_migration_report

    echo ""
    log_info "================================================"
    log_info "Migration preparation complete!"
    log_info "================================================"
    echo ""
    log_info "Backup location: $BACKUP_DIR"
    log_warn "Do NOT delete C# data until neo-rs is verified (minimum 30 days)"
    echo ""
    log_info "Start neo-rs with: systemctl start neo-node"
}

main "$@"
