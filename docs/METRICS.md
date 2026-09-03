# Metrics & Health

`neo-node` can expose a small HTTP server (bound to localhost) that serves:

- `GET /healthz` (JSON)
- `GET /readyz` (JSON)
- `GET /metrics` (Prometheus text format)

Enable it with `--health-port <port>` (or `NEO_HEALTH_PORT=<port>`).

## Current Status (v0.7.x)

`neo-node` periodically updates health and Prometheus gauges from the live `NeoSystem`
(block/header height, peers, mempool size, state root ingest stats, and timeout counters).

Code references:

- Health server: `neo-node/src/health.rs`
- Metric definitions + exporter: `neo-node/src/metrics.rs`

## Prometheus Metrics

All metrics are registered via the `prometheus` crate and exposed at `GET /metrics`.

### Chain / Sync

- `neo_header_height`: Highest header seen.
- `neo_block_height`: Highest block persisted.
- `neo_header_lag`: Header lag in blocks.
- `neo_mempool_size`: Mempool size (transactions).
- `neo_peer_count`: Connected peer count.

### P2P Timeouts

- `neo_p2p_timeouts_handshake`: Handshake timeouts.
- `neo_p2p_timeouts_read`: Read timeouts.
- `neo_p2p_timeouts_write`: Write timeouts.

### Storage / Disk

- `neo_storage_free_bytes`: Free bytes on the disk hosting the configured storage path.
- `neo_storage_total_bytes`: Total bytes on the disk hosting the configured storage path.

### State Root (StateService)

- `neo_state_local_root_index`: Current local state root index (block height) when known; `-1` when unknown.
- `neo_state_validated_root_index`: Current validated state root index when known; `-1` when unknown.
- `neo_state_validated_lag`: Local vs validated lag; `-1` when unknown.
- `neo_state_roots_accepted_total`: Total accepted state roots since process start (gauge).
- `neo_state_roots_rejected_total`: Total rejected state roots since process start (gauge).
- `neo_state_roots_accepted`: Counter of accepted state roots since process start.
- `neo_state_roots_rejected`: Counter of rejected state roots since process start.

## Quick Checks

```bash
# health/readiness JSON (default bind: 127.0.0.1:<health_port>)
curl -s http://127.0.0.1:3030/healthz | jq .
curl -s http://127.0.0.1:3030/readyz  | jq .

# prometheus scrape
curl -s http://127.0.0.1:3030/metrics | head
```
