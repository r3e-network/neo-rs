# Monitoring & Alerting

Guidance for observing a `neo-cli` deployment.

## Key signals
- Liveness: RPC `getversion` (already used by the Docker health check).
- Sync: block height/header height vs. trusted reference (seed RPC/explorer).
- P2P: connected peers, inbound/outbound balance, connection churn/timeouts.
- Mempool: size and acceptance rate.
- Storage: RocksDB disk usage, disk latency/IOPS, free space.
- Process: memory, CPU, file descriptors (`nofile`), threads, and container health.
- Health/metrics: scrape `/healthz` or `/readyz` (or `/metrics`) when `--health-port` is enabled; set `--health-max-header-lag` to fail health on sync gaps.

## How to collect
- RPC polling: scrape `getblockcount`/`getpeers` periodically; export to Prometheus/Influx via a lightweight sidecar or Telegraf HTTP input.
- Health/metrics: enable `--health-port` to expose `/healthz`, `/readyz` (JSON) and `/metrics` (Prometheus text) on localhost; proxy/scrape as needed.
- Host/container metrics: run `node_exporter`/`cAdvisor` or your preferred host agent; include filesystem metrics for the RocksDB volume and process FD counts.
- Logs: ship `/data/Logs/neo-cli.log` (or your configured log path) to your log stack with alerts on errors/timeouts/restarts.

## Alerts to start with
- Height lag > N blocks vs. reference for M minutes.
- Peer count below threshold for M minutes.
- Mempool size stuck at 0 or exploding past a cap.
- RocksDB volume free space < 20% or inode pressure.
- Process FD usage > 80% of `nofile` limit; repeated restarts.

## Grafana/Prometheus notes
- The sample Compose profile includes Grafana only; pair it with Prometheus/scrapers for the metrics above.
- Suggested dashboards: block/height lag, peer counts, RPC latency/error rates, disk IO and free space, process FDs.

## Health checks
- Keep the existing RPC `getversion` probe; add a secondary check comparing height to a seed RPC if you need stronger assurance.

## Backups/operations
- Tie backup failures (RocksDB snapshots) and restore events into your alerting so operators know when data protection is stale.
