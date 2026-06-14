# Neo N3 benchmark results

> Fill this in from the script output. Delete the example numbers — they are
> placeholders, **not** measurements.

## Environment

| | |
|---|---|
| Date | YYYY-MM-DD |
| CPU | e.g. Apple M3 Max, 12 cores / AMD EPYC 7763 |
| RAM | e.g. 64 GB |
| Disk | e.g. NVMe SSD |
| OS | e.g. macOS 14.5 / Ubuntu 22.04 |
| Network | mainnet / testnet |
| Block range | genesis → 500,000 (same `.acc` for all) |

## Versions

| Impl | Version / commit | Storage engine | Build |
|------|------------------|----------------|-------|
| neo-rs | `<git sha>` | RocksDB | `--release` |
| neo-cli | `vX.Y.Z` | LevelDB | Release |
| neo-go | `vX.Y.Z` | LevelDB | default |

## 1. Block import throughput  (`sync-bench.sh`)

The headline metric — full consensus hot path, no network variance.

| Impl | Blocks | Wall time (s) | Blocks/s | Startup→ready (s) | Peak RSS (MB) |
|------|-------:|--------------:|---------:|------------------:|--------------:|
| neo-rs  | | | | | |
| neo-cli | | | | | |
| neo-go  | | | | | |

## 2. RPC throughput — `getblock` verbose  (`rpc-bench.sh --scenario block-read`)

Concurrency = 64, 30 s, warmup 3 s.

| Impl | req/s | p50 (ms) | p95 (ms) | p99 (ms) | errors |
|------|------:|---------:|---------:|---------:|-------:|
| neo-rs  | | | | | |
| neo-cli | | | | | |
| neo-go  | | | | | |

## 3. RPC overhead — `getblockcount`  (`rpc-bench.sh --scenario count`)

| Impl | req/s | p50 (ms) | p99 (ms) |
|------|------:|---------:|---------:|
| neo-rs  | | | |
| neo-cli | | | |
| neo-go  | | | |

## 4. Rust-internal micro-benchmarks  (`cargo bench`, neo-rs only)

Reproducible in this repo; no cross-impl equivalent.

| Bench | Result |
|-------|--------|
| header_serialize | |
| header_deserialize | |
| state_root (sha256/hash256/hash160) | |
| vm_add_loop_1000 | |

## Notes / anomalies

-
