---
kind: external_dependency
name: RocksDB persistent storage backend
slug: rocksdb
category: external_dependency
category_hints:
    - vendor_identity
    - client_constraint
scope:
    - '**'
---

### RocksDB
- Role: Default production key-value store for Neo N3 state (MPT trie, ledger, plugins). The shipped TOML configs select RocksDB; a plain `cargo build` without `--features full` produces a binary that cannot start with those configs.
- Integration: Declared as `rocksdb = "0.21"` with `snappy`, `lz4`, `zlib` compression features enabled in the workspace `Cargo.toml`; host must provide `librocksdb-dev`. The node exposes `--backend memory|rocksdb` and env `NEO_BACKEND` to switch away from it.
- Durable usage model: Data directory carries `NETWORK_MAGIC` and `VERSION` markers — only matching binaries/configs may open a given path. Production runs back up the RocksDB data directory via `scripts/backup-rocksdb.sh` and are expected to be mounted on durable volumes.
- Platform note: Performance work in this repo targets Windows-specific mmap read behavior of RocksDB (`allow_mmap_reads`) and pinning L0 filter/index blocks into cache; these are configuration-level concerns rather than code changes.