# Neo-RS Mainnet Deployment - Quick Reference

## Build Command
```bash
# Linux (recommended)
cargo build --profile production --features "full" --package neo-node

# Windows (use MSVC toolchain)
rustup default stable-x86_64-pc-windows-msvc
cargo build --profile production --features "full" --package neo-node --target x86_64-pc-windows-msvc
```

## Deployment Steps
1. Build binary with production profile
2. Run `./scripts/install.sh` (requires root)
3. Configure credentials in `/opt/neo-node/config/mainnet-production.toml`
4. Download snapshot (optional, recommended): `wget https://sync.ngd.network/mainnet-snapshot-latest.tar.gz`
5. Start service: `systemctl start neo-node`
6. Verify: `./scripts/healthcheck.sh`

## Configuration Highlights
- **RPC**: localhost:10332 (authentication required)
- **P2P**: 0.0.0.0:10333
- **Metrics**: localhost:9090
- **Data**: /opt/neo-node/data/mainnet
- **Logs**: /opt/neo-node/logs/

## Security Checklist
- [x] RPC bound to localhost only
- [x] Authentication enabled (set credentials!)
- [x] Rate limiting: 100 req/s
- [x] Wallet methods disabled
- [x] Debug symbols stripped
- [ ] Configure firewall (allow 10333, block 10332 externally)
- [ ] Set file permissions (700 for data, 600 for config)
- [ ] Run as non-root user (neo:neo)
- [ ] Enable systemd hardening

## Monitoring
- **Prometheus**: http://localhost:9090
- **Grafana Dashboard**: Import `monitoring/neo-node-dashboard.json`
- **Health Check**: `./scripts/healthcheck.sh`
- **Logs**: `journalctl -u neo-node -f`

## Maintenance
- **Backup**: `./scripts/backup.sh` (keeps 7 days)
- **Update**: Test in staging, stop node, replace binary, restart
- **Migration from C#**: `./scripts/migration/migrate_from_csharp.sh`

## System Requirements
**Minimum**: 4 CPU cores, 16GB RAM, 500GB SSD
**Recommended**: 16+ CPU cores, 64GB RAM, 2TB NVMe SSD, 1Gbps network

## Known Issues
⚠️ **Windows Build**: RocksDB compilation fails with MinGW. Use MSVC toolchain or build on Linux.

## Support
- Repository: https://github.com/r3e-network/neo-rs
- Documentation: https://docs.rs/neo-rs
- Neo Network: https://neo.org
