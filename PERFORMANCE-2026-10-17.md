# Performance Notes — 2026-10-17

Measured on the Windows development host with Criterion 0.5, `bench` profile
(release, `lto = "fat"`, `codegen-units = 1`), 20 samples, 0.2 s warm-up and
1 s measurement. Numbers are medians from the criterion output; treat them as
microbenchmarks, not full-node throughput.

## RocksDB production persistence

Empty block through `NeoSystem::persist_block` with the durable RocksDB provider
(`--features rocksdb`):

| Benchmark | Median |
|---|---:|
| `rocksdb_persist_empty_block` | **1.12 ms** |

This is the actual production storage path (WAL + write batch + compaction
bookkeeping), not the in-memory provider. It is slightly faster than the
1.02 ms memory-store result because the memory store's copy-on-write snapshots
and BTreeMap scans are avoided; RocksDB uses native range scans and a durable
write batch.

Command:

```text
cargo bench -p neo-benches --features rocksdb --bench rocksdb_persistence -- \
  --warm-up-time 0.5 --measurement-time 2 --sample-size 20
```

## End-to-end empty-block persistence

`NeoSystem::persist_block` on a fresh in-memory testnet system, with real
native `OnPersist`, ledger updates, write cache, and commit, but no
transactions (`benches-package/benches/block_persistence.rs`):

| Benchmark | Median |
|---|---:|
| `persist_empty_block` | **1.02 ms** (range 0.83–1.16 ms) |
| `persist_block_one_ret_tx` | **1.32 ms** (range 1.07–1.51 ms) |
| `persist_block_ten_ret_txs` | **1.40 ms** (range 1.22–1.54 ms) |

The one-transaction case adds ~0.30 ms for engine setup, transaction execution,
ledger state, and cache merge. Ten transactions do **not** increase the median,
confirming the per-transaction VM-state lookup is now O(1) instead of scanning
the whole block state list. Before the lookup fix, each transaction scanned all
N block states (`mark_vm_state`), giving O(N²) behavior for large blocks.

## Memory-store snapshot/prefix scan (fixed)

`MemoryStore::snapshot` previously cloned the entire `BTreeMap`, and
`MemoryStore::find` / `MemorySnapshot::find` scanned and cloned every entry
before filtering by prefix. That made every block O(chain length) for the
default in-memory provider.

Changes:
- snapshots share an immutable `Arc<BTreeMap>` (copy-on-write on store writes),
- prefix scans use the correct `BTreeMap` range,
- snapshots overlay only matching write-batch keys instead of cloning the map.

Result: empty-block persistence is **1.02 ms** instead of growing to ~4.0 ms;
one-RET-tx persistence is **1.32 ms** instead of ~5.2 ms in the same
long-running criterion setup. Added tests cover prefix ordering and snapshot
isolation from later store writes.

## Per-transaction VM-state lookup (fixed)

`LedgerTransactionStates::mark_vm_state` used to scan every transaction state
in the block for each transaction, so a block with N transactions performed
O(N²) hash lookups. It now builds a hash → index map lazily once per block.

Measured with the end-to-end benchmark: one `RET` transaction = **1.32 ms**,
ten `RET` transactions = **1.40 ms**. The per-transaction marginal cost no
longer grows with block transaction count. Existing ledger tests
(`batch_vm_state_update_applies_all_entries`, persist/update tests) still pass.

Command:

```text
cargo bench -p neo-benches --bench block_persistence -- \
  --warm-up-time 0.5 --measurement-time 2 --sample-size 10
```

## Native-contract seeding (fixed)

Block persistence creates one `ApplicationEngine` per transaction and seeds it
with the native contracts materialized by the `OnPersist` engine. Previously
each transaction deep-cloned every `ContractState` (NEF script + manifest).

| Path | Median |
|---|---:|
| `native_contracts_deep_clone` (old) | **13.27 µs** |
| `native_contracts_arc_clone` (new) | **105.78 ns** |

The per-transaction map now stores `Arc<ContractState>`; cloning the map only
bumps reference counts. For a 1,000-transaction block this removes roughly
**13.17 ms** of pure memcpy/allocation work from the persistence loop
(1,000 × (13.27 µs − 0.106 µs)), before accounting for allocator pressure.

Command:

```text
cargo bench -p neo-benches --bench native_contracts -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

Implementation: `neo-core/src/smart_contract/application_engine/state.rs`
(`contracts_shared_snapshot`, `new_with_preloaded_native_shared`) and
`neo-core/src/neo_system/persistence.rs`.

## P2P frame encode/decode

500-transaction block message, LZ4 compression enabled
(`benches-package/benches/p2p_frames.rs`):

| Benchmark | Median |
|---|---:|
| `p2p_block_encode_500tx_compressed` | 28.30 µs |
| `p2p_message_deserialize_500tx_compressed` (framing + LZ4) | 1.98 µs |
| `p2p_protocol_decode_500tx` | 402.04 µs |
| `p2p_block_decode_500tx_compressed` (full) | 404.73 µs |

Decoding is dominated by protocol decode (~99.5%): per-transaction
deserialization/validation plus transaction hashing and merkle-root
reconstruction. Framing/decompression itself is only ~2 µs. This is inherent
to the current validation-on-decode design; skipping it would weaken the
duplicate-tx/merkle checks that match C#.

Command:

```text
cargo bench -p neo-benches --bench p2p_frames -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

## Signature verification

secp256r1 ECDSA is the dominant per-witness cost:

| Benchmark | Median |
|---|---:|
| `crypto_secp256r1_verify/32` | 141.26 µs |

A transaction with two standard witnesses therefore pays ~280 µs before VM
execution. This is in the same range as a 500-tx block protocol decode
(402 µs), so witness verification dominates multi-signature workloads.
Parallelizing independent witness checks is possible with `rayon`, but it
changes the execution ordering and needs a consensus/correctness proof plus a
C#-differential check before landing.

Command:

```text
cargo bench -p neo-crypto --bench crypto_ops -- crypto_secp256r1_verify -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

## MPT / state root

The state root is a Merkle-Patricia Trie over storage changes. Measured with an
in-memory snapshot (`benches-package/benches/mpt_state_root.rs`):

| Benchmark | Median |
|---|---:|
| `mpt_build_1000` (put 1,000 keys + commit) | 11.61 ms |
| `mpt_build_10000` (put 10,000 keys + commit) | 154.33 ms |
| `mpt_get_1000` | 99.09 µs |
| `mpt_get_10000` | 1.16 ms |

A 1,000-storage-change block pays roughly **11.6 ms** for the trie build/hash
step; a 10,000-change block pays ~154 ms, still below the 15 s block interval
but now the largest measured per-block cost in the state path. The cost is
dominated by re-hashing nodes along each updated path (up to 64 nibble levels
for 32-byte keys). Lazy/root-only hashing is the next structural optimization
candidate; it needs a careful C#-parity proof because node hashes are part of
the state root.

Command:

```text
cargo bench -p neo-benches --bench mpt_state_root -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

## VM / interpreter baseline

| Benchmark | Median | Notes |
|---|---:|---|
| `vm_add_loop_1000` | 98.04 µs | 4,001 instructions (`PUSH1/PUSH1/ADD/DROP` × 1,000 + `RET`) ≈ 24.5 ns/instruction |
| `script_parse/1000_bytes` | 11.31 µs | NOP fill, strict validation |
| `script_parse/10000_bytes` | 142.85 µs | NOP fill, strict validation |
| `stack_peek_top_10000` | 935.5 ns | 10,000 `last()` calls |
| `script_builder_1000_opcodes` | 1.010 µs | 2,000 emitted opcodes |

## Native registry construction (fixed)

`NativeRegistry::new` used to construct all 11 standard contracts (manifests,
method tables, scripts) for every transaction engine. The 10 stateless
contracts are now brokered from a process-wide `OnceLock`; only the stateful
`ContractManagement` is constructed per engine.

| Benchmark | Before | After |
|---|---:|---:|
| `native_registry_new` | 13.30 µs | **1.45 µs** |
| `application_engine_new` | 68.13 µs | **57.19 µs** |
| `application_engine_new_with_preloaded_native` | 46.31 µs | **29.59 µs** |

The preloaded number still includes a deep clone of the benchmark's
`ContractState` map (~13.27 µs). The production path uses the shared `Arc`
snapshot, so its per-engine cost is approximately **16.3 µs**; a 1,000-tx block
spends ~16.3 ms on engine setup instead of ~33 ms.

## Remaining per-engine cost

The remaining ~16 µs/engine is dominated by interop handler registration
(string hashing + descriptor allocation for ~50 syscalls), policy refresh, and
the fresh stateful `ContractManagement`. Measured in isolation,
`interop_service_register_50` is **4.19 µs**, so descriptor registration is a
real but not dominant slice. Caching pre-hashed interop descriptors remains a
small next step; the rest is policy/native setup.

Command:

```text
cargo bench -p neo-benches --bench native_contracts -- application_engine_new -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

Command:

```text
cargo bench -p neo-benches --bench vm_execution -- \
  --warm-up-time 0.2 --measurement-time 1 --sample-size 20
```

## What is not measured yet

The 14 MainNet replay tests now validate real transaction execution
(witness + VM + native contracts + state root), but they are correctness
fixtures, not a throughput benchmark. Do not extrapolate the empty-block or
microbenchmarks into blocks-per-second claims.

## Contiguous stored-chain integrity

`mainnet_chain_integrity` loaded every block from the local MainNet RocksDB
through height 1,352,010 and re-computed the block hash, previous-hash link,
and Merkle root:

```text
stored_chain_ok heights=0..=1352010 blocks=1352011 transactions=609487
```

It completed in **27.3 s** with no failures.

## Bulk MainNet state-root differential


`mainnet_state_roots_vs_csharp` compared **1,460,685** stored MainNet state
roots through height 1,465,790 against the C# reference JSONL:

```text
max_height_cap=1465790 compared=1460685 skipped_no_local=5106
skipped_above_cap=11684450 skipped_explicit=0 mismatches=0
```

This is correctness evidence, not a throughput benchmark; it took 10.92 s on
the local RocksDB backup because it reads the stored roots rather than
re-executing every block.

## From-genesis execution replay

`mainnet_contiguous_replay` executes every transaction plus
OnPersist/PostPersist for a contiguous genesis range against the previous
state trie and compares the resulting root with the C# reference. Ledger
contract storage is excluded from the StateService trie, so Ledger keys are
read from the chain store.

```text
blocks 1..=99877: 0 state-root mismatches
blocks 200000..=1352000: 1,152,001 roots checked, 0 gaps, 0 mismatches
chunked release replay: 10,000 light blocks in ~65 s; heavy range 1.35M in 155 s
harness overrides: Ledger storage from chain store; historical Ledger.currentIndex
```

The block 238,711 mismatch reported earlier was caused by the replay harness
returning Null for `Ledger.getBlock`; routing Ledger storage to the chain store
fixed it. Blocks 1..=1000 replay in ~5 s; 1..=10,000 in ~26 s.

## Native contract lookup

Every `ContractManagement.getContract` call used to hydrate all contract
records from storage before answering: **~4.5 ms per call**. The heavy
1,346,000..=1,352,000 range was dominated by this. `getContract` and
`getContractById` now read the contract key directly from the snapshot.

```text
heavy range 1,346,000..=1,352,000 (6,001 blocks)
before: 286 s
after:  118.6 s
speedup: 2.41x, identical roots
```

A second fix in the same range removed full contract-state deserialization from
ApplicationEngine::get_storage_context; it now reuses the engine contract cache
(175.4 s -> 158.5 s on the same 6,001 blocks). Reusing the state backend read snapshot instead of creating a new RocksDB snapshot per MPT node read gives a further 158.5 s -> 152.2 s. A bounded 262,144-entry state node cache brings the same range to 122.4 s. Avoiding a full script hash on every RET (the cached context hash is already available) gives 118.6 s. The state-node cache capacity is configurable through NEO_STATE_NODE_CACHE_ENTRIES (default 262,144); 4,194,304 entries gives 115.9 s.

The replay harness also enables DataCache read caching only for blocks with
more than two transactions, avoiding the per-block cache overhead on empty and
light blocks.

Further experiments that did **not** help this range (kept out of the tree):
raising the node cache to 2,097,152 entries (121.0 s vs 122.4 s, within noise),
and a replay-level flat storage-value cache (121.5 s). The remaining time is
cold MPT reads with low locality plus custom-contract VM execution.

## Real MainNet block replay

All 14 checked-in `mainnet_block_*_repro` tests pass against the local
state-root DB (`data/Plugins/StateService/StateRoot`), including C# state-root
assertions and exact C# gas for the sampled transactions. Block 1,268,131 now
HALTed with **40,436,620** gas, matching the C# applog; block 1,074,782's three
transactions HALTed with **9,687,851**, **6,704,385**, and **1,820,391,692** gas.
This replaces the previous "no replay benchmark" caveat: real transactions now
execute end-to-end through witness/VM/native/state-root paths, although this is
correctness evidence rather than a throughput benchmark.

Known remaining costs in the persist path:

1. `ApplicationEngine` construction and interop registration per transaction.
2. `load_script(tx.script().to_vec())` copies each transaction script.
3. Signature verification (secp256r1) per witness.
4. RocksDB write batch + WAL commit.
5. MPT/state-root maintenance.

## Reproduction / CI

The benchmarks are registered in `benches-package/Cargo.toml`
(`native_contracts`, `mpt_state_root`, `p2p_frames`, `block_persistence`,
`rocksdb_persistence`) plus `neo-core` (`serialization`, `storage`),
`neo-io`, `neo-crypto` and `neo-tests`.

`cargo bench --workspace` is **not** usable here. It also runs every crate's
lib/binary test harness and every auto-discovered `benches/*.rs` that uses the
default libtest harness; none of those measure anything, but they parse argv and
abort with `error: Unrecognized option: 'save-baseline'`, which makes the whole
invocation exit non-zero. Any additional criterion flag has the same effect.
`scripts/bench-regression.sh` therefore selects targets from the manifests
(`[[bench]]` with `harness = false`) and runs `-p <package> --bench <name>`
pairs.

The blocking CI gate is `./scripts/bench-regression.sh --gate`. It measures a
fixed subset (`mpt_state_root`, `block_persistence`, `native_contracts`) twice
in the same job — once to record a baseline, once to compare against it — and
fails on a >10% regression. The baseline is deliberately **not** cached across
runs: criterion reports absolute wall times, so comparing a 2-core runner
against a baseline produced on a different host measures machine variance, not
code. The remaining targets are still available locally through
`--save-baseline` / `--compare`, but are advisory because their run-to-run
spread on an unchanged tree is larger than the threshold.

MainNet replay and bulk-root checks are ignored by default because they need
the local chain/state backups. CI runs them in
`.github/workflows/mainnet-validation.yml` on a self-hosted runner labelled
`neo-mainnet-data`.

```text
NEO_REPRO_DB_PATH=data/Plugins/StateService/StateRoot \
  cargo test -p neo-core --features rocksdb --test mainnet_block_1268131_repro -- --ignored

NEO_BULK_ROOT_DB_PATH=data/Plugins/StateService/StateRoot \
NEO_BULK_ROOT_MAX_HEIGHT=1465790 \
  cargo test -p neo-core --features rocksdb --test mainnet_state_roots_vs_csharp -- --ignored

NEO_CHAIN_DB_PATH=data/mainnet-protocol-consistency \
  cargo test -p neo-core --features rocksdb --test mainnet_chain_integrity -- --ignored
```
