#!/bin/bash
# Neo-N3 Testnet Sync - Main Launcher Script
# Provides unified interface for all sync operations

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/../../"

cd "$PROJECT_ROOT"

echo "╔════════════════════════════════════════════════════════╗"
echo "║     Neo-N3 Testnet Full Sync Toolkit                  ║"
echo "║     Real-time Monitoring & State Validation           ║"
echo "╚════════════════════════════════════════════════════════╝"
echo ""

show_help() {
    cat << 'EOF'
Usage: ./sync-launcher.sh [COMMAND] [OPTIONS]

Commands:
  sync [BLOCK]      Start full synchronization (default)
                    Optional BLOCK parameter to resume from specific height
  
  monitor           Start monitoring dashboard only
  test              Run simulation without actual node
  verify            Check toolkit installation
  
  help              Show this help message

Examples:
  ./sync-launcher.sh sync               # Full sync from genesis
  ./sync-launcher.sh sync 5000000       # Resume from block 5M
  ./sync-launcher.sh test               # Simulation mode
  ./sync-launcher.sh verify             # Verify setup

Additional Info:
  Dashboard: http://localhost:8080
  Logs: logs/neo-sync-*.log
  Reports: reports/final-sync-report-*.md

Documentation:
  See README.md or QUICK_START.md for detailed usage
EOF
}

case "$1" in
    sync|s)
        echo "Starting full synchronization..."
        RESUME_BLOCK="${2:-0}"
        if [ "$RESUME_BLOCK" != "0" ]; then
            echo "Resuming from block #$RESUME_BLOCK"
        fi
        echo ""
        
        # Verify neo-node exists
        if [ ! -f "target/release/neo-node" ]; then
            echo "[ERROR] Binary not found. Run: cargo build --release"
            exit 1
        fi
        
        # Execute main sync script
        ./tools/sync-monitor/sync.sh sync "$RESUME_BLOCK"
        ;;
    
    monitor|m)
        echo "Starting monitoring service..."
        python3 tools/sync-monitor/sync_monitor.py --config config/testnet-production.toml
        ;;
    
    test|t)
        echo "Running simulation mode..."
        echo "(No real nodes will be started)"
        echo ""
        python3 tools/sync-monitor/sync_monitor.py --dry-run
        ;;
    
    verify|v)
        echo "Verifying toolkit installation..."
        python3 tools/sync-monitor/verify_installation.py
        ;;
    
    help|--help|-h)
        show_help
        ;;
    
    *)
        echo "Unknown command: $1"
        echo ""
        show_help
        exit 1
        ;;
esac
