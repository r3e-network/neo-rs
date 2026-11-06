# Performance Baselines

## Established: 2026-03-23

### Block Processing

- Deserialization: TBD (run `cargo bench` to establish)
- Validation: TBD

### State Root Calculation

- 1000 entries: TBD

### VM Execution

- Simple ADD operation: TBD

## How to Update Baselines

1. Run benchmarks:

    ```bash
    cargo bench
    ```

2. Record results in this file

3. CI will alert on >10% regression

## Toolchain and target-cpu

Measured 2026-10-29 on a 24-thread Windows workstation. `neo-benches` suites:
hashing, vm_execution, p2p_frames, payload_serialization, native_contracts,
mpt_state_root. Bench profile is `lto = "fat"`, `codegen-units = 1`.

Method: prebuilt bench binaries were run interleaved (A, B, C, … then reversed)
for three rounds, and the per-benchmark median was taken. The baseline is Rust
1.95.0 `x86_64-pc-windows-gnullvm`. A negative percent is faster.

| Configuration | Host | Geomean vs 1.95 | Median vs 1.95 |
|---|---|---|---|
| Rust 1.99.1 | gnullvm | −2.7% | −0.5% |
| Rust 1.99.1 + `-C target-cpu=x86-64-v3` | gnullvm | +0.8% | +1.6% |
| Rust 1.99.1 | msvc | −2.9% | −0.3% |
| Rust 1.99.1, PGO (`scripts/pgo-build.sh`) | msvc | −4.5% | −1.4% |

Same-host changes from 1.95 to 1.99.1 on gnullvm, largest first:

| Benchmark | 1.95 | 1.99.1 |
|---|---|---|
| `stack_push_pop_1000` | 12.2 µs | −52% |
| `vm_nop_5000` | 3.72 µs | −12% |
| `p2p_message_deserialize_500tx_compressed` | 2.60 µs | −9% |
| `vm_add_loop_1000` | 97.5 µs | −8% |
| `sha256/32_bytes` | 34.9 ns | −7% |
| `p2p_block_encode_500tx_compressed` | 27.6 µs | +8% |
| `hash256/1024_bytes` | 262 ns | +5% |

Conclusions:

* Upgrading 1.95 to 1.99.1 on the same host is a small win overall. The VM
  stack path is much faster. The only clear slowdown in this set is compressed
  block encoding.
* `-C target-cpu=x86-64-v3` is slower in this set (geomean +0.8%) and the
  binary will not start on CPUs without AVX2/BMI2. It stays opt-in only.
* PGO on the MSVC host is the fastest of the four. About 1.6 points of the
  −4.5% come from the profile, and the profile was collected by running these
  same benches, so that extra is an in-sample upper bound. A mainnet replay
  training run is the next measurement (`PGO_TRAIN_CMD`).
* Instrumented binaries crash during training on `x86_64-pc-windows-gnullvm`
  (Rust 1.99.1, `hashing` suite, exit 139). The PGO script is for the MSVC host
  on Windows, and for Linux.

## State-root commit path (2026-10-30)

Block import spends its time in two places after execution: hashing every dirty
MPT node, then writing those nodes through the RocksDB snapshot.

The hash of a node is SHA-256 of its body without the reference count. The
bytes stored in RocksDB are that same body plus the reference count. `try_hash`
keeps the body. `MptCache::commit` appends the current reference instead of
walking the node a second time. If the body was not kept, commit serializes
from scratch, so a node loaded from the store still writes the same bytes.

`RocksDbSnapshot` keeps uncommitted writes in a `HashMap` for point reads.
Range scans still merge those writes into a sorted map before returning, so
prefix iteration order does not change. The write batch is built from that map
once, at commit. A key written more than once in the same snapshot keeps only
its latest value.

## Execution path (2026-10-30)

Contract execution reads the same storage item many times. `StorageItem` keeps
its bytes in an `Arc`, so a cache hit increments a reference count. Replacing
the value installs a new `Arc`; an earlier clone keeps the previous bytes.

Opcode fees used to lock the current execution context on every instruction to
read a whitelist flag. The engine reads that flag once. Loading or unloading a
context, and assigning the whitelist flag, drops the cached value. Later
instructions in the same context only test the cached flag. The fixed whitelist
fee is charged before the flag is set, so that charge is not skipped.

Contract scripts are parsed once in `Script::new`. Each later instruction is
an index into that table, shared by clones of the script. Scripts that skip
validation still fill a locked map on first use. `System.Storage.Get` moves
the key into the lookup. The storage trace switches are read once per process.

## Trie node cache (2026-10-30)

A trie lookup that hits `GlobalNodeCache` takes a shared lock and returns the
cached `Arc`. The store key is built only after a miss. The per-block cache
keeps that `Arc`. The trie receives one detached copy. Changing a reference
count copies the node first, so the cached node stays as it was stored. One
hit in sixteen also moves the node to the front of the eviction list.

## Block sync (2026-10-30)

A peer can deliver blocks faster than they are persisted. The node keeps at
most 256 unverified blocks from that peer. The in-flight index window is one
1,000-block batch, matching that budget: a 10,000-block window filled the
256-slot buffer, marked every dropped height complete, and immediately asked
for it again. Duplicate deliveries were then ignored, so those tasks stayed
in flight until the 60s timeout. The live node dropped the same heights
millions of times and persistence only moved in short bursts.

When the per-peer buffer is full of blocks above the next height, the task
manager drops the highest of those blocks and asks for the next height again.
Leaving that buffer full and refusing every new request stalls persistence
until the process is restarted. A block closer to the persist frontier still
displaces the highest retained index when it arrives. A block that is already persisted, or already
forwarded, still completes the in-flight index task so the window can advance.
The drop is counted every time and written to the log once per 1024 drops.

## Target Improvements

- Block processing: 30-50% faster
- Memory usage: 20-30% reduction
- Latency: <100ms per block
