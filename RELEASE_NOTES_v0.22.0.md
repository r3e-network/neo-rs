# Neo-RS v0.22.0 Release Notes

**Release date:** 2026-11-22
**Protocol compatibility:** Neo N3 v3.11.2
**MSRV:** Rust 1.99
**License:** MIT

v0.22.0 ports the behavioural delta between the C# Neo N3 v3.10.2 baseline and
the official v3.11.2 release, and modularizes every oversized source file in
the workspace. `docs/V3_11_2_ALIGNMENT.md` is the complete map of what was
ported, where it lives, and which Rust-side divergences are deliberate.

## Protocol

**Hardforks.** `HF_Iara` and `HF_Jackalope` join `HF_Huyao` in
`Hardfork`; `Huyao` is the last configuration-managed hardfork, matching the
C# `Hardfork` static class. Parsing, `FromStr` forms, and the
`UnknownHardforkException` fault path are ported; the `Policy` activations
(`activateHardfork`, `getLastConfigManagedHardfork`, `tempStorageMaxTTL`) feed
the engine's `is_hardfork_enabled`.

**TemporaryStorage.** The v3.11.2 native contract is implemented with
`get_contract_id` and TTL enforcement through the Policy setting.

**Huyao dynamic pricing.** The VM collects `RunStats` per instruction and
selects pre/post pricing hooks by hardfork. The contract layer adds
`StaticCoefficients`, `DynamicPriceTable`, and `OpcodeV1` pricing with femtoGAS
accounting (`AddFemtoGas`, `GasLeft`, `FeeConsumed`). The 25 fixed syscalls
keep static prices; variable fees cover CheckWitness rule counts, LoadScript
bytes, Notify items, GetNotifications, CurrentSigners references,
Iterator.Value, CreateMultisig per key, Contract.Call/CALLT disk fees, and
CheckMultisig/CheckSig. The NEP17 fast path keeps Huyao fee parity, and the
pre-Huyao jump table retains the legacy `SETITEM_Before606` / `Roll_Before575`
handlers.

**Huyao behaviour fixes.** `StdLib.strLen` counts codepoints under Huyao; the
`FindWithStart` syscalls get forward/backward seek parity with a local-store
variant; `CryptoLib.recoverSecp256K1` splits V0 (BC-wrap math) from V1 (R/S
range check); `bls12381_deserialize` gains the Iara identity gate.

**Non-hardfork ports.** `MerkleTree.Trim` depth-vs-flags fix; witness cost
helpers with the `cap_gas` parameter in `VerifyWitnesses`; `MaxTransactionSize`
enforced in `Transaction` deserialization; `Header.Verify` applies the 3 GAS
budget verbatim (`capGas: false`); `DataCache`/`Find` skip parity;
`StdLib.jsonSerialize` reject-cycles message; `Notary` persist guards and the
Conflicts else-branch; `NeoToken` vote early-return.

**P2P and mempool.** `MemoryPool.checkConflicts` matches any common signer;
stale conflict entries are evicted from both verified and unverified pools on
block persist; `MerkleBlockPayload` counts `TxCount.GetVarSize()` and bounds
the hash count by `RoundUpToPowerOf2(TxCount)`; the task manager fully
releases global slots on completion, counts timed-out index tasks per session
and re-requests in ascending order; `RequestTasks` deduplicates in-flight
hashes; `Peer.IsIntranetAddress` implements the v3.11.2 rules; out-of-window
blocks still notify the task manager.

Deliberate Rust divergences (stronger DoS budgets, bounded outbound queues)
are documented in the alignment doc's "Deliberate Rust divergences" section.

## Maintenance

Every production Rust file above 1000 lines (37 files) was split into
directory modules: a small parent keeps the module surface, cohesive children
carry the implementations, public paths are preserved via re-exports, and
inline `mod tests` moved to sibling test files. The largest non-test parent is
659 lines. The interrupted rocksdb store modularization in `neo-storage` was
completed (`RocksDbSnapshot` re-export and field visibility), and the
architecture guard suite was updated for the new module layout.

The workspace version moved 0.21.0 → 0.22.0 (root, fuzz, benches-package, and
tests manifests).

## Verification for this release commit

```text
cargo fmt --all -- --check:              clean
cargo check --workspace --all-targets:   0 errors, 0 warnings
cargo test --workspace:                  5430 passed, 0 failed (199 blocks, incl. doctests)
architecture guard suite:                green
```

## Known limitations

This remains a testnet and private-network release, not a mainnet-certified
node. A single uninterrupted genesis-to-tip state-root comparison is still
open. See `RELEASE_NOTES_v0.20.0.md` and `docs/PROTOCOL_CONSISTENCY.md`.
