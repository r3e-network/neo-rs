# Operations Runbook

Practical checks and routines for running `neo-cli` in production.

## Daily/regular checks
- Verify RPC health: `curl -sf -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}' http://127.0.0.1:20332`
- When piping RPC output to `jq`, prefer `curl --compressed` so gzipped responses are decoded correctly.
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
- Live checkpoint (short pause, copy, resume): `scripts/checkpoint-live-rocksdb.sh <writer_pid> <rocksdb_path> [checkpoint_root]`.
- Periodic live checkpoints with rotation: `scripts/checkpoint-live-rocksdb-loop.sh <writer_pid> <rocksdb_path> [interval_secs] [max_checkpoints] [checkpoint_root]` (default interval `1800`, retention `8`).
- Restore: stop the service, untar the backup into the configured storage path, fix permissions for the `neo` user, then start the service.
- Disk space: keep at least 20% free where RocksDB resides; monitor inode usage.
- Network markers: the CLI writes `NETWORK_MAGIC` into the data dir; ensure you use a matching directory per network.
- Version markers: the node writes `VERSION` into the data dir; if it differs from the running binary, startup will fail—use a fresh path or migrate.
- Fail-fast on RocksDB: the node now aborts if RocksDB cannot be opened instead of silently falling back to memory; check permissions and available disk if startup fails.
- ContractManagement integrity guard: startup validates persisted non-native contract state for malformed payloads and duplicate contract IDs. If the check fails, startup aborts with a corruption error instead of continuing with undefined VM behavior.
- If you see a ContractManagement corruption error (for example duplicate non-native contract IDs), restore from a known-good backup or resync from a clean storage directory.
- After upgrading to builds with strict prefix-bound `DataCache.find` behavior, any node that previously ran divergent state (unexpected tx `FAULT`s vs TestNet/MainNet reference) should resync from a clean data directory.
- RocksDB backward-prefix parity: `find(prefix, Backward)` must include keys with extra suffix bytes (for example `prefix + u32`). Builds that start reverse iteration at the raw prefix can miss NeoToken GAS-per-block records, make `unclaimedGas` return `0` in live transfer paths, and drift state from early chain heights; resync from a clean checkpoint after upgrading.
- Contract caller-hash parity: inter-contract caller resolution must use `ExecutionContextState.script_hash` (contract hash), not raw VM script bytes hash. Builds with incorrect caller resolution can make contract-originated native transfers (for example GAS `transfer`) return `false` unexpectedly and drift chain state; resync from a trusted snapshot after upgrading.
- If block persistence repeatedly fails with `GasToken burn failed ... Insufficient balance for burn` on canonical network blocks, treat the local DB as divergent state and resync from a clean directory or trusted snapshot (this is not a recoverable peer/network timeout).
- Neo N3 manifest compatibility: `manifest.permissions = []` is valid and means "cannot call other contracts". Nodes that reject empty permissions can diverge during contract deployment replay.
- Native return-value compatibility: for `System.Contract.CallNative` methods returning `Array`/`Map`/`Any`, an empty payload must map to VM `null` (not empty byte string). Incorrect handling can flip valid deployment flows from `HALT` to `FAULT`.
- Before deploying changes, run `neo-node --check-config --config <path>` to catch schema/credential/storage issues without starting the daemon.
- To verify the RocksDB backend is reachable/writable, run `neo-node --check-storage --config <path>`; it will open the configured backend and exit.
- Shortcut: `neo-node --check-all --config <path>` runs both checks.
- For bundled configs, `make preflight` runs `--check-all` against MainNet and TestNet samples.
- For `--import-acc` bootstrap runs, the node defaults to `NEO_ROCKSDB_BATCH_PROFILE=high_throughput` (unless you set `NEO_ROCKSDB_BATCH_PROFILE` explicitly).
- Control import durability checkpoints with `NEO_IMPORT_FLUSH_INTERVAL` (default `10000` blocks). Lower values reduce recovery loss window after crashes at the cost of throughput.
- With `--storage <path>`, each import checkpoint flush now verifies persisted height/hash from a fresh read-only on-disk RocksDB view; import aborts immediately on mismatch.
- During import progress logs, `local_view_height` is the in-process view and can be ahead of on-disk state between flush checkpoints.
- To auto-verify stop-height parity after an import-only run exits: `scripts/wait-import-and-verify-testnet.sh <importer_pid> <storage_path> <target_height> [rpc_url]`.
- To verify stop-height storage parity for a specific key (block hash + `getstateroot/getstate` vs local RocksDB): `scripts/verify-testnet-storage-parity.sh <storage_path> <height> <script_hash> <contract_id> <key_suffix_hex> [rpc_url]`.
- Example (GAS totalSupply at TestNet height `128955`): `scripts/verify-testnet-storage-parity.sh data/repro-diverge-fixed-run 128955 0xd2a4cff31913016155e38e474a2c06d08be276cf -6 0b http://seed1t5.neo.org:20332`.
- For keys that do not exist at a given root, the parity script treats both remote `unknown storage item` and local missing key as `<none>` and reports parity success.
- Run storage-parity checks against a quiesced DB (for example after `--import-only` exits). The script now blocks when a live `neo-node` writer is detected on the same storage path; override only for debugging with `NEO_VERIFY_ALLOW_LIVE_DB=1`.
- When exposing RPC, prefer `--rpc-hardened` with `NEO_RPC_USER/NEO_RPC_PASS` to enforce auth, disable CORS, and keep risky methods disabled.
- Use `--health-port` (or `NEO_HEALTH_PORT`) to expose a localhost `/healthz` endpoint for liveness.
- Set `--health-max-header-lag` / `NEO_HEALTH_MAX_HEADER_LAG` (default 20; set 0 to disable) so `/healthz` returns 503 if headers are far ahead of persisted blocks (sync lag).
- For offline verification, you can open storage in read-only mode with `NEO_STORAGE_READONLY=1` and `--check-storage/--check-all`; the node will not start normally in read-only mode.
- `/readyz` is available alongside `/healthz` when the health server is enabled (same checks).

## Configuration changes
- Edit the TOML and restart the service to apply changes.
- For Docker/compose, update env vars (`NEO_NETWORK`, `NEO_CONFIG`, `NEO_STORAGE`, `NEO_BACKEND`, ports) and `docker compose up -d` to re-create the container.
- After changes, confirm RPC health and peer counts; watch logs for errors on startup. CORS is disabled by default; if exposing RPC beyond localhost, place it behind a reverse proxy with TLS/auth/rate limiting instead of opening it directly.
- Ensure the configured log directory exists and is writable (defaults to `/data/Logs` in the production TOML; create it on bare metal or mount it in containers).
- Native-contract strict security checks are **opt-in**:
  - default (`NEO_NATIVE_STRICT_SECURITY` unset): compatibility-first behavior for consensus parity.
  - strict mode (`NEO_NATIVE_STRICT_SECURITY=1`): enables additional native guard/invariant checks for hardening experiments.
  - do not enable strict mode on production consensus nodes unless you have validated full chain parity for your exact network/data set.

## TEE operation modes
- Strict TEE mode (fail-closed): `neo-node --config <path> --tee --tee-data-path <path>`.
- Opportunistic mode (prefer TEE, fallback to ordinary node): `neo-node --config <path> --tee-auto --tee-data-path <path>`.
- Ordinary mode (no TEE): start without `--tee`/`--tee-auto`.
- In strict mode, TEE initialization/self-check/attestation failures stop startup.
- In opportunistic mode, the node logs a warning and continues without TEE when TEE setup fails.
- SGX runtime validation loop (includes peer checks, sync progress, repeated RPC validation, and TEE key-export denial checks):
  `scripts/validate-tee-sgx-runtime.sh --iterations 100 --require-block-progress`.
- If strict SGX startup fails with non-terminal DCAP status (for example `0xA008`), use `--allow-non-terminal-qv` only as a temporary operator override while platform remediation is in progress.
- TestNet strict SGX validation example (real hardware + 100-loop runtime checks):
  `scripts/validate-tee-sgx-runtime.sh --config neo_testnet_node.toml --rpc-url http://127.0.0.1:20332 --storage /tmp/neo-tee-validate-storage --iterations 100 --require-block-progress --allow-non-terminal-qv`.
- If another local process already binds default P2P ports (for example `10333`/`20333`), run validation with alternate ports:
  `--listen-port 30333 --rpc-port 30332 --rpc-url http://127.0.0.1:30332`.
- Opportunistic fallback check: run with an invalid evidence path and verify log line `TEE auto mode: runtime initialization failed; continuing without TEE`, then verify RPC/P2P still progresses.
- Ordinary-mode control check: run without `--tee`/`--tee-auto` on a clean storage path and verify `getconnectioncount > 0`, `getblockcount` increases, and `invokefunction ... totalSupply` returns `HALT`.
- Useful log checks:
  - strict enable: `verified SGX quote and sealing key binding in strict mode`
  - auto fallback: `TEE auto mode: runtime initialization failed; continuing without TEE`
  - auto success: `TEE auto mode: runtime initialized successfully`

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
- If startup fails with a ContractManagement integrity error, do not keep restarting the same data directory. Move the corrupted directory aside, restore a backup, or re-bootstrap/resync.
- If RPC is overloaded: raise `rpc.max_connections` / `NEO_RPC_PORT` and place a reverse proxy with rate limits; consider moving RPC to a dedicated instance.
- If disk is full: expand the volume, prune old backups/logs, and keep RocksDB on fast, durable storage.
- If plugin state looks off: use `neo-cli plugins active` (local) to see loaded plugins. The `listplugins` RPC is disabled by default in production; only enable it behind auth/proxy if you need remote visibility.

## Upgrades
- Backup data and configs.
- Deploy new binaries (or rebuild Docker image), then restart the service.
- Watch logs during catch-up; verify RPC health and height parity after upgrade.
