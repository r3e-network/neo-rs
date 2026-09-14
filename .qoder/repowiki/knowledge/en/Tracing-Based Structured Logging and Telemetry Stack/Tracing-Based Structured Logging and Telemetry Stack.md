---
kind: logging_system
name: Tracing-Based Structured Logging and Telemetry Stack
category: logging_system
scope:
    - '**'
source_files:
    - neo-telemetry/src/lib.rs
    - neo-telemetry/src/logging.rs
    - neo-telemetry/src/node_logging.rs
    - neo-telemetry/src/config.rs
    - neo-core/src/telemetry/mod.rs
    - neo-core/src/application_logs/service.rs
---

## What system/approach is used

The repository uses **`tracing`** as the unified structured logging framework across all crates, with `tracing-subscriber` for runtime configuration and output routing. The production-grade initialization lives in the dedicated `neo-telemetry` crate, which wires `tracing` to stdout/stderr, optional file sinks via `tracing-appender::non_blocking`, and a configurable log level filter (`EnvFilter`). A separate lightweight internal metrics subsystem lives in `neo-core/src/telemetry` (counters, gauges, histograms) that is independent of any external dependency — it is intended for embedding inside core components without pulling in the full telemetry stack.

## Key files and packages

- `neo-telemetry/src/lib.rs` — single entry points `init()` and `init_for_node()` that bootstrap both logging and Prometheus metrics; exposes `TelemetryHandle` holding metrics + system monitor.
- `neo-telemetry/src/logging.rs` — basic `init_logging(config)` that builds a `tracing_subscriber::registry` with an `EnvFilter` and one of text/compact/json layers.
- `neo-telemetry/src/node_logging.rs` — node-specific `init_node_logging(config, daemon_mode)` that composes stderr and an append-only file writer (via `tracing_appender::non_blocking`) using `BoxMakeWriter`; returns a `LoggingGuard` whose lifetime keeps async workers alive so logs are flushed on shutdown.
- `neo-telemetry/src/config.rs` — `LoggingConfig` (level, format, file path, console flag, color, include_target, include_location), `LogFormat` enum (`Text`, `Compact`, `Json`, `Pretty`), and `TelemetryAndLoggingConfig` combining logging + prometheus/health settings.
- `neo-core/src/telemetry/mod.rs` — internal `Telemetry` struct exposing `record_gauge`, `increment_counter`, `record_histogram`, `record_duration_ms`, plus domain helpers like `record_blockchain_metrics`, `record_timeout_stats`, `record_state_metrics`, `record_storage_metrics`. Uses `tracing` only for its own debug/info traces under target `telemetry`.
- `neo-core/src/application_logs/service.rs` — application-level execution log persistence (block/tx execution results stored in RocksDB under prefixed keys); errors are logged via `tracing::error!` with target `neo::application_logs` and can disable itself through `UnhandledExceptionPolicy`.

## Architecture and conventions

1. **Single initialization boundary.** All production nodes call `neo_telemetry::init_for_node(&config, daemon_mode)` from the node binary. This sets up the global `tracing` subscriber once at startup and returns a `LoggingGuard` that must be held for the process lifetime to ensure buffered file writers flush.
2. **Two-tier observability.**
   - *Deployment tier* (`neo-telemetry`): configures sinks (console, file), formats, and Prometheus HTTP server. Used by the node binary.
   - *Internal tier* (`neo-core::telemetry`): zero-dependency counters/gauges/histograms exposed to core modules for snapshot export or later forwarding to Prometheus. It does not configure any sink.
3. **Structured fields over formatted strings.** Call sites use `tracing::info!(field = value, "message")` style (e.g. `metrics_enabled = ...`, `daemon_mode = ...`, `block_height`, `header_height`, `mempool_size`, `peer_count`) rather than string interpolation for key dimensions. Errors are logged with an `error` field.
4. **Target-based filtering.** Node logging constructs a filter spec `"{level},neo={level}"` so the whole app and the `neo` crate hierarchy share the configured level. Internal `neo-core::telemetry` logs explicitly set `target: "telemetry"` to keep them separate.
5. **Daemon mode suppresses console.** When `daemon_mode` is true, `init_node_logging` routes to file-only (or `io::sink` when no file is configured), preventing interactive console noise in long-running processes.
6. **ApplicationLogs is a persisted log, not a tracing sink.** Block/transaction execution results are serialized to JSON and written into the blockchain store under fixed prefixes (`0x40` for blocks, `0x41` for transactions). They are queried via RPC and are distinct from tracing logs.
7. **Error handling policy.** ApplicationLogs wraps committing/committed handlers in `panic::catch_unwind`; panics or errors invoke `handle_panic` / `handle_error`, which log via `tracing::error!` and then apply the configured `UnhandledExceptionPolicy` (which may mark the service disabled).

## Conventions and constraints

- **Log levels**: default is `info`; callers use `trace`/`debug` for high-volume internals, `info` for lifecycle milestones, `warn` for recoverable anomalies, `error` for failures. The `include_location` flag controls whether file/line metadata is attached.
- **Output formats**: `Text` (default), `Compact`, `Json`, `Pretty` are selected via `LoggingConfig::format`. Production deployments typically pick `Json` for machine parsing.
- **File rotation**: the current implementation opens a single append-only file per process run (default name `neo-node-YYYY-MM-DD.log`); there is no built-in rotation — rotation is expected to be handled externally.
- **Console vs file**: `console` flag enables stderr output; combined with `daemon_mode` it determines whether stderr is included alongside the file writer.
- **Metrics naming**: internal metrics use a `neo_` prefix (e.g. `neo_block_height`, `neo_header_lag`, `neo_mempool_size`, `neo_peer_count`, `neo_p2p_timeouts_*`, `neo_state_*`, `neo_storage_*`).
- **Separation of concerns**: code inside `neo-core` should never depend on `neo-telemetry`; it uses the internal `neo-core::telemetry` module. Only the node binary (and plugins that opt in) initialize the full `tracing` subscriber via `neo-telemetry`.