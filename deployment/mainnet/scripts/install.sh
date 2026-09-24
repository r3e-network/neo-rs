#!/bin/bash
# Neo-RS Mainnet Node Installation Script
# Automates production deployment

set -euo pipefail

# Configuration
INSTALL_DIR="/opt/neo-node"
SERVICE_USER="neo"
SERVICE_GROUP="neo"
BINARY_NAME="neo-node"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root"
        exit 1
    fi
}

check_system_requirements() {
    log_info "Checking system requirements..."

    # Check CPU cores
    CPU_CORES=$(nproc)
    if [[ $CPU_CORES -lt 4 ]]; then
        log_warn "CPU cores: $CPU_CORES (minimum 4 recommended)"
    else
        log_info "CPU cores: $CPU_CORES ✓"
    fi

    # Check RAM
    TOTAL_RAM=$(free -g | awk '/^Mem:/{print $2}')
    if [[ $TOTAL_RAM -lt 16 ]]; then
        log_warn "RAM: ${TOTAL_RAM}GB (minimum 16GB recommended)"
    else
        log_info "RAM: ${TOTAL_RAM}GB ✓"
    fi

    # Check disk space
    AVAILABLE_SPACE=$(df -BG / | awk 'NR==2 {print $4}' | sed 's/G//')
    if [[ $AVAILABLE_SPACE -lt 500 ]]; then
        log_warn "Disk space: ${AVAILABLE_SPACE}GB (minimum 500GB recommended)"
    else
        log_info "Disk space: ${AVAILABLE_SPACE}GB ✓"
    fi
}

create_service_user() {
    log_info "Creating service user..."

    if id "$SERVICE_USER" &>/dev/null; then
        log_info "User $SERVICE_USER already exists"
    else
        useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
        log_info "Created user: $SERVICE_USER ✓"
    fi
}

create_directories() {
    log_info "Creating directory structure..."

    mkdir -p "$INSTALL_DIR"/{bin,config,data,logs,backups}

    # Set ownership
    chown -R "$SERVICE_USER:$SERVICE_GROUP" "$INSTALL_DIR"

    # Set permissions
    chmod 755 "$INSTALL_DIR"
    chmod 700 "$INSTALL_DIR"/data
    chmod 755 "$INSTALL_DIR"/logs
    chmod 700 "$INSTALL_DIR"/backups
    chmod 755 "$INSTALL_DIR"/config

    log_info "Directory structure created ✓"
}

install_binary() {
    log_info "Installing binary..."

    if [[ ! -f "./bin/$BINARY_NAME" ]]; then
        log_error "Binary not found: ./bin/$BINARY_NAME"
        log_error "Please build the binary first with: cargo build --profile production --features full --package neo-node"
        exit 1
    fi

    cp "./bin/$BINARY_NAME" "$INSTALL_DIR/bin/"
    chmod 755 "$INSTALL_DIR/bin/$BINARY_NAME"
    chown "$SERVICE_USER:$SERVICE_GROUP" "$INSTALL_DIR/bin/$BINARY_NAME"

    log_info "Binary installed ✓"
}

install_config() {
    log_info "Installing configuration..."

    cp ./config/mainnet-production.toml "$INSTALL_DIR/config/"
    chmod 600 "$INSTALL_DIR/config/mainnet-production.toml"
    chown "$SERVICE_USER:$SERVICE_GROUP" "$INSTALL_DIR/config/mainnet-production.toml"

    log_warn "IMPORTANT: Edit $INSTALL_DIR/config/mainnet-production.toml and set secure RPC credentials"

    log_info "Configuration installed ✓"
}

install_systemd_service() {
    log_info "Installing systemd service..."

    cat > /etc/systemd/system/neo-node.service <<EOF
[Unit]
Description=Neo N3 Mainnet Node
Documentation=https://github.com/r3e-network/neo-rs
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_GROUP
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/bin/$BINARY_NAME --config $INSTALL_DIR/config/mainnet-production.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=neo-node

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$INSTALL_DIR/data $INSTALL_DIR/logs $INSTALL_DIR/backups
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictRealtime=true
RestrictNamespaces=true
LimitNOFILE=65536
LimitNPROC=4096

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable neo-node

    log_info "Systemd service installed ✓"
}

configure_firewall() {
    log_info "Configuring firewall..."

    if command -v ufw &> /dev/null; then
        # UFW firewall
        ufw allow 10333/tcp comment 'Neo P2P'
        ufw status
        log_info "UFW firewall configured ✓"
    elif command -v firewall-cmd &> /dev/null; then
        # Firewalld
        firewall-cmd --permanent --add-port=10333/tcp
        firewall-cmd --reload
        log_info "Firewalld configured ✓"
    else
        log_warn "No firewall detected. Please manually configure:"
        log_warn "  - Allow inbound/outbound TCP 10333 (P2P)"
        log_warn "  - Block external access to TCP 10332 (RPC)"
        log_warn "  - Block external access to TCP 9090 (Metrics)"
    fi
}

print_next_steps() {
    echo ""
    log_info "================================================"
    log_info "Installation complete!"
    log_info "================================================"
    echo ""
    echo "Next steps:"
    echo ""
    echo "1. Configure RPC credentials:"
    echo "   sudo nano $INSTALL_DIR/config/mainnet-production.toml"
    echo ""
    echo "2. (Optional) Download blockchain snapshot for faster sync:"
    echo "   sudo -u $SERVICE_USER wget https://sync.ngd.network/mainnet-snapshot-latest.tar.gz"
    echo "   sudo -u $SERVICE_USER tar -xzf mainnet-snapshot-latest.tar.gz -C $INSTALL_DIR/data/"
    echo ""
    echo "3. Start the node:"
    echo "   sudo systemctl start neo-node"
    echo ""
    echo "4. Check status:"
    echo "   sudo systemctl status neo-node"
    echo "   sudo journalctl -u neo-node -f"
    echo ""
    echo "5. Verify synchronization:"
    echo "   curl http://localhost:10332 -u username:password -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}'"
    echo ""
    log_info "================================================"
}

main() {
    log_info "Neo-RS Mainnet Node Installation"
    echo ""

    check_root
    check_system_requirements
    create_service_user
    create_directories
    install_binary
    install_config
    install_systemd_service
    configure_firewall
    print_next_steps
}

main "$@"
