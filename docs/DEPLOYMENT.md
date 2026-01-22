# Deployment Guide

Production-oriented checklist for running `neo-cli` as a long-lived Neo N3 node.

## Prerequisites
- Rust build artifacts produced with `cargo build --release --workspace`.
- `librocksdb` installed on the target host.
- Dedicated data volume for RocksDB (avoid tmpfs/ephemeral disks).
- OS limits raised: e.g. `nofile >= 65535`, `nproc` sufficient for your workload.

## Directory layout
- Binaries: `target/release/neo-cli`
- Configs: `neo_mainnet_node.toml` or `neo_production_node.toml`
- Data: `/var/neo/data` (or your chosen path; set via `--storage`/TOML)
- Plugins: `/var/neo/Plugins` (or set `NEO_PLUGINS_DIR` to a persistent, writable path)
- Logs: `/var/log/neo` (or use journald)

## Systemd unit example
Create `/etc/systemd/system/neo-cli.service`:

```
[Unit]
Description=Neo N3 Rust Node
After=network.target
Wants=network-online.target

[Service]
User=neo
WorkingDirectory=/opt/neo
ExecStart=/opt/neo/neo-cli --config /opt/neo/neo_production_node.toml
Restart=always
RestartSec=5
LimitNOFILE=65535
Environment=RUST_LOG=info
Environment=NEO_PLUGINS_DIR=/var/neo/Plugins
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

Then:
```
sudo systemctl daemon-reload
sudo systemctl enable --now neo-cli
```

## Backups
- Periodically tar/rsync the RocksDB directory (configured `data_dir`).
- Keep config and wallet backups (ensure keys are encrypted and stored securely).

## Monitoring
- Enable `RUST_LOG=info` (or `debug` temporarily).
- Watch `Logs/` or journald for warnings/faults.
- Track peer counts and block height via CLI commands (e.g., `show node`, `show block`).

## Upgrades
- Stop the service, deploy new binary/config, start the service.
- If schema changes occur, follow release notes; keep a backup before upgrading.

## Containers
- Build the image: `docker build -t neo-rs .`
- Run on TestNet with persisted data: `docker run -d --name neo-node -p 20332:20332 -p 20333:20333 -v /var/lib/neo:/data neo-rs`
- Switch to MainNet: add `-e NEO_NETWORK=mainnet` (RPC defaults to 10332/10333).
- Override config: mount a TOML file and set `-e NEO_CONFIG=/path/in/container.toml`.
- Persist plugin configs: set `NEO_PLUGINS_DIR` to a host-mounted directory (defaults to `/data/Plugins` inside the container).
- Custom RPC port: the entrypoint will read the port from the TOML `[rpc]` section when possible; set `NEO_RPC_PORT` to force a value for the health check.
- RPC exposure: the production TOML disables CORS; if exposing RPC beyond localhost, front it with a reverse proxy (TLS/auth/rate limits) instead of opening it directly.
- Plugin visibility: `listplugins` is available but disabled by default in production configs; only enable remotely behind auth/proxy if you need plugin inventory. Locally, `neo-cli plugins active` shows loaded plugins. The Rust build ships only the stable services (rpc-server, rocksdb-store, application-logs, tokens-tracker). `tokens-tracker` provides NEP-11/NEP-17 balance and transfer tracking when enabled. Consensus (dBFT) is not wired into `neo-node` yet. `state-service` is optional and only enabled when configured (or via `--state-root`). Experimental plugins (sign-client, storage-dumper, oracle, leveldb-store) are removed.
- Custom storage backend: set `NEO_BACKEND` (e.g., `rocksdb`) to pass through to `neo-cli --backend`.
- Override P2P listen port: set `NEO_LISTEN_PORT` to pass `--listen-port` without changing the TOML.
- Containers run as the unprivileged `neo` user (`/home/neo`); ensure mounted volumes are writable by this UID.
- Using Docker Compose: `docker compose up -d neo-node` (see `docker-compose.yml`), which also raises `nofile`/`nproc` ulimits by default and exposes envs for custom configs/backends/ports. Copy `.env.example` to `.env` to start with sensible defaults.
- Makefile shortcuts: `make compose-up`, `make compose-logs`, `make compose-ps`, `make compose-monitor`, and `make compose-down` wrap the compose commands.
- Logs: default `logging.path` in the production TOML is `/data/Logs/neo-cli.log`; ensure this directory exists or override the path.
- RPC hardening: see `docs/RPC_HARDENING.md` for a secured `RpcServer.json` and proxy guidance.
- Monitoring: see `docs/MONITORING.md` for signals/alerts.
- Sample RPC plugin config: copy `config/Plugins/RpcServer/RpcServer.json.example` into your `Plugins/RpcServer` directory and adjust network/credentials.
- Optional Grafana dashboard: `docker compose --profile monitoring up -d neo-monitor` (or `make compose-monitor`).

## Operations
See `docs/OPERATIONS.md` for ongoing health checks, backups, monitoring hints, and incident response basics.
