# C# v3.11.2 Protocol Alignment

neo-rs 0.22.0 ports the behavioural delta between the C# Neo N3 **v3.10.2**
baseline and the official **v3.11.2** release. This document is the map of what
changed, where it lives in the Rust tree, and which Rust-side divergences are
deliberate. The state-root verification methodology referenced below is
unchanged; see `PROTOCOL_CONSISTENCY.md`.

## Hardforks

`neo-primitives/src/hardfork.rs`:

- `HF_Huyao` (7), `HF_Iara` (8), `HF_Jackalope` (9) — Iara and Jackalope were
  added in this alignment; their on-chain activation heights are not yet
  configured, matching C# v3.11.2.
- `Hardfork::LAST_CONFIG_MANAGED = HF_Huyao` and the `Hardforks` helpers
  (`from_removed_prefix` parsing rules, `FromStr` accepting `HF_HUYAO`/`HUYAO`
  forms) mirror the C# `Hardfork` static class.
- `UnknownHardforkException` is ported as a typed `CoreError`; the application
  engine records it as a fault flag, ledger persistence halts with
  `LedgerError`, and the poison-marker path matches C# abort semantics.
- `Policy.activateHardfork` / `Policy.getLastConfigManagedHardfork` /
  `Policy.tempStorageMaxTTL` (prefix 24) and the
  `LastConfigManagedHardfork` protocol-setting are wired; on-chain hardfork
  heights feed the engine's `is_hardfork_enabled`.

## TemporaryStorage native contract

`neo-native-contracts/src/native/temporary_storage.rs` — the v3.11.2 native
contract (`TemporaryStorage`, with `get_contract_id`) plus registry wiring.
TTL enforcement uses the Policy setting above.

## Huyao dynamic pricing

- VM: `RunStats` engine field, post-instruction stats hook, stats collection in
  the jump-table handlers, hardfork-selected pre/post instruction pricing
  hooks, and `GetPersistingIndex`.
- Contract layer: `StaticCoefficients`, `DynamicPriceTable`, `OpcodeV1`
  pricing, and femtoGAS fee accounting (`AddFemtoGas` / `GasLeft` /
  `FeeConsumed`).
- Interop prices: static prices for the 25 fixed syscalls plus variable fees
  (CheckWitness rule counts, LoadScript bytes, Notify items, GetNotifications,
  CurrentSigners references, Iterator.Value, CreateMultisig per key,
  Contract.Call/CALLT disk fee, CheckMultisig, CheckSig).
- The direct NEP17 fast path keeps Huyao fee parity (OpcodeV1, Call price,
  RunStats).
- Pre-Huyao jump table keeps the legacy `SETITEM_Before606` /
  `Roll_Before575` handlers.

## Huyao behaviour fixes

- `StdLib.strLen` returns codepoint counts under Huyao.
- `System.Storage.FindWithStart` / `Local.FindWithStart` syscalls (Huyao
  gate): seek parity for forward/backward scans, local-store variant, and the
  pre-Huyao fault.
- `CryptoLib.recoverSecp256K1` splits V0 (BC-wrap math) from V1 (R/S range
  check) at Huyao; `VerifyWithECDsa` widening required no change (V0/V1 were
  already all-false and the V0 curve whitelist faults).

## Non-hardfork ports

- `MerkleTree.Trim` depth-vs-flags fix (`neo-crypto/src/merkle_tree.rs`).
- Witness cost helpers: `SignatureContractCost`, multi-sig costs, and
  `VerifyWitnesses` gained the `cap_gas` parameter
  (`neo-smart-contract/src/helper.rs`); call sites in neo-ledger,
  neo-wallets, and neo-oracle were rewired; `Transaction` deserialization
  enforces `MaxTransactionSize`.
- RPC parity: `sendrawtransaction` rejects raw payloads longer than
  `MaxTransactionSize` with the -509 invalid-size error *before*
  deserialization, mirroring the C# `RpcServer.Node.cs` pre-check
  (byte-identical from v3.10.2 through master-n3;
  `neo-rpc-server/src/rpc_server_node/mod.rs`).
- `Header.Verify` applies the 3 GAS budget verbatim
  (`capGas: false`) — the caller budget is no longer capped to
  `Helper::MAX_VERIFICATION_GAS` (`neo-ledger/src/validation/header.rs`).
- `DataCache`/`IReadOnlyStore` `Find` skip and `StorageKey` comparison helpers
  match the C# `ClonedCache` delta.
- `StdLib.jsonSerialize` reject-cycles message matches the C# text.
- `Notary` guards: empty-notaries silent return on persist; the Conflicts
  verification else-branch.
- `NeoToken` vote early-return; `CryptoLib` BLS Iara identity gate in
  `bls12381_deserialize` (96-byte branch).
- Manifest deploy/update comparer (legacy-path compatibility) — verified
  already implemented.
- `Signer` `get_all_rules`/`get_scope_rules` — verified already in parity.

## P2P and mempool

- `MemoryPool.checkConflicts` step 1 matches **any common signer**
  (`HasCommonSigner`) instead of the resolved payer; step 2 and the
  `conflictsFeeSum != 0 && conflictsFeeSum >= NetworkFee` tail are unchanged.
- `MemoryPool.updatePoolForBlockPersisted` scans verified **and** unverified
  transactions for stale entries (a demoted transaction whose Conflicts target
  was persisted is evicted).
- `MerkleBlockPayload`: `Size` counts `TxCount.GetVarSize()` (var-int width,
  not a fixed 4 bytes) and deserialization bounds the hash count by
  `RoundUpToPowerOf2(TxCount)` (trimmed odd merkle levels duplicate the last
  subtree).
- `TaskManager.OnTaskCompleted` fully releases the global slots
  (`globalInvTasks.Remove` / `globalIndexTasks.Remove`) and clears
  `InvTasks`/`IndexTasks` from **every** session, not only the sender's.
- `TaskManager.OnTimer` counts timed-out index tasks per session
  (`TaskSession.BlockTaskTimeouts`, saturating) and re-requests in ascending
  order of that counter.
- `RequestTasks` removes hashes already in flight on the same session from
  `AvailableTasks` before scheduling; `OnNewTasks` marks a hash available only
  when it is globally tracked **and** not in flight on this session; a session
  never re-requests headers while its own request is pending.
- `Peer.IsIntranetAddress` implements the v3.11.2 rules (unmapped IPv4 ranges:
  0.0.0.0/8, 10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.168/16; IPv6:
  loopback, unspecified, link-local, fec0::/10, fc00::/7). Addr endpoints are
  filtered so learned peers never point into our own network
  (`neo-p2p/src/p2p/remote_node/peer_messages.rs`).
- Out-of-window blocks still notify the task manager so matching tasks
  complete instead of stalling until the timeout
  (`neo-p2p/src/p2p/remote_node/inventory.rs`).
- Verified already in parity (no change): `NodeCapability` DisableCompression /
  ArchivalNode wire formats, Signer rule rendering, overflow-safe height scans
  (Rust uses saturating arithmetic throughout), `OnPersistCompleted` release.

## Deliberate Rust divergences (kept)

- `TaskSession` caches full received blocks with a 64 MiB / 256-entry per-peer
  budget instead of C#'s hash-only `ReceivedBlockHashes` map (stronger DoS
  posture, superset of the C# mismatch/abort checks).
- `TaskSession::MAX_PENDING_TASKS = 4096` vs C#'s 100 (pipelined range
  requests stay effective).
- Outbound queues are bounded per lane (1024) and per peer (8 MiB) with
  fail-closed errors, exceeding the C# queue rework.
- The timer keeps a request cooldown for peers whose batch just expired, and
  the Addr filter omits C#'s local-address set (not statically available; the
  local set is handled at connection time).

## Verification

- `cargo fmt --all --check`, `cargo check --workspace --all-targets`: clean.
- Full-workspace `cargo test --workspace --no-fail-fast`: all targets green
  (libs, integration suites, doctests). The run surfaced three stale
  rpc-server expectations, each confirmed against the C# v3.11.2 sources and
  updated: the PUSH1 `gasconsumed` vector (66 datoshi under Huyao dynamic
  pricing), a wallet-relay scenario that now succeeds under the recalibrated
  69,911-datoshi signature cost, and the `sendrawtransaction` oversize
  mapping aligned to the C# -509 pre-check.
- Key suites: neo-ledger (239), neo-p2p (166), neo-mempool (70),
  neo-rpc-server (373), neo-vm (245 plus integration), the 95-test
  architecture guard suite, and the neo-native-contracts /
  neo-smart-contract suites from the hardfork phases.
- Version: workspace bumped 0.21.0 → 0.22.0 (root, fuzz, benches-package,
  tests manifests).
