# Metrics Conventions

This document records the naming conventions for the Prometheus-style metrics
that neo-rs exposes, and the two deliberate deviations that are frozen for
compatibility. It is descriptive: it documents the conventions the codebase
already follows, so new metrics stay consistent and existing scrape tooling
keeps working.

## Metric prefix ownership

Every metric name begins with a crate-scoped prefix. The prefix tells you which
crate owns and emits the metric.

| Prefix | Owning crate / subsystem | Examples |
|--------|--------------------------|----------|
| `neo_node_` | `neo-node` (daemon, indexer, mempool gauges, task supervision) | `neo_node_ledger_height`, `neo_node_daemon_task_spawned_total`, `neo_node_mempool_transactions` |
| `neo_sync_` | `neo-node` sync/persistence metrics (`neo-node/src/node/sync_metrics`) | `neo_sync_height`, `neo_sync_blocks_persisted`, `neo_sync_avg_commit_us` |
| `neo_state_service_` | `neo-state-service` (MPT apply pipeline) | `neo_state_service_mpt_apply_height`, `neo_state_service_mpt_apply_avg_total_us` |
| `neo_storage_rocksdb_` | `neo-storage` RocksDB backend (batch writer) | `neo_storage_rocksdb_batch_bytes_written_total`, `neo_storage_rocksdb_batch_pending_operations` |
| `neo_rpc_` | `neo-rpc` (JSON-RPC request/error counters) | `neo_rpc_requests_total`, `neo_rpc_errors_total` |

When adding a metric, pick the prefix that matches the emitting crate/subsystem
and keep the rest of the name descriptive.

## Naming rules

- **Counters end in `_total`.** A monotonically increasing counter should carry
  the `_total` suffix (e.g. `neo_rpc_requests_total`,
  `neo_storage_rocksdb_batch_batches_flushed_total`).

  Known deviation: `neo_sync_blocks_persisted` is declared `# TYPE ... counter`
  but does **not** carry the `_total` suffix. Do **not** rename it — see
  "Frozen names" below.

- **Gauges** carry no `_total` suffix (e.g. `neo_sync_height`,
  `neo_node_ledger_height`, `neo_storage_rocksdb_batch_pending_operations`).

- **Duration metrics use the `_us` suffix** (microseconds). This is the
  established convention across the sync and state-service metrics
  (e.g. `neo_sync_avg_commit_us`, `neo_sync_avg_verify_us`,
  `neo_state_service_mpt_apply_avg_total_us`).

  Note: the Prometheus base-unit convention would use `_seconds` instead. neo-rs
  deliberately keeps `_us` — see "Frozen names" below.

## Frozen names

Two conventions deviate from strict Prometheus base-unit / naming guidance and
are intentionally frozen, because `scripts/run-bounded-mainnet-replay.py` scrapes
metrics by exact name and matching a fixed list breaks if the names change:

- **`_us` duration suffix is not renamed to `_seconds`.** The replay script's
  `DEFAULT_METRIC_NAMES` list matches names such as `neo_sync_avg_total_us` and
  `neo_state_service_mpt_apply_avg_us` verbatim. Renaming to the Prometheus
  base-unit `_seconds` would break that scrape parsing, so `_us` is frozen.

- **`neo_sync_blocks_persisted` keeps its non-`_total` name.** The same replay
  script matches `neo_sync_blocks_persisted` by exact name. It is a known
  deviation from the counter naming rule, flagged here rather than renamed.

Before renaming any scraped metric, update
`scripts/run-bounded-mainnet-replay.py` (and any other consumers) in the same
change.
