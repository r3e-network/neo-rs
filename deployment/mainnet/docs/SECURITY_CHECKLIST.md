# Security Hardening Checklist for Neo-RS Mainnet Deployment

## Binary Security ✅
- [x] **Debug symbols stripped**: Enabled in production profile (`strip = true`)
- [x] **Panic = abort**: No unwinding overhead (`panic = "abort"`)
- [x] **Link-time optimization**: Full LTO enabled (`lto = "fat"`)
- [x] **Single codegen unit**: Maximum optimization (`codegen-units = 1`)
- [ ] **Additional hardening** (platform-specific):
  - Linux: Consider `-C target-feature=+crt-static` for static linking
  - Add PIE (Position Independent Executable) flags
  - Enable RELRO (Relocation Read-Only)
  - Stack canaries (usually enabled by default in Rust)

## Network Security ⚠️
- [x] **RPC localhost binding**: `bind_address = "127.0.0.1"`
- [x] **CORS disabled**: `cors_enabled = false`
- [x] **Authentication enabled**: `auth_enabled = true`
- [x] **Rate limiting**: 100 req/s with burst capacity
- [x] **Wallet methods disabled**: `["openwallet", "closewallet", ...]`
- [ ] **ACTION REQUIRED: Set RPC credentials**
  - Edit `/opt/neo-node/config/mainnet-production.toml`
  - Change `username = "CHANGE_ME"` and `password = "CHANGE_ME"`
  - Use strong, randomly generated credentials
- [ ] **ACTION REQUIRED: Configure firewall**
  ```bash
  # UFW (Ubuntu/Debian)
  sudo ufw allow 10333/tcp comment 'Neo P2P'
  sudo ufw deny 10332/tcp comment 'Neo RPC - use reverse proxy'
  sudo ufw deny 9090/tcp comment 'Prometheus - internal only'
  
  # Firewalld (RHEL/CentOS)
  sudo firewall-cmd --permanent --add-port=10333/tcp
  sudo firewall-cmd --permanent --remove-port=10332/tcp
  sudo firewall-cmd --reload
  ```
- [ ] **RECOMMENDED: Use reverse proxy for RPC**
  - Deploy nginx/caddy with TLS termination
  - Enable HTTP/2
  - Add additional rate limiting
  - WAF (Web Application Firewall) protection

## Storage Security
- [x] **Data directory isolation**: `/opt/neo-node/data/mainnet`
- [ ] **ACTION REQUIRED: Set filesystem permissions**
  ```bash
  sudo chown -R neo:neo /opt/neo-node
  sudo chmod 700 /opt/neo-node/data
  sudo chmod 600 /opt/neo-node/config/mainnet-production.toml
  sudo chmod 640 /opt/neo-node/logs/*.log
  ```
- [ ] **RECOMMENDED: Encrypt data at rest**
  - Use LUKS for disk encryption (Linux)
  - BitLocker (Windows Server)
  - Consider per-directory encryption for sensitive data
- [ ] **RECOMMENDED: Secure backups**
  - Encrypt backup archives
  - Store offsite with restricted access
  - Test restore procedures regularly

## Process Security
- [x] **Non-root user**: Service runs as `neo:neo`
- [x] **Systemd hardening**: Multiple security options enabled
  - `NoNewPrivileges=true`
  - `PrivateTmp=true`
  - `ProtectSystem=strict`
  - `ProtectHome=true`
  - `ProtectKernelTunables=true`
  - `RestrictRealtime=true`
- [ ] **RECOMMENDED: Additional process isolation**
  - Consider running in container/VM
  - AppArmor or SELinux profiles
  - Seccomp filtering

## Consensus Node Security (if enabled)
- [ ] **Wallet security**
  - Store consensus wallet in HSM if possible
  - Never store unencrypted private keys on disk
  - Require manual wallet unlock (no auto-unlock)
  - Use neo-hsm feature for hardware security module support
- [ ] **Network isolation**
  - Dedicated VLAN for consensus nodes
  - Firewall rules between consensus nodes
  - VPN/WireGuard for consensus communication
- [ ] **Monitoring and alerting**
  - Alert on missed blocks
  - Alert on peer disconnections
  - Alert on abnormal resource usage

## Monitoring and Logging Security
- [x] **Metrics endpoint localhost only**: `bind_address = "127.0.0.1"`
- [x] **Structured logging**: JSON format for SIEM integration
- [x] **Log rotation**: 10 files x 100MB
- [ ] **RECOMMENDED: Log aggregation**
  - Forward logs to secure log server
  - Enable audit logging for critical operations
  - Retain logs per compliance requirements
- [ ] **RECOMMENDED: Metrics security**
  - Prometheus with authentication
  - Grafana with strong passwords
  - VPN access for monitoring dashboard

## API Security
- [x] **Request size limits**: 10MB max request size
- [x] **Batch request limits**: 50 requests per batch
- [x] **Gas invoke limits**: 50,000,000 max gas
- [x] **Iterator limits**: 100 max results
- [ ] **RECOMMENDED: Additional API protection**
  - Rate limiting per API key (not just per IP)
  - Request signature validation
  - API versioning and deprecation policy

## DDoS Protection
- [x] **Rate limiting**: 100 req/s per IP
- [x] **Burst capacity**: 200 requests
- [x] **Connection limits**: 100 max P2P connections
- [x] **Peer limits**: 3 max peers per IP
- [x] **Blacklisting**: Auto-ban after 10 violations
- [ ] **RECOMMENDED: External DDoS protection**
  - Cloudflare or similar CDN (for RPC if public)
  - Hardware firewall with DDoS mitigation
  - BGP blackhole routing for large attacks

## Update and Patch Management
- [ ] **Establish update schedule**
  - Security patches: within 48 hours
  - Minor updates: monthly maintenance window
  - Major updates: quarterly with full testing
- [ ] **Staging environment**
  - Mirror production configuration
  - Test all updates before production
  - Document rollback procedures
- [ ] **Dependency scanning**
  - Regular `cargo audit` runs
  - Subscribe to Rust security advisories
  - Monitor Neo network security announcements

## Incident Response
- [ ] **Document incident response procedures**
  - Contact information for team members
  - Escalation procedures
  - Backup node activation
- [ ] **Regular security drills**
  - Test backup restoration
  - Simulate node compromise
  - Practice rollback procedures
- [ ] **Post-incident review**
  - Document what happened
  - Update procedures
  - Implement preventive measures

## Compliance and Auditing
- [ ] **Access logging**
  - Log all RPC requests
  - Log all administrative actions
  - Retain logs per compliance requirements
- [ ] **Regular security audits**
  - Quarterly vulnerability scans
  - Annual penetration testing
  - Code security review for custom changes
- [ ] **Compliance documentation**
  - Document security controls
  - Maintain change log
  - Regular compliance reviews

## Critical Actions Summary

### IMMEDIATE (Before Starting Node)
1. ✅ Set strong RPC credentials in config
2. ✅ Configure firewall rules (allow 10333, block 10332 externally)
3. ✅ Set correct filesystem permissions
4. ✅ Verify systemd service security settings

### FIRST 24 HOURS
1. ✅ Monitor initial sync for anomalies
2. ✅ Verify peer connections are legitimate
3. ✅ Test backup and restore procedures
4. ✅ Configure monitoring alerts

### FIRST WEEK
1. ⬜ Deploy reverse proxy for RPC (if exposing publicly)
2. ⬜ Set up log aggregation
3. ⬜ Enable disk encryption
4. ⬜ Document runbooks and procedures

### ONGOING
1. ⬜ Daily monitoring review
2. ⬜ Weekly backup verification
3. ⬜ Monthly security updates
4. ⬜ Quarterly security audit

## Security Contacts
- **Neo Security Team**: security@neo.org
- **Rust Security**: security@rust-lang.org
- **RocksDB Security**: Report via GitHub Security Advisory

---

**Last Updated**: 2025-01-01  
**Review Schedule**: Monthly  
**Next Review**: 2025-02-01
