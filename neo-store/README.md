# neo-store

`neo-store` implements a minimal storage abstraction for Neo N3 components. It
currently ships with an in-memory backend and typed column families so higher
layers can persist headers/blocks without committing to a specific database.

## Modules

- `traits`: `Store` trait plus `StoreExt` helpers that work with any type
  implementing `NeoEncode`/`NeoDecode`.
- `columns`: strongly typed columns (`Headers`, `Blocks`) and record types that
  leverage the derive macros for deterministic encoding.
- `memory`: concurrency-safe in-memory store with batching, snapshots, and
  fixtures for tests.

## Fixtures

JSON fixtures in [`neo-store/fixtures`](fixtures) demonstrate how to hydrate
`HeaderRecord` and `BlockRecord` via serde:

```rust
let record: HeaderRecord = serde_json::from_str(include_str!("fixtures/header.json"))?;
```

## Testing

```bash
cargo test --manifest-path neo-store/Cargo.toml
```

The tests cover typed helpers, batches, snapshots, and fixture round-tripping.
