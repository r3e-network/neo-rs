# Neo-RS Mainnet Deployment Package - Package Manifest

## Package Overview
- **Package Name**: neo-rs-mainnet-v0.17.0
- **Target Network**: Neo N3 Mainnet
- **Build Profile**: production
- **Features**: full (rocksdb, upnp, monitoring, oracle, neofs-grpc, parallel)
- **Created**: 2025-01-01
- **Status**: Ready for deployment (binary build required)

## Package Structure

```
deployment/mainnet/
├── DEPLOYMENT_MANIFEST.md      # Complete deployment documentation
├── README.md                   # Quick reference guide
├── BUILD_NOTES.md             # Build instructions and known issues
├── Dockerfile                  # Docker containerization
├── docker-compose.yml         # Complete stack deployment
│
├── bin/                       # Binary artifacts (to be populated)
│   └── neo-node               # Production binary (requires build)
│
├── config/                    # Configuration files
│   ├── mainnet.toml          # Standard mainnet config
│   └── mainnet-production.toml # Hardened production config
│
├── scripts/                   # Deployment and maintenance scripts
│   ├── install.sh            # Automated installation script
│   ├── healthcheck.sh        # Node health monitoring
│   ├── backup.sh             # Backup automation
│   └── migration/
│       └── migrate_from_csharp.sh  # C# to Rust migration
│
├── monitoring/                # Observability stack
│   ├── prometheus.yml        # Prometheus configuration
│   └── neo-node-dashboard.json  # Grafana dashboard
│
└── docs/                      # Additional documentation
    └── SECURITY_CHECKLIST.md  # Security hardening guide
```

## File Inventory

### Documentation (5 files)
1. `DEPLOYMENT_MANIFEST.md` - Comprehensive deployment guide (13 sections)
2. `README.md` - Quick reference and commands
3. `BUILD_NOTES.md` - Build instructions and troubleshooting
4. `docs/SECURITY_CHECKLIST.md` - Security hardening checklist

### Configuration (2 files)
1. `config/mainnet.toml` - Standard mainnet configuration
2. `config/mainnet-production.toml` - Production-hardened configuration
   - RPC authentication enabled
   - Rate limiting configured
   - Resource limits set
   - Security settings optimized

### Scripts (4 files, all executable)
1. `scripts/install.sh` - Automated deployment script
   - System requirements validation
   - User and directory creation
   - Binary installation
   - Systemd service setup
   - Firewall configuration

2. `scripts/healthcheck.sh` - Health monitoring script
   - Service status
   - RPC endpoint check
   - Sync status
   - Peer connections
   - System resources
   - Metrics validation

3. `scripts/backup.sh` - Backup automation
   - Data backup
   - Config backup
   - Checksum verification
   - Retention policy (7 days)

4. `scripts/migration/migrate_from_csharp.sh` - Migration tool
   - C# node data analysis
   - Snapshot download
   - Migration guidance
   - Verification steps

### Containerization (2 files)
1. `Dockerfile` - Multi-stage production build
   - Rust 1.88 builder
   - Debian slim runtime
   - Security hardening
   - Health checks

2. `docker-compose.yml` - Complete stack
   - neo-node service
   - Prometheus monitoring
   - Grafana dashboards
   - Volume management
   - Network isolation

### Monitoring (2 files)
1. `monitoring/prometheus.yml` - Metrics collection
   - 15s scrape interval
   - 30-day retention
   - Multiple exporters
   - Alert rules ready

2. `monitoring/neo-node-dashboard.json` - Grafana dashboard
   - Block height tracking
   - Transaction throughput
   - Peer connections
   - Resource utilization
   - Cache performance
   - RPC metrics

### Binary (not included - requires build)
- `bin/neo-node` - Production binary
  - **Build Required**: See BUILD_NOTES.md
  - **Size**: ~50-200MB (after strip)
  - **Features**: full feature set enabled

## Build Status

⚠️ **Binary Not Built**: The production binary could not be built due to RocksDB compilation issues on Windows with MinGW toolchain.

**Build Solutions** (see BUILD_NOTES.md):
1. Build on Linux (recommended)
2. Use Windows MSVC toolchain
3. Build in Docker container
4. Use CI/CD pipeline

## Deployment Prerequisites

### System Requirements
- **OS**: Linux (Ubuntu 22.04+, Debian 12+) or Windows Server 2022+
- **CPU**: 4+ cores (16+ recommended)
- **RAM**: 16GB minimum (64GB recommended)
- **Storage**: 500GB SSD minimum (2TB NVMe recommended)
- **Network**: 100 Mbps minimum (1 Gbps recommended)

### Software Dependencies
- Rust 1.88+ (for building)
- C++ compiler (GCC 11+/Clang 15+ on Linux, MSVC 2022+ on Windows)
- RocksDB libraries
- OpenSSL 3.0+

## Configuration Highlights

### Security Features
- ✅ RPC localhost binding (127.0.0.1)
- ✅ Authentication required
- ✅ Rate limiting (100 req/s)
- ✅ CORS disabled
- ✅ Wallet methods disabled
- ✅ Systemd hardening enabled
- ⚠️ **ACTION REQUIRED**: Set RPC credentials

### Performance Optimizations
- Predictive prefetch enabled
- Tiered cache system (512MB tier3)
- Parallel processing (rayon)
- Optimized RocksDB settings
- 50,000 mempool capacity

### Network Configuration
- **P2P Port**: 10333 (public)
- **RPC Port**: 10332 (localhost only)
- **Metrics Port**: 9090 (localhost only)
- **Max Connections**: 100 peers
- **Seed Nodes**: seed1-5.neo.org

## Deployment Workflow

### Quick Start (Linux)
```bash
# 1. Build binary (on Linux or in Docker)
cargo build --profile production --features "full" --package neo-node
cp target/production/neo-node deployment/mainnet/bin/

# 2. Run installation
cd deployment/mainnet
sudo ./scripts/install.sh

# 3. Configure credentials
sudo nano /opt/neo-node/config/mainnet-production.toml
# Set username and password

# 4. Download snapshot (optional, faster)
sudo -u neo wget https://sync.ngd.network/mainnet-snapshot-latest.tar.gz
sudo -u neo tar -xzf mainnet-snapshot-latest.tar.gz -C /opt/neo-node/data/

# 5. Start node
sudo systemctl start neo-node

# 6. Monitor
sudo journalctl -u neo-node -f
./scripts/healthcheck.sh
```

### Docker Deployment
```bash
cd deployment/mainnet
docker-compose up -d

# Monitor
docker-compose logs -f neo-node
```

## Post-Deployment Validation

### Critical Checks
- [ ] Service started successfully
- [ ] Blockchain syncing (check block height)
- [ ] Peer connections >10
- [ ] RPC endpoint responding
- [ ] Metrics endpoint accessible
- [ ] No errors in logs
- [ ] Firewall rules applied
- [ ] Backups configured

### Performance Validation
- [ ] Block processing <1s
- [ ] RPC latency <50ms (p95)
- [ ] Memory usage stable <32GB
- [ ] CPU usage <80% average

## Maintenance Procedures

### Daily
- Monitor sync status
- Check peer connections
- Review error logs
- Verify disk space

### Weekly
- Run health check script
- Verify backup completion
- Review metrics dashboards
- Check for updates

### Monthly
- Security updates
- Performance review
- Capacity planning
- Backup restoration test

## Support Resources

### Documentation
- Full Manifest: `DEPLOYMENT_MANIFEST.md`
- Quick Reference: `README.md`
- Build Guide: `BUILD_NOTES.md`
- Security: `docs/SECURITY_CHECKLIST.md`

### External Resources
- Repository: https://github.com/r3e-network/neo-rs
- Documentation: https://docs.rs/neo-rs
- Neo Network: https://neo.org

### Contact
- Maintainer: R3E Network <jimmy@r3e.network>
- License: MIT

## Package Integrity

### Checksums (to be generated after binary build)
```bash
# Generate checksums
cd deployment/mainnet
find . -type f -exec sha256sum {} \; > CHECKSUMS.sha256
```

### Verification
```bash
# Verify package integrity
sha256sum -c CHECKSUMS.sha256
```

## Known Limitations

### Build Issues
- ⚠️ RocksDB fails to compile on Windows with MinGW
- ✅ Solution: Use Linux build or Windows MSVC toolchain

### Runtime Considerations
- Initial sync: 2-3 days from genesis (use snapshot instead)
- Memory usage: Can peak at 32GB during initial sync
- Disk I/O: Heavy during sync, requires SSD with high IOPS

### Feature Status
- ✅ Full node functionality
- ✅ RPC server
- ✅ State service and MPT trie
- ✅ Prometheus metrics
- ⚠️ Oracle service (requires additional configuration)
- ⚠️ NeoFS integration (optional, requires NeoFS deployment)

## Version History
- v0.17.0 (2025-01-01): Initial mainnet deployment package

## Next Steps

1. **Build Binary**: Follow BUILD_NOTES.md to compile production binary
2. **Security Review**: Complete SECURITY_CHECKLIST.md
3. **Deploy**: Run install.sh and follow deployment workflow
4. **Monitor**: Set up Prometheus/Grafana dashboards
5. **Backup**: Configure automated backups
6. **Test**: Validate all functionality before mainnet launch

---

**Package Status**: ✅ Ready for deployment (pending binary build)  
**Last Updated**: 2025-01-01  
**Maintainer**: R3E Network
