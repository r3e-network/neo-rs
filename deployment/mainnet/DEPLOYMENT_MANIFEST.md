# Neo-RS Mainnet Deployment Package
**Version**: 0.17.0  
**Profile**: Production  
**Target**: Mainnet  
**Generated**: 2025-01-01

## Package Contents

### 1. Binary Artifacts
**Location**: `bin/`

#### Primary Binary
- **Name**: `neo-node` or `neo-node.exe`
- **Build Command**: 
  ```bash
  cargo build --profile production --features "full" --package neo-node
  ```
- **Features Enabled**:
  - `full` → includes: `rocksdb`, `upnp`, `monitoring`, `oracle`, `neofs-grpc`, `parallel`
  - RocksDB storage backend
  - UPnP port forwarding
  - Prometheus metrics
  - System monitoring (sysinfo)
  - Oracle service
  - NeoFS gRPC support
  - Parallel transaction processing

#### Build Profile Settings
```toml
[profile.production]
inherits = "release"
opt-level = 3
lto = "fat"              # Full link-time optimization
codegen-units = 1        # Maximum optimization
panic = "abort"          # No unwinding
strip = true             # Strip debug symbols
debug = false            # No debug info
incremental = false      # Clean builds only
```

#### Security Hardening
- ✅ Debug symbols stripped (`strip = true`)
- ✅ Panic abort mode (no unwinding overhead)
- ✅ Full LTO enabled
- ✅ Single codegen unit (maximum optimization)
- ⚠️ **Build Issue**: RocksDB compilation fails on Windows with MinGW toolchain
  - Error: `FILE_ID_INFO` undefined (Windows SDK headers)
  - **Workaround**: Build on Linux or use MSVC toolchain on Windows

### 2. Configuration Files
**Location**: `config/`

#### Primary Configuration: `mainnet.toml`
- **Network Magic**: `0x334F454E` (MainNet)
- **P2P Port**: 10333
- **RPC Port**: 10332 (localhost only by default)
- **Data Directory**: `./data/mainnet`
- **Storage Backend**: RocksDB
- **Max Connections**: 100 peers
- **Seed Nodes**: seed1-5.neo.org:10333

#### Security Configuration
```toml
[rpc]
enabled = true
bind_address = "127.0.0.1"  # ⚠️ LOCALHOST ONLY - do not expose publicly without firewall
cors_enabled = false         # Disabled for security
auth_enabled = false         # Consider enabling for production
max_gas_invoke = 50000000
disabled_methods = ["openwallet"]  # Wallet methods disabled

# DoS Protection
max_requests_per_second = 100
rate_limit_burst = 200
```

#### Performance Tuning
```toml
[mempool]
max_transactions = 50000  # Larger for mainnet workload

[performance.predictive_prefetch]
enabled = true
prediction_window_blocks = 10
hot_keys_per_block = 800
prefetch_batch_size = 200
prefetch_worker_threads = 4

[performance.tiered_cache]
tier0_capacity = 512
tier1_capacity = 100000
tier2_max_prefixes = 5000000
tier3_lru_size_mb = 512  # 512MB cache
```

#### Monitoring Configuration
```toml
[telemetry.metrics]
enabled = true
port = 9090
bind_address = "127.0.0.1"  # Prometheus endpoint

[logging]
level = "info"
format = "json"
file_path = "./logs/neo-node-mainnet.log"
max_file_size = "100MB"
max_files = 10
```

### 3. Security Hardening Checklist

#### Binary Security
- [x] Debug symbols stripped
- [x] Release optimizations enabled
- [x] Panic = abort (no unwinding)
- [ ] Consider additional hardening: PIE, RELRO, stack canaries (platform-dependent)

#### Network Security
- [x] RPC bound to localhost only
- [x] CORS disabled
- [x] Rate limiting enabled (100 req/s)
- [x] Wallet methods disabled
- [ ] **REQUIRED**: Configure firewall rules
  - Allow P2P: port 10333 (inbound/outbound)
  - Block RPC: port 10332 (external access)
  - Allow monitoring: port 9090 (internal only)
- [ ] **RECOMMENDED**: Enable RPC authentication for production
- [ ] **RECOMMENDED**: Use reverse proxy (nginx) with TLS for RPC

#### Storage Security
- [x] Data directory isolation
- [ ] **REQUIRED**: Set appropriate filesystem permissions
  - Data directory: 700 (owner only)
  - Config files: 600 (owner read/write only)
  - Logs: 640 (owner write, group read)

#### Process Security
- [ ] **RECOMMENDED**: Run as non-root user
- [ ] **RECOMMENDED**: Use systemd hardening (Linux)
  - `PrivateTmp=true`
  - `NoNewPrivileges=true`
  - `ProtectSystem=strict`
  - `ProtectHome=true`

### 4. Deployment Scripts
**Location**: `scripts/`

#### Installation Script: `install.sh`
- Validates system requirements
- Creates directory structure
- Sets file permissions
- Installs systemd service (Linux)

#### Health Check: `healthcheck.sh`
- Monitors node synchronization status
- Checks block height
- Validates peer connections
- Monitors resource usage

#### Backup Script: `backup.sh`
- Backs up blockchain data
- Backs up configuration
- Rotation policy (keep last 7 days)

### 5. Monitoring Setup
**Location**: `monitoring/`

#### Prometheus Configuration: `prometheus.yml`
- Scrape neo-node metrics endpoint
- 15-second scrape interval
- Retention: 30 days

#### Grafana Dashboard: `neo-node-dashboard.json`
- Block height and sync status
- Transaction throughput
- Peer connection metrics
- Memory and CPU usage
- RPC request rates
- Cache hit rates
- Storage I/O metrics

### 6. Migration Scripts
**Location**: `scripts/migration/`

#### From C# Neo Node
- **Script**: `migrate_from_csharp.sh`
- **Process**:
  1. Stop C# node gracefully
  2. Export blockchain data
  3. Import into neo-rs RocksDB format
  4. Validate block heights match
  5. Start neo-rs node

#### Database Upgrade
- **Script**: `upgrade_db.sh`
- Handles RocksDB schema migrations between versions

### 7. System Requirements

#### Hardware (Minimum)
- **CPU**: 4 cores (8+ recommended)
- **RAM**: 16 GB (32+ recommended)
- **Storage**: 500 GB SSD (NVMe recommended)
- **Network**: 100 Mbps (1 Gbps recommended)

#### Hardware (Recommended Production)
- **CPU**: 16+ cores
- **RAM**: 64 GB
- **Storage**: 2 TB NVMe SSD
- **Network**: 1 Gbps dedicated

#### Software Requirements
- **OS**: Linux (Ubuntu 22.04+, Debian 12+) or Windows Server 2022+
- **Rust**: 1.88+ (for building from source)
- **C++ Compiler**: 
  - Linux: GCC 11+ or Clang 15+
  - Windows: MSVC 2022+ (MinGW has RocksDB issues)
- **System Libraries**: OpenSSL 3.0+, librocksdb-dev

### 8. Build Instructions

#### Linux Production Build
```bash
# Install dependencies
sudo apt-get install -y build-essential libssl-dev librocksdb-dev clang

# Build production binary
cargo build --profile production --features "full" --package neo-node

# Binary location
./target/production/neo-node

# Strip and verify (already stripped by profile)
strip --strip-all ./target/production/neo-node
file ./target/production/neo-node
```

#### Windows Production Build
```bash
# Use MSVC toolchain (not MinGW)
rustup default stable-x86_64-pc-windows-msvc

# Build production binary
cargo build --profile production --features "full" --package neo-node --target x86_64-pc-windows-msvc

# Binary location
.\target\x86_64-pc-windows-msvc\production\neo-node.exe
```

#### Cross-Platform Build (Docker)
```dockerfile
FROM rust:1.88-bullseye as builder
RUN apt-get update && apt-get install -y librocksdb-dev clang
WORKDIR /build
COPY . .
RUN cargo build --profile production --features "full" --package neo-node

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y librocksdb6.11 ca-certificates
COPY --from=builder /build/target/production/neo-node /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/neo-node"]
```

### 9. Deployment Procedure

#### Initial Deployment
1. **Provision Infrastructure**
   - Allocate hardware/VM meeting requirements
   - Configure network and firewall rules
   - Set up monitoring infrastructure

2. **Build Binary**
   - Use production profile with full features
   - Verify binary integrity (checksum)
   - Test binary on staging environment

3. **Deploy Configuration**
   ```bash
   mkdir -p /opt/neo-node/{data,logs,config}
   cp config/mainnet.toml /opt/neo-node/config/
   chown -R neo:neo /opt/neo-node
   chmod 700 /opt/neo-node/data
   chmod 600 /opt/neo-node/config/mainnet.toml
   ```

4. **Initialize Database**
   ```bash
   # Fast sync from snapshot (recommended)
   wget https://sync.ngd.network/mainnet-snapshot-latest.tar.gz
   tar -xzf mainnet-snapshot-latest.tar.gz -C /opt/neo-node/data/
   
   # Or sync from genesis (slow, ~2-3 days)
   /usr/local/bin/neo-node --config /opt/neo-node/config/mainnet.toml
   ```

5. **Configure Systemd Service**
   ```bash
   cp scripts/neo-node.service /etc/systemd/system/
   systemctl daemon-reload
   systemctl enable neo-node
   systemctl start neo-node
   ```

6. **Verify Synchronization**
   ```bash
   # Check logs
   journalctl -u neo-node -f
   
   # Check RPC
   curl http://localhost:10332 -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
   
   # Monitor sync progress
   ./scripts/healthcheck.sh
   ```

7. **Enable Monitoring**
   ```bash
   # Configure Prometheus
   cp monitoring/prometheus.yml /etc/prometheus/
   systemctl restart prometheus
   
   # Import Grafana dashboard
   curl -X POST http://grafana:3000/api/dashboards/db \
     -H "Content-Type: application/json" \
     -d @monitoring/neo-node-dashboard.json
   ```

### 10. Post-Deployment Validation

#### Functional Tests
- [ ] Node starts successfully
- [ ] Blockchain synchronization progressing
- [ ] P2P connections established (>10 peers)
- [ ] RPC endpoint responding
- [ ] Metrics endpoint accessible
- [ ] Log rotation working

#### Performance Tests
- [ ] Block processing time < 1 second
- [ ] Transaction verification < 100ms
- [ ] RPC latency < 50ms (p95)
- [ ] Memory usage stable < 32GB
- [ ] CPU usage < 80% average

#### Security Tests
- [ ] RPC not accessible externally
- [ ] Firewall rules validated
- [ ] File permissions correct
- [ ] No exposed secrets in logs
- [ ] Rate limiting functional

### 11. Maintenance Procedures

#### Routine Maintenance
- **Daily**: Monitor sync status, peer connections, resource usage
- **Weekly**: Review logs for errors, check disk space, backup data
- **Monthly**: Update dependencies, security patches, performance review

#### Upgrade Procedure
1. Test new version in staging
2. Schedule maintenance window
3. Stop node gracefully
4. Backup data and config
5. Replace binary
6. Run migration scripts if needed
7. Start node and validate
8. Monitor for 24 hours

#### Emergency Procedures
- **Node Crash**: Auto-restart via systemd
- **Disk Full**: Enable log rotation, archive old data
- **Network Split**: Verify seed nodes, check firewall
- **Performance Degradation**: Review metrics, increase resources

### 12. Known Issues and Limitations

#### Build Issues
- ⚠️ **RocksDB Windows Build**: Fails with MinGW toolchain
  - **Solution**: Use MSVC toolchain or build on Linux
  - **Status**: Known issue in rocksdb-rs crate
  
#### Runtime Considerations
- **Initial Sync**: 2-3 days from genesis, or use snapshot
- **Memory Usage**: Can peak at 32GB during initial sync
- **Disk I/O**: Heavy during sync, ensure SSD with high IOPS

#### Feature Status
- ✅ Full node functionality
- ✅ RPC server
- ✅ Consensus participation (when enabled)
- ✅ State service and MPT trie
- ✅ Prometheus metrics
- ⚠️ Oracle service (requires additional configuration)
- ⚠️ NeoFS integration (optional, requires NeoFS deployment)

### 13. Support and Documentation

#### Configuration Reference
- **Full Config**: See `config/mainnet.toml` with inline comments
- **RPC Methods**: See `docs/rpc-reference.md`
- **Metrics**: See `docs/metrics-reference.md`

#### Troubleshooting
- **Logs Location**: `/opt/neo-node/logs/` or `./logs/`
- **Common Issues**: See `docs/troubleshooting.md`
- **Performance Tuning**: See `docs/performance-tuning.md`

#### Community Resources
- **Repository**: https://github.com/r3e-network/neo-rs
- **Documentation**: https://docs.rs/neo-rs
- **Neo Network**: https://neo.org

---

## Quick Start Commands

```bash
# Deploy package
tar -xzf neo-rs-mainnet-v0.17.0.tar.gz
cd neo-rs-mainnet-v0.17.0

# Run installation
sudo ./scripts/install.sh

# Start node
sudo systemctl start neo-node

# Check status
sudo systemctl status neo-node
./scripts/healthcheck.sh

# View logs
sudo journalctl -u neo-node -f

# Access monitoring
# Prometheus: http://localhost:9090
# Grafana: http://localhost:3000 (import dashboard)
```

---

**Package Integrity**
- Manifest Version: 1.0
- Generated: 2025-01-01
- Maintainer: R3E Network <jimmy@r3e.network>
- License: MIT
