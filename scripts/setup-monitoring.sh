#!/bin/bash
# Neo-N3 Production Monitoring Setup Script
# Automated deployment of Prometheus, Alertmanager, and Grafana

set -e

echo "======================================"
echo "Neo-N3 Production Monitoring Setup"
echo "======================================"
echo ""

# Configuration
PROMETHEUS_VERSION="2.47.0"
ALERTMANAGER_VERSION="0.26.0"
GRAFANA_VERSION="10.0.0"
BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_DIR="${BASE_DIR}/config/prometheus"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."
    
    local missing_tools=()
    
    # Check for required tools
    if ! command -v wget &> /dev/null; then
        missing_tools+=("wget")
    fi
    
    if ! command -v tar &> /dev/null; then
        missing_tools+=("tar")
    fi
    
    if [ ${#missing_tools[@]} -gt 0 ]; then
        log_error "Missing required tools: ${missing_tools[*]}"
        log_error "Please install them and try again"
        return 1
    fi
    
    log_info "All prerequisites satisfied"
    return 0
}

# Create directories
create_directories() {
    log_info "Creating directories..."
    
    local dirs=(
        "/var/lib/prometheus"
        "/var/lib/alertmanager"
        "/etc/prometheus"
        "/etc/alertmanager"
        "/var/log/prometheus"
        "/var/log/alertmanager"
    )
    
    for dir in "${dirs[@]}"; do
        if [ ! -d "$dir" ]; then
            mkdir -p "$dir"
            log_info "Created directory: $dir"
        fi
    done
}

# Download Prometheus
download_prometheus() {
    log_info "Downloading Prometheus v${PROMETHEUS_VERSION}..."
    
    local url="https://github.com/prometheus/prometheus/releases/download/v${PROMETHEUS_VERSION}/prometheus-${PROMETHEUS_VERSION}.linux-amd64.tar.gz"
    local temp_file="/tmp/prometheus-${PROMETHEUS_VERSION}.tar.gz"
    
    if [ ! -f "$temp_file" ]; then
        wget -O "$temp_file" "$url" || {
            log_error "Failed to download Prometheus"
            return 1
        }
    else
        log_info "Prometheus archive already exists"
    fi
    
    # Extract
    tar xzf "$temp_file" -C /tmp
    log_info "Extracted Prometheus"
    
    # Copy binaries
    cp /tmp/prometheus-${PROMETHEUS_VERSION}.linux-amd64/prometheus /usr/local/bin/
    cp /tmp/prometheus-${PROMETHEUS_VERSION}.linux-amd64/promtool /usr/local/bin/
    
    log_info "Installed Prometheus"
}

# Configure Prometheus
configure_prometheus() {
    log_info "Configuring Prometheus..."
    
    # Copy config file
    cp "${CONFIG_DIR}/prometheus.yml" /etc/prometheus/prometheus.yml
    
    # Verify configuration
    if command -v promtool &> /dev/null; then
        log_info "Validating Prometheus configuration..."
        promtool check config /etc/prometheus/prometheus.yml || {
            log_error "Invalid Prometheus configuration"
            return 1
        }
    fi
    
    log_info "Prometheus configured successfully"
}

# Download Alertmanager
download_alertmanager() {
    log_info "Downloading Alertmanager v${ALERTMANAGER_VERSION}..."
    
    local url="https://github.com/prometheus/alertmanager/releases/download/v${ALERTMANAGER_VERSION}/alertmanager-${ALERTMANAGER_VERSION}.linux-amd64.tar.gz"
    local temp_file="/tmp/alertmanager-${ALERTMANAGER_VERSION}.tar.gz"
    
    if [ ! -f "$temp_file" ]; then
        wget -O "$temp_file" "$url" || {
            log_error "Failed to download Alertmanager"
            return 1
        }
    else
        log_info "Alertmanager archive already exists"
    fi
    
    # Extract
    tar xzf "$temp_file" -C /tmp
    log_info "Extracted Alertmanager"
    
    # Copy binaries
    cp /tmp/alertmanager-${ALERTMANAGER_VERSION}.linux-amd64/alertmanager /usr/local/bin/
    cp /tmp/alertmanager-${ALERTMANAGER_VERSION}.linux-amd64/amtool /usr/local/bin/
    
    log_info "Installed Alertmanager"
}

# Configure Alertmanager
configure_alertmanager() {
    log_info "Configuring Alertmanager..."
    
    # Create template directories
    mkdir -p /etc/alertmanager/html_templates
    mkdir -p /etc/alertmanager/alerts
    
    # Copy config file
    cp "${CONFIG_DIR}/alertmanager.yml" /etc/alertmanager/alertmanager.yml
    
    log_info "Alertmanager configured successfully"
}

# Create systemd services
create_systemd_services() {
    log_info "Creating systemd services..."
    
    # Prometheus service
    cat > /etc/systemd/system/prometheus.service <<EOF
[Unit]
Description=Prometheus Metrics Collector
After=network.target

[Service]
User=root
ExecStart=/usr/local/bin/prometheus \
    --config.file=/etc/prometheus/prometheus.yml \
    --storage.tsdb.path=/var/lib/prometheus \
    --web.enable-lifecycle \
    --storage.tsdb.retention.time=${PROMETHEUS_RETENTION_TIME:-30d}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    
    # Alertmanager service
    cat > /etc/systemd/system/alertmanager.service <<EOF
[Unit]
Description=Prometheus Alertmanager
After=network.target

[Service]
User=root
Environment="SMTP_PASSWORD=${SMTP_PASSWORD:-}"
Environment="SLACK_WEBHOOK_URL=${SLACK_WEBHOOK_URL:-}"
ExecStart=/usr/local/bin/alertmanager \
    --config.file=/etc/alertmanager/alertmanager.yml \
    --storage.path=/var/lib/alertmanager
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    
    # Reload systemd
    systemctl daemon-reload
    
    log_info "Systemd services created"
}

# Start services
start_services() {
    log_info "Starting monitoring services..."
    
    # Start Prometheus
    systemctl enable prometheus
    systemctl start prometheus
    sleep 2
    
    if systemctl is-active --quiet prometheus; then
        log_info "Prometheus started successfully"
    else
        log_error "Failed to start Prometheus"
        return 1
    fi
    
    # Start Alertmanager
    systemctl enable alertmanager
    systemctl start alertmanager
    sleep 2
    
    if systemctl is-active --quiet alertmanager; then
        log_info "Alertmanager started successfully"
    else
        log_error "Failed to start Alertmanager"
        return 1
    fi
}

# Install Grafana (optional)
install_grafana() {
    log_info "Installing Grafana..."
    
    case "$(uname -s)" in
        Linux)
            case "$(uname -m)" in
                x86_64)
                    # Add Grafana repository
                    wget -q -O - https://packages.grafana.com/gpg.key | sudo apt-key add -
                    echo "deb https://packages.grafana.com/oss/deb stable main" | sudo tee -a /etc/apt/sources.list.d/grafana.list
                    sudo apt-get update
                    sudo apt-get install -y grafana || {
                        log_warn "Grafana installation failed, but this is optional"
                        return 1
                    }
                    
                    # Enable and start Grafana
                    systemctl enable grafana-server
                    systemctl start grafana-server
                    
                    log_info "Grafana installed and started"
                    ;;
                *)
                    log_warn "Unsupported architecture for automatic Grafana installation"
                    return 1
                    ;;
            esac
            ;;
        *)
            log_warn "Automated Grafana installation only supported on Linux x86_64"
            return 1
            ;;
    esac
}

# Import Grafana dashboard
import_grafana_dashboard() {
    log_info "Importing Grafana dashboard..."
    
    if [ ! -f "${BASE_DIR}/config/grafana/neo-node-dashboard.json" ]; then
        log_warn "Grafana dashboard file not found, skipping import"
        return 0
    fi
    
    # Wait for Grafana to be ready
    for i in {1..30}; do
        if curl -s http://localhost:3000/api/health > /dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    
    # Import dashboard via API
    curl -X POST http://localhost:3000/api/dashboards/db \
        -H "Content-Type: application/json" \
        -d @${BASE_DIR}/config/grafana/neo-node-dashboard.json \
        -u admin:admin 2>/dev/null || {
        log_warn "Dashboard import failed manually via UI"
        return 0
    }
    
    log_info "Grafana dashboard imported"
}

# Test connectivity
test_connectivity() {
    log_info "Testing service connectivity..."
    
    # Test Prometheus
    if curl -s http://localhost:9090/api/v1/status > /dev/null 2>&1; then
        log_info "✓ Prometheus is accessible"
    else
        log_error "✗ Prometheus is not accessible"
        return 1
    fi
    
    # Test Alertmanager
    if curl -s http://localhost:9093/api/v1/status > /dev/null 2>&1; then
        log_info "✓ Alertmanager is accessible"
    else
        log_warn "✗ Alertmanager is not accessible"
    fi
    
    # Test Grafana (if installed)
    if curl -s http://localhost:3000/api/health > /dev/null 2>&1; then
        log_info "✓ Grafana is accessible"
    else
        log_info "○ Grafana not installed (optional)"
    fi
}

# Display next steps
display_next_steps() {
    log_info ""
    log_info "============================================"
    log_info "Setup Complete!"
    log_info "============================================"
    log_info ""
    log_info "Services running:"
    log_info "  • Prometheus:      http://localhost:9090"
    log_info "  • Alertmanager:    http://localhost:9093"
    log_info "  • Grafana (opt):   http://localhost:3000"
    log_info ""
    log_info "Neo-N3 Node endpoints:"
    log_info "  • Metrics:         http://127.0.0.1:9090/metrics"
    log_info "  • Health:          http://127.0.0.1:9090/healthz"
    log_info "  • Ready:           http://127.0.0.1:9090/ready"
    log_info ""
    log_info "Next steps:"
    log_info "1. Review and customize config/prometheus/alertmanager.yml"
    log_info "2. Set environment variables for notifications:"
    log_info "   export SMTP_PASSWORD='your-password'"
    log_info "   export SLACK_WEBHOOK_URL='your-webhook-url'"
    log_info "3. If using Grafana, login with admin/admin"
    log_info "4. Import dashboard from config/grafana/"
    log_info ""
    log_info "For documentation, see: docs/MONITORING.md"
    log_info ""
}

# Main execution
main() {
    log_info "Starting Neo-N3 Production Monitoring Setup"
    log_info ""
    
    # Parse arguments
    local skip_grafana=false
    while [[ $# -gt 0 ]]; do
        case $1 in
            --skip-grafana)
                skip_grafana=true
                shift
                ;;
            --help|-h)
                echo "Usage: $0 [--skip-grafana]"
                echo ""
                echo "Options:"
                echo "  --skip-grafana    Skip Grafana installation (optional)"
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done
    
    # Execute setup steps
    check_prerequisites || exit 1
    create_directories
    download_prometheus
    configure_prometheus
    download_alertmanager
    configure_alertmanager
    create_systemd_services
    start_services
    
    if [ "$skip_grafana" = false ]; then
        install_grafana || true
        import_grafana_dashboard || true
    fi
    
    test_connectivity
    display_next_steps
    
    log_info "Monitoring setup completed successfully!"
}

# Run main
main "$@"
