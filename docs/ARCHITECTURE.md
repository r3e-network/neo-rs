# neo-rs Architecture

This document captures the current layering and service boundaries so new changes stay coherent.

## Crates and responsibilities

- **neo-core**: consensus-neutral protocol logic (ledger, state service, VM host integration, persistence adapters). Keep public APIs small; prefer `pub(crate)` where possible.
- **neo-vm / neo-io / neo-extensions**: execution engine, serialization, and utility primitives. These should remain dependency-free on higher layers.
- **neo-plugins**: node-side extensions (RPC server, RocksDB storage). Interact with core through typed handles (e.g., `StoreCache`, `StateStore`); avoid direct downcasts.
- **neo-rpc-client**: client-side RPC bindings with typed models and helpers. Keep it UI-agnostic so both CLI and external callers can reuse it.
- **neo-cli**: thin command wrappers over `neo-rpc-client`, no business logic.
- **neo-node**: daemon composition (config, wiring actors, plugin loading). Owns service registration and lifecycle; avoids protocol logic.
- **neo-tee**: enclave-facing utilities and optional mempool/wallet. Feature-gated; avoid leaking into core.

## Services and context

- Core service access is via `NeoSystemContext`; prefer typed accessors (e.g., `state_store()`) instead of `Any` downcasts. When introducing a new service, provide:
  - A trait for the required behaviour.
  - A typed accessor on `NeoSystemContext`/`NeoSystem`.
  - A readiness/health hook if it has external dependencies.
- State flow: blocks persist via `LedgerContext` → `StateStore` updates local root → state-root extensible payloads verify/persist via shared `StateStore` (with `StateRootVerifier` backed by `StoreCache`).

## Error handling

- Use `thiserror` enums per crate and convert at boundaries (e.g., RPC -> JSON-RPC codes, actor -> `CoreError`). Avoid stringly-typed errors.
- Prefer domain newtypes (`BlockHeight`, `TimestampMs`, `Gas`) to reduce unit mixups when adding new APIs.

## Concurrency and safety

- Locks: `parking_lot` locks preferred; document lock order when multiple locks are taken. Avoid holding locks across async/await.
- IO/persistence: any blocking store calls from async paths should be pushed to `spawn_blocking` or actor threads.
- Avoid global mutable singletons; pass handles explicitly.

## Testing strategy

- Unit tests for serialization, VM, state store (proofs, validation), and contract/native logic remain in `neo-core`.
- Integration tests should exercise service registration and RPC surfaces (e.g., state-service endpoints) with in-process components where possible.
- Golden compatibility tests (C# parity) must not be broken without updating fixtures.

## Observability

- Use `tracing` with clear targets (`neo`, `rpc`, `state`, `tee`). Wrap significant operations (block import, state-root verification, RPC handlers) in spans.
- Metrics: expose counters/gauges for mempool size, state-root ingest success/failure, RPC latency; keep metric names stable for dashboards.

## Configuration

- Keep configs serde-driven with explicit defaults and validation. Avoid silent fallbacks for protocol-critical values (network magic, validator counts, storage paths).
- For new components, add a config struct, validation, and a TOML example snippet under `docs/`.

