# Operations Runbook

Practical checks and routines for running `neo-cli` in production.

## Daily/regular checks
- Verify RPC health: `curl -sf -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}' http://127.0.0.1:20332`
- Check sync status: `neo-cli show block` and compare header height to trusted explorers/seeds.
- Inspect peers: `neo-cli show node` for connected peer counts and penalties/timeouts.
- Review logs: `journalctl -u neo-cli -p warning..alert --since "1 hour ago"` (or tail `Logs/neo-cli-*.log` / `/data/Logs/neo-cli.log` depending on your config).

## Service control (systemd example)
- Restart: `sudo systemctl restart neo-cli`
- Status: `sudo systemctl status neo-cli`
- Logs: `journalctl -u neo-cli -f`

## Data and storage
- Location: see `--storage` or TOML `storage.path` (`/var/neo/...` recommended; `/data/...` in Docker).
- Backups: stop the service, snapshot the RocksDB directory (e.g., `tar czf /backups/neo-$(date +%F).tgz /var/neo/mainnet`), then start the service. The helper `scripts/backup-rocksdb.sh <rocksdb_path> [backup_dir]` (or `make backup-rocksdb ROCKSDB_PATH=/var/neo/mainnet BACKUP_DIR=/backups`) automates this.
- Restore: stop the service, untar the backup into the configured storage path, fix permissions for the `neo` user, then start the service.
- Disk space: keep at least 20% free where RocksDB resides; monitor inode usage.
- Network markers: the CLI writes `NETWORK_MAGIC` into the data dir; ensure you use a matching directory per network.

## Configuration changes
- Edit the TOML and restart the service to apply changes.
- For Docker/compose, update env vars (`NEO_NETWORK`, `NEO_CONFIG`, `NEO_STORAGE`, `NEO_BACKEND`, ports) and `docker compose up -d` to re-create the container.
- After changes, confirm RPC health and peer counts; watch logs for errors on startup. CORS is disabled by default; if exposing RPC beyond localhost, place it behind a reverse proxy with TLS/auth/rate limiting instead of opening it directly.
- Ensure the configured log directory exists and is writable (defaults to `/data/Logs` in the production TOML; create it on bare metal or mount it in containers).

## Monitoring hints
- Expose node RPC locally and scrape basic liveness via the health probe above.
- Track:
  - Block height and header height
  - Peer counts and connection churn
  - Mempool size
  - RocksDB disk usage and IOPS
  - Process memory and FD count (`nofile` should allow >= 65535)
- Use the optional Grafana compose profile (`make compose-monitor` or `docker compose --profile monitoring up -d neo-monitor`) as a starting point; wire in dashboards that show height lag and peer counts.
- See `docs/MONITORING.md` for signals/alerts to implement.

## Incident response basics
- If out of sync: restart, then check peers/ports and compare network magic/seed list. If the DB is corrupt, restore from the latest good backup and resync.
- If RPC is overloaded: raise `rpc.max_connections` / `NEO_RPC_PORT` and place a reverse proxy with rate limits; consider moving RPC to a dedicated instance.
- If disk is full: expand the volume, prune old backups/logs, and keep RocksDB on fast, durable storage.
- If plugin state looks off: use `neo-cli plugins active` (local) to see loaded plugins. The `getplugins` RPC is disabled by default in production; only enable it behind auth/proxy if you need remote visibility.

## Upgrades
- Backup data and configs.
- Deploy new binaries (or rebuild Docker image), then restart the service.
- Watch logs during catch-up; verify RPC health and height parity after upgrade.
