#!/bin/bash

# Neo Rust Monitoring Setup Script
# Creates monitoring configuration files for Prometheus and Grafana

set -e

# Create monitoring directory
mkdir -p monitoring

# Create Prometheus configuration
cat > monitoring/prometheus.yml << 'EOF'
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  # - "first_rules.yml"
  # - "second_rules.yml"

scrape_configs:
  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'neo-rust-node'
    static_configs:
      - targets: ['neo-node:8080']
    metrics_path: '/metrics'
    scrape_interval: 10s
    
  - job_name: 'neo-rust-health'
    static_configs:
      - targets: ['neo-node:8080']
    metrics_path: '/health'
    scrape_interval: 30s
EOF

# Create Grafana dashboard configuration
cat > monitoring/grafana-dashboard.json << 'EOF'
{
  "dashboard": {
    "id": null,
    "title": "Neo Rust Node Monitoring",
    "tags": ["neo", "blockchain"],
    "style": "dark",
    "timezone": "browser",
    "panels": [
      {
        "id": 1,
        "title": "Node Health Status",
        "type": "stat",
        "targets": [
          {
            "expr": "neo_node_health_status",
            "legendFormat": "Health Status"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "color": {
              "mode": "thresholds"
            },
            "thresholds": {
              "steps": [
                {"color": "red", "value": 0},
                {"color": "yellow", "value": 1},
                {"color": "green", "value": 2}
              ]
            }
          }
        },
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 0}
      },
      {
        "id": 2,
        "title": "Block Height",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_block_height",
            "legendFormat": "Block Height"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 0}
      },
      {
        "id": 3,
        "title": "Connected Peers",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_peer_count",
            "legendFormat": "Connected Peers"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 8}
      },
      {
        "id": 4,
        "title": "Memory Usage",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_memory_usage_bytes",
            "legendFormat": "Memory Usage (Bytes)"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 8}
      },
      {
        "id": 5,
        "title": "Transaction Pool Size",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_transaction_pool_size",
            "legendFormat": "Transaction Pool"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 16}
      },
      {
        "id": 6,
        "title": "RPC Requests per Second",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(neo_rpc_requests_total[5m])",
            "legendFormat": "RPC Requests/sec"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 16}
      }
    ],
    "time": {
      "from": "now-1h",
      "to": "now"
    },
    "refresh": "5s"
  }
}
EOF

echo "✅ Monitoring configuration files created successfully!"
echo "📁 Files created:"
echo "  - monitoring/prometheus.yml"
echo "  - monitoring/grafana-dashboard.json"
echo ""
echo "🚀 Run the following command to start with monitoring:"
echo "  ./scripts/deploy.sh"