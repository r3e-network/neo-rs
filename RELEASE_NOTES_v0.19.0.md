# Neo-RS v0.19.0 Release Notes

**Release date:** 2026-10-19  
**Protocol compatibility:** Neo N3 v3.10.1  
**MSRV:** Rust 1.88  
**License:** MIT

## Overview

v0.19.0 focuses on correctness evidence and production performance. It is the
first release with end-to-end MainNet execution replay, exact VM reference
counting, and real-socket multi-node dBFT tests.

This is a **testnet / private-network release candidate**, not a
mainnet-certified node. See [Known limitations](#known-limitations).

## Verification

| Check | Result |
|---|---|
| Workspace test suite | **4,517 passed, 0 failed**, 220 suites |
| Checked-in MainNet replay fixtures | **14 / 14** |
| Stored-chain validation | **1,352,011 blocks / 609,487 transactions**, 0 failures |
| Bulk C# state-root comparison | **1,460,685 roots**, 0 mismatches |
| Contiguous execution replay | **1,152,001 roots** (blocks 200,000–1,352,000), 0 mismatches |
| From-genesis execution replay | **99,877 blocks**, 0 mismatches |
| C# Neo.VM vectors | **26 / 26** |
| Formatting / production feature build | `cargo fmt --all --check`, `cargo check -p neo-node --features full` |

## Correctness changes

- Exact VM reference-counter accounting. `MaxStackSize` is enforced from the
  real counter with no slot-based fallback; compound children are added on the
  first stack reference and released on the last.
- MainNet replay harness fixes: Ledger storage is read from the chain store and
  `Ledger.currentIndex` is pinned to the historical height. These resolved the
  earlier block 238,711 and `IdoContract` divergences, which were harness
  artifacts rather than node bugs.
- dBFT wire-format, timestamp, transaction-availability, pending-commit and
  invalid-transaction handling fixes.
- Witness validation rejects malformed 33-byte public keys; `TRY`/`ENDTRY`
  state and bounded interpreter equality follow C# v3.10.1.
- Pre-Gorgon `HASKEY` semantics and native-contract syscall flags corrected.

## Performance

Measured on the local MainNet datasets:

| Benchmark | Result |
|---|---|
| Empty block persistence | **1.02 ms** |
| One RET transaction | **1.32 ms** |
| Ten RET transactions | **1.40 ms** |
| RocksDB empty block | **1.12 ms** |
| Heavy replay, blocks 1,346,000–1,352,000 | **286 s → 118.6 s (2.41x)** |
| Same range with `NEO_STATE_NODE_CACHE_ENTRIES=4194304` | **115.9 s** |

Main changes:

- `ContractManagement.getContract` no longer hydrates every contract record on
  each call (~4.5 ms → ~1.5 µs).
- `Storage.GetContext` reuses the engine contract cache instead of
  deserializing `ContractState`.
- The state backend reuses one RocksDB read snapshot and keeps a bounded node
  cache; capacity is configurable with `NEO_STATE_NODE_CACHE_ENTRIES`
  (default 262,144).
- `RET` reuses the cached execution-context script hash instead of re-hashing
  the contract script.

## Consensus networking

- Four-node dBFT tests over real TCP and UDP sockets commit block 1.
- The in-process dBFT harness covers multiple rounds and one offline validator.
- A primary-offline view-change test is not included.

## Known limitations

- No independent security audit has been completed; cargo audit has documented exceptions (h2 0.3 via hyper 0.14/warp, rustls 0.21 RPC TLS). See SECURITY-AUDIT-v0.19.0.md.
- The dBFT network tests use in-process services over real sockets; a
  multi-process `neo-node` deployment test is still outstanding.
- Execution replay covers blocks 200,000–1,352,000 plus a 99,877-block
  from-genesis run; it is not yet a single uninterrupted genesis-to-tip run.
- MainNet certification and public-network soak testing are not claimed.
- The Docker release workflow builds linux/amd64 only.

## Upgrade notes

- MainNet operators should keep the **rocksdb** storage backend and enable
  `[state_service]` with `full_state = true`; the memory backend is for
  development only.
- Optional cache tuning: `NEO_STATE_NODE_CACHE_ENTRIES` (default `262144`,
  higher values trade memory for fewer MPT reads).
- No protocol-level migration is required from v0.18.0. Review the public API
  diff before upgrading embedded users.

## Verification commands

```bash
cargo test --workspace --no-fail-fast
cargo check -p neo-node --features full
cargo build --release --locked -p neo-node
cargo audit
```
