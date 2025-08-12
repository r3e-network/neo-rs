#!/bin/bash

# Neo Rust Node Deployment Script
# This script automates the deployment of Neo Rust node with monitoring

set -e  # Exit on any error

# Configuration
VERSION="${VERSION:-v0.3.0-monitoring-fixes}"
ENVIRONMENT="${ENVIRONMENT:-testnet}"
DATA_DIR="${DATA_DIR:-/opt/neo-rust/data}"
CONFIG_DIR="${CONFIG_DIR:-/opt/neo-rust/config}"
LOG_DIR="${LOG_DIR:-/opt/neo-rust/logs}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Check if running as root or with sudo
check_privileges() {
    if [[ $EUID -eq 0 ]]; then
        log_info "Running as root"
    elif sudo -n true 2>/dev/null; then
        log_info "Running with sudo privileges"
    else
        log_error "This script requires root privileges or sudo access"
    fi
}

# System requirements check
check_system_requirements() {
    log_info "Checking system requirements..."
    
    # Check OS
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        log_success "Operating system: Linux"
    else
        log_error "This script is designed for Linux systems only"
    fi
    
    # Check memory
    MEMORY_GB=$(free -g | awk '/^Mem:/{print $2}')
    if [[ $MEMORY_GB -lt 4 ]]; then
        log_warning "Less than 4GB RAM detected. Neo node may experience performance issues"
    else
        log_success "Memory: ${MEMORY_GB}GB (sufficient)"
    fi
    
    # Check disk space
    DISK_FREE_GB=$(df / | tail -1 | awk '{print int($4/1024/1024)}')
    if [[ $DISK_FREE_GB -lt 50 ]]; then
        log_error "Insufficient disk space. At least 50GB required"
    else
        log_success "Disk space: ${DISK_FREE_GB}GB (sufficient)"
    fi
    
    # Check Docker
    if command -v docker &> /dev/null; then
        log_success "Docker is installed"
    else
        log_error "Docker is not installed. Please install Docker first"
    fi
    
    # Check Docker Compose
    if command -v docker-compose &> /dev/null || docker compose version &> /dev/null; then
        log_success "Docker Compose is available"
    else
        log_error "Docker Compose is not installed. Please install Docker Compose first"
    fi
}

# Create necessary directories
create_directories() {
    log_info "Creating directories..."
    
    sudo mkdir -p "$DATA_DIR" "$CONFIG_DIR" "$LOG_DIR"
    sudo mkdir -p "$DATA_DIR/blockchain" "$DATA_DIR/peers" "$DATA_DIR/temp"
    sudo mkdir -p "$LOG_DIR/neo-node" "$LOG_DIR/monitoring"
    
    # Set proper permissions
    sudo chown -R $(whoami):$(whoami) "$DATA_DIR" "$CONFIG_DIR" "$LOG_DIR"
    
    log_success "Directories created successfully"
}

# Generate configuration files
generate_config() {
    log_info "Generating configuration files..."
    
    # Neo node configuration
    cat > "$CONFIG_DIR/neo-node.json" << EOF
{
  "network": "$ENVIRONMENT",
  "bind_address": "0.0.0.0",
  "rpc_port": 20332,
  "p2p_port": 20333,
  "data_path": "$DATA_DIR/blockchain",
  "peers_file": "$DATA_DIR/peers/peers.json",
  "max_peers": 100,
  "enable_rpc": true,
  "enable_monitoring": true,
  "monitoring_port": 8080,
  "log_level": "info",
  "log_file": "$LOG_DIR/neo-node/node.log"
}
EOF
    
    # Monitoring configuration
    cat > "$CONFIG_DIR/monitoring.json" << EOF
{
  "health_checks": {
    "enabled": true,
    "interval_seconds": 30,
    "cache_duration_seconds": 5
  },
  "performance_monitoring": {
    "enabled": true,
    "max_samples": 1000,
    "collection_interval_seconds": 10
  },
  "metrics_export": {
    "prometheus": {
      "enabled": true,
      "endpoint": "/metrics",
      "port": 8080
    },
    "json": {
      "enabled": true,
      "endpoint": "/health",
      "detailed_endpoint": "/health/detailed"
    }
  },
  "thresholds": {
    "memory_usage_percent": {
      "warning": 75.0,
      "critical": 90.0
    },
    "cpu_usage_percent": {
      "warning": 70.0,
      "critical": 85.0
    },
    "disk_usage_percent": {
      "warning": 80.0,
      "critical": 95.0
    },
    "peer_count": {
      "warning": 10,
      "critical": 5
    }
  }
}
EOF
    
    # Docker Compose configuration
    cat > docker-compose.yml << EOF
version: '3.8'

services:
  neo-node:
    image: neo-rust:${VERSION}
    build:
      context: .
      dockerfile: Dockerfile
    container_name: neo-rust-node
    ports:
      - "20332:20332"  # RPC port
      - "20333:20333"  # P2P port
      - "8080:8080"    # Monitoring port
    volumes:
      - "$DATA_DIR:/opt/neo-rust/data"
      - "$CONFIG_DIR:/opt/neo-rust/config"
      - "$LOG_DIR:/opt/neo-rust/logs"
    environment:
      - NEO_NETWORK=$ENVIRONMENT
      - RUST_LOG=info
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 60s
    networks:
      - neo-network

  prometheus:
    image: prom/prometheus:latest
    container_name: neo-prometheus
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.console.libraries=/etc/prometheus/console_libraries'
      - '--web.console.templates=/etc/prometheus/consoles'
      - '--storage.tsdb.retention.time=200h'
      - '--web.enable-lifecycle'
    restart: unless-stopped
    networks:
      - neo-network

  grafana:
    image: grafana/grafana:latest
    container_name: neo-grafana
    ports:
      - "3000:3000"
    volumes:
      - grafana_data:/var/lib/grafana
      - ./monitoring/grafana-dashboard.json:/etc/grafana/provisioning/dashboards/neo-dashboard.json
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    restart: unless-stopped
    networks:
      - neo-network

networks:
  neo-network:
    driver: bridge

volumes:
  prometheus_data:
  grafana_data:
EOF
    
    log_success "Configuration files generated successfully"
}

# Build and start services
deploy_services() {
    log_info "Building and starting Neo Rust node services..."
    
    # Build the Docker image
    log_info "Building Docker image..."
    docker build -t neo-rust:$VERSION .
    
    # Start services
    log_info "Starting services..."
    docker-compose up -d
    
    # Wait for services to be ready
    log_info "Waiting for services to be ready..."
    sleep 30
    
    # Check service status
    if curl -f http://localhost:8080/health &> /dev/null; then
        log_success "Neo node is healthy and responding"
    else
        log_warning "Neo node health check failed, but service may still be starting"
    fi
    
    if curl -f http://localhost:9090 &> /dev/null; then
        log_success "Prometheus is running"
    else
        log_warning "Prometheus health check failed"
    fi
    
    if curl -f http://localhost:3000 &> /dev/null; then
        log_success "Grafana is running"
    else
        log_warning "Grafana health check failed"
    fi
    
    log_success "Deployment completed successfully!"
}

# Display deployment information
display_info() {
    log_info "Deployment Information:"
    echo "======================================="
    echo "Neo Rust Node: http://localhost:20332 (RPC)"
    echo "Neo P2P Port: 20333"
    echo "Monitoring: http://localhost:8080"
    echo "  - Health: http://localhost:8080/health"
    echo "  - Metrics: http://localhost:8080/metrics"
    echo "Prometheus: http://localhost:9090"
    echo "Grafana: http://localhost:3000 (admin/admin)"
    echo "======================================="
    echo
    echo "Data Directory: $DATA_DIR"
    echo "Config Directory: $CONFIG_DIR"
    echo "Log Directory: $LOG_DIR"
    echo
    echo "To check service status:"
    echo "  docker-compose ps"
    echo
    echo "To view logs:"
    echo "  docker-compose logs neo-node"
    echo "  docker-compose logs prometheus"
    echo "  docker-compose logs grafana"
    echo
    echo "To stop services:"
    echo "  docker-compose down"
    echo
    echo "To restart services:"
    echo "  docker-compose restart"
}

# Cleanup function
cleanup() {
    log_info "Cleaning up..."
    docker-compose down
    log_success "Services stopped"
}

# Main deployment function
main() {
    echo "======================================="
    echo "Neo Rust Node Deployment Script"
    echo "Version: $VERSION"
    echo "Environment: $ENVIRONMENT"
    echo "======================================="
    echo
    
    # Handle cleanup if requested
    if [[ "${1:-}" == "cleanup" ]]; then
        cleanup
        exit 0
    fi
    
    # Handle status check if requested
    if [[ "${1:-}" == "status" ]]; then
        docker-compose ps
        curl -s http://localhost:8080/health | jq . || echo "Health endpoint not available"
        exit 0
    fi
    
    check_privileges
    check_system_requirements
    create_directories
    generate_config
    deploy_services
    display_info
    
    log_success "Neo Rust node deployment completed successfully!"
    log_info "Visit http://localhost:3000 to access Grafana monitoring dashboard"
}

# Trap errors and cleanup
trap cleanup ERR

# Run main function
main "$@"