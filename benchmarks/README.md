# Neo N3 cross-implementation benchmarks

A reproducible harness for comparing the performance of the three production
Neo N3 node implementations on **identical workloads, identical hardware**:

| Implementation | Language | Binary used here |
|----------------|----------|------------------|
| **neo-rs** (this repo) | Rust | `neo-node` |
| **neo-cli** | C# / .NET | `neo-cli` |
| **neo-go** | Go | `neo-go` |

> **Honesty note.** This directory ships the *harness*, not a table of numbers.
> A credible "Rust is faster than C#/Go" claim requires running all three nodes
> on the *same* machine — fabricated or hand-waved figures are worse than none.
> Run the harness yourself (or in CI) and the scripts emit the comparison table.
> The only pre-measured figures we publish are the Rust-internal Criterion
> micro-benchmarks under [`../benches-package`](../benches-package), which are
> reproducible with `cargo bench` in this repo.

## What gets measured

All three nodes speak the same JSON-RPC and consume the same block-export
(`.acc`) format, so every workload below is genuinely apples-to-apples.

| Workload | Metric | Why it matters | Driver |
|----------|--------|----------------|--------|
| **Block import** | blocks/sec, wall-time | Raw block validation + VM execution + state-write throughput. The headline node metric. | `scripts/sync-bench.sh` (offline `.acc` import) |
| **RPC throughput** | req/sec, p50/p95/p99 latency | Read-serving capacity under concurrency. | `bench-client` + `scripts/rpc-bench.sh` |
| **RPC `getblock` (verbose)** | req/sec, latency | Block read + full JSON serialization cost. | `bench-client --scenario block-read` |
| **`getblockcount`** | req/sec | Pure RPC/dispatch overhead (no I/O). | `bench-client --scenario count` |
| **Resource footprint** | peak RSS, avg CPU% | Memory/CPU cost of the same work. | `scripts/resource-sample.sh` |
| **Startup → ready** | seconds to first RPC 200 | Cold-start cost. | `scripts/sync-bench.sh` (reports it) |

Block import is the most meaningful cross-implementation comparison: it
exercises deserialization, signature verification, the VM, native contracts,
and the state/MPT layer — i.e. the whole consensus-critical hot path — with
zero network variance.

## Layout

```
benchmarks/
├── README.md              # this file — methodology + how to run
├── bench-client/          # standalone Rust JSON-RPC load generator (own workspace)
│   └── src/main.rs         #   build: cargo build --release  (from this dir)
├── scripts/
│   ├── rpc-bench.sh        # run bench-client across N endpoints, print a table
│   ├── sync-bench.sh       # time an offline .acc block import for one node
│   ├── resource-sample.sh  # sample RSS/CPU of a pid for a duration
│   └── lib.sh              # shared helpers
└── results/
    └── TEMPLATE.md         # fill-in comparison table
```

## Prerequisites

1. **neo-rs** — build this repo: `cargo build --release -p neo-node`
   (binary at `target/release/neo-node`).
2. **neo-cli** — download a release from
   <https://github.com/neo-project/neo-node/releases> (or `neo-cli` from
   <https://github.com/neo-project/neo/releases> for current versions) and
   ensure `dotnet` is installed.
3. **neo-go** — download from <https://github.com/nspcc-dev/neo-go/releases>
   or `go install`.
4. A block-export file (`chain.acc` / `chain.0.acc`) covering the height range
   you want to import. Generate one from a synced node, or use
   `../scripts/build-acc-from-rpc.py`. **Use the same `.acc` for all three.**

Run everything on the **same machine, same storage, no other load**, and pin
each node to the same data directory on the same disk.

## Quick start

```bash
# 1. Build the load generator (standalone — its own workspace)
cd benchmarks/bench-client && cargo build --release && cd -

# 2. Block-import throughput (run once per implementation, fresh data dir each)
benchmarks/scripts/sync-bench.sh \
    --name neo-rs \
    --cmd "target/release/neo-node --config config/mainnet.toml --import chain.acc" \
    --rpc http://127.0.0.1:10332 \
    --target-height 500000

benchmarks/scripts/sync-bench.sh --name neo-cli --cmd "..." --rpc ... --target-height 500000
benchmarks/scripts/sync-bench.sh --name neo-go  --cmd "..." --rpc ... --target-height 500000

# 3. RPC throughput across all three (nodes already running + synced)
benchmarks/scripts/rpc-bench.sh \
    --scenario block-read --max-height 500000 --duration 30 --concurrency 64 \
    neo-rs=http://127.0.0.1:10332 \
    neo-cli=http://127.0.0.1:20332 \
    neo-go=http://127.0.0.1:30332
```

Each script prints a table and appends a row to `results/`. Copy the numbers
into `results/TEMPLATE.md`.

## Fairness checklist

- [ ] Same hardware, same disk, nothing else running (close editors/CI).
- [ ] Same `.acc` file and same `--target-height` for the import benchmark.
- [ ] Same protocol config (mainnet vs testnet) on all three.
- [ ] Storage engine noted (neo-rs: RocksDB; neo-cli: LevelDB/RocksDB plugin;
      neo-go: LevelDB/BoltDB) — storage choice affects import speed, so record it.
- [ ] Release/optimized builds only (`--release`; .NET `Release`; Go default).
- [ ] Warm up RPC benchmarks (the client does 3s by default) before measuring.
- [ ] Run each measurement ≥3 times; report median, not best-of.
- [ ] Record CPU model, core count, RAM, disk type in the results file.

## Interpreting results

- **Block import** is the fairest single number — it has no network and no
  client variance. Report it as blocks/sec and as total wall-time for the range.
- **RPC throughput** depends heavily on the HTTP stack and JSON serializer, not
  just consensus logic; treat it as a serving-capacity metric, not a "core speed"
  metric.
- A single slow path can dominate; if one implementation looks anomalous, sample
  with `resource-sample.sh` (CPU-bound vs I/O-bound tells you where the time goes).
