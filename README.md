# neo-rs

Rust implementation of the Neo N3 node stack, including the virtual machine, core protocol logic, and `neo-cli` command line interface.

For a high-level tour of crate boundaries and service lifecycles, see `docs/ARCHITECTURE.md`.

## Compatibility

| neo-rs Version | Neo N3 Version | C# Reference Commit |
|----------------|----------------|---------------------|
| 0.4.x          | 3.8.2          | [`ede620e`](https://github.com/neo-project/neo/commit/ede620e5722c48e199a0f3f2ab482ae090c1b878) |

This implementation maintains byte-for-byte serialization compatibility with the official C# Neo implementation for blocks, transactions, and P2P messages.

## Prerequisites

- Rust (stable toolchain recommended)
- RocksDB native library (required by the default storage provider). On Ubuntu/Debian: `sudo apt-get install librocksdb-dev`.

## Build

```bash
cargo build --workspace
```

Release build for production:

```bash
cargo build --workspace --release
```

The `neo-cli` crate always ships with the full stable plugin set (dBFT, RocksDB storage, RPC server, application logs, token tracker, SQLite wallet). Plugin toggles are runtime-only; Cargo features are no longer used to select plugins.

## Run the node

`neo-cli` is the entry point:

```bash
# Uses neo_mainnet_node.toml by default
cargo run -p neo-cli --release -- --config neo_mainnet_node.toml
```

Common overrides:

- `--storage <path>`: custom RocksDB path
- `--backend <memory|rocksdb>`: storage backend
- `--network-magic <u32>` / `--port <u16>`: network parameters

Use `cargo run -p neo-cli -- --help` for the full command list.

Useful CLI commands:
- `plugins`: list available/downloadable plugins from the release feed (checks installed dirs too)
- `plugins active`: show plugins currently loaded in the running node (name/version/category)
- `open wallet <path> <password>`: unlock a NEP-6 wallet for RPC/console actions

Validate a node config without starting the daemon:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-config
```

Validate storage accessibility without starting the daemon:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-storage
```

Run both checks in one go:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-all
```

Preflight both bundled configs:

```bash
make preflight
```

Environment overrides:
- `NEO_CONFIG` (path to TOML), `NEO_STORAGE` (data path), `NEO_BACKEND` (storage backend)
- `NEO_STORAGE_READONLY` (open storage read-only; use with `--check-*` only)
- `NEO_NETWORK_MAGIC`, `NEO_LISTEN_PORT`, `NEO_SEED_NODES`
- `NEO_MAX_CONNECTIONS`, `NEO_MIN_CONNECTIONS`, `NEO_MAX_CONNECTIONS_PER_ADDRESS`, `NEO_BROADCAST_HISTORY_LIMIT`
- `NEO_BLOCK_TIME`, `NEO_DISABLE_COMPRESSION`, `NEO_DAEMON`
- `NEO_RPC_BIND`, `NEO_RPC_PORT`, `NEO_RPC_DISABLE_CORS`, `NEO_RPC_USER`, `NEO_RPC_PASS`, `NEO_RPC_TLS_CERT`, `NEO_RPC_TLS_PASS`
- `NEO_RPC_ALLOW_ORIGINS`, `NEO_RPC_DISABLED_METHODS`
- `NEO_LOG_PATH`, `NEO_LOG_LEVEL`, `NEO_LOG_FORMAT`
- `NEO_HEALTH_PORT` to expose `/healthz` on localhost
- `NEO_HEALTH_MAX_HEADER_LAG` to fail `/healthz` if header lag exceeds the threshold
- `/metrics` is available when the health server is enabled; scrape it with Prometheus.
- `/readyz` is available when the health server is enabled (same contract as `/healthz`).

Hardened RPC preset:
- Use `--rpc-hardened` (or set via CLI) to disable CORS, require auth, and disable `openwallet`/`getplugins` by default; combine with `NEO_RPC_USER/NEO_RPC_PASS`.

Example hardened run:

```bash
NEO_RPC_USER=admin NEO_RPC_PASS="$(openssl rand -hex 16)" \
NEO_RPC_BIND=127.0.0.1 NEO_RPC_PORT=10332 \
cargo run -p neo-node -- --config neo_mainnet_node.toml --rpc-hardened --check-all
```

## Docker

Build an image and run on TestNet with a persistent data volume:

```bash
docker build -t neo-rs .
docker run -d --name neo-node \
  -p 20332:20332 -p 20333:20333 \
  -v $(pwd)/data:/data \
  -e NEO_NETWORK=testnet \
  neo-rs
```

Key environment knobs:
- `NEO_NETWORK`: `testnet` (default) or `mainnet` to pick the bundled TOML config.
- `NEO_STORAGE`: RocksDB path inside the container (defaults to `/data/testnet` or `/data/mainnet` based on `NEO_NETWORK`).
- `NEO_CONFIG`: custom config path if you bind-mount your own TOML.
- `NEO_PLUGINS_DIR`: where plugin configs (e.g., RpcServer.json) are written; defaults to `/data/Plugins`.
- `NEO_BACKEND`: storage backend passed to `--backend` (default `rocksdb` in Docker/compose).
- `NEO_RPC_PORT`: if set, forces the RPC port (used by the health check). Otherwise the entrypoint will try to read the port from the TOML `[rpc]` section and fall back to network defaults.
- `NEO_LISTEN_PORT`: override the P2P listen port without editing the TOML.
- Containers run as an unprivileged `neo` user with home at `/home/neo`; mount data under `/data` for persistence.

Health checks hit `getversion` on the detected RPC port (parsed from the config when possible; otherwise 20332 for TestNet or 10332 for MainNet). See `docker-compose.yml` for a compose-based setup.

## Security

Please see `SECURITY.md` for vulnerability reporting guidelines.

## Contributing

See `CONTRIBUTING.md` for development, testing, and release note guidelines before opening a PR.
Use the GitHub issue templates for bug reports and feature requests; for security issues, follow `SECURITY.md`.

Using Docker Compose (defaults to TestNet):

```bash
# optional: cp .env.example .env and tweak values
docker compose up -d neo-node
# or use Makefile helpers
make compose-up   # start
make compose-logs # tail logs
make compose-down # stop/remove
make compose-ps   # status
make compose-monitor # start Grafana (monitoring profile)
```

Optional monitoring (Grafana) is behind a compose profile:

```bash
docker compose --profile monitoring up -d neo-monitor
make compose-monitor  # equivalent
```

Adjust `.env` or environment variables to switch to mainnet (`NEO_NETWORK=mainnet`), mount your own config (`NEO_CONFIG`), pick a backend (`NEO_BACKEND`), tweak ports, or change the storage location. The compose file also raises `nofile`/`nproc` limits for better production defaults.

## Tests

Run the full suite:

```bash
cargo test --workspace
```

For faster iterations you can target a specific crate or test:

```bash
cargo test -p neo-vm --test vm_integration_tests
```

## Linting & formatting

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

## Configuration

- `neo_mainnet_node.toml`: default mainnet settings.
- `neo_production_node.toml`: production template you can adjust for your environment.
- `NEO_PLUGINS_DIR`: set this env var to move plugin state/config (like `Plugins/RpcServer.json`) to a writable, persistent path.
- Config files are strict: unknown keys/tables fail parsing. Supported sections are `[network]`, `[p2p]`, `[storage]`, `[blockchain]`, `[rpc]`, `[logging]`, `[unlock_wallet]`, `[contracts]`, `[plugins]`.
- Validate configs without starting the node via `neo-node --check-config --config <path>`.
- Logging defaults to `/data/Logs/neo-cli.log` in Docker and can be moved via the config `logging.path`.
- If you use the bundled production TOML outside Docker, create the configured log directory (or override `logging.path`).
- See `docs/RPC_HARDENING.md` for a hardened `RpcServer.json` example and reverse-proxy guidance.
- See `docs/MONITORING.md` for signal/alert suggestions.
- Sample RPC plugin config: `config/Plugins/RpcServer/RpcServer.json.example` (copy to your `Plugins/RpcServer` directory and adjust network/credentials).

Logs and data directories default to `Logs/` and `data/` in the repository root; override via CLI flags or the TOML configuration.

## Production notes

- Build with `--release` and ensure `librocksdb` is available on the host.
- Data directories carry `NETWORK_MAGIC` and `VERSION` markers; start a node only with matching binaries/configs for that path.
- Read-only storage mode is available for offline checks (`NEO_STORAGE_READONLY=1` + `--check-storage/--check-all`); the node will refuse to start in read-only mode.
- Point `--storage` and `--config` to durable volumes; back up RocksDB data regularly.
- RPC security: CORS is disabled by default in the production TOML; expose RPC through a reverse proxy with TLS/auth and rate limits if publishing it beyond localhost.
- Ensure the log directory exists and is writable for the configured path (default `/data/Logs` in the production TOML).
- Keep plugin configs on persistent storage; set `NEO_PLUGINS_DIR` when running from a read-only prefix (containers, packages).
- Tune OS limits: increase `nofile` and `nproc`, and run under a service manager (systemd, supervisord) with restart policies.
- Set logging via `RUST_LOG=info` (or `debug` when diagnosing); rotate `Logs/` via your log manager.
- Keep peers and network magic consistent with your target network; verify via the TOML files.
- For a systemd-based setup, see `docs/DEPLOYMENT.md` for a sample unit and checklist.
- For day-to-day operations (health checks, backups, upgrades), see `docs/OPERATIONS.md`.
- Backups: use `scripts/backup-rocksdb.sh <rocksdb_path> [backup_dir]` (or `make backup-rocksdb ROCKSDB_PATH=/path/to/db BACKUP_DIR=backups`) and keep backups off the data volume; stopping the node during backup is recommended.
- Monitoring: see `docs/MONITORING.md` for suggested signals and alerts.
- Releases: `docs/RELEASE.md` covers tagging and the GHCR publish workflow.
