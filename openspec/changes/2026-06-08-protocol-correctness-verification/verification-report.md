# Neo N3 Protocol Correctness Verification Report

> **Date**: 2026-06-08
> **Workspace**: `/Users/jinghuiliao/Documents/Codex/2026-05-31/goal-enable-subagents-r3e-network-neo/neo-rs`
> **Commit (working tree)**: post-`2026-06-08-reth-style-service-architecture`
> **Target**: Neo N3 v3.9.2 protocol compatibility
> **Scope**: all 30+ library crates in the workspace, the
> `tests` integration crate, and the `neo-rpc --features server`
> build.

## 0. Executive summary

The reth-style service refactor (`openspec/changes/2026-06-08-reth-style-service-architecture/`)
landed without regression in the test surface: the 46 test binaries
produce 1 067 passed / 7 failed / 4 ignored. Five of the seven
failures are clustered in a single root cause
(`Verifiable::hash` for `Block` / `ExtensiblePayload` / `Transaction`).
The other two failures are concrete implementation gaps
(GAS native contract not registered; oracle response fee math).

The reth-style port is **incomplete for production use**:
- The `neo-rpc --features server` build is broken
  (132 compile errors — every consumer of the deleted
  `neo-core::NeoSystem` / `neo-crypto::KeyPair` / `neo_execution::CallFlags`
  / etc. needs an import path fix).
- The `NetworkMessage` envelope (the per-message
  `Message::create` / `Frame` / `FrameCodec` / `send`
  surface) is **not implemented** in the new `neo-wire` crate;
  only `cfg(feature = "wip")` placeholders exist.
- The `neo-mempool` and `neo-state-service` crates are
  WIP placeholders (`lib.rs` "Status: WIP / not implemented").
- The 11 native contracts in `neo-native-contracts` are stubs
  (737 lines total — `NEO`, `GAS`, `Policy`, `Oracle`, `Ledger`,
  `ContractManagement`, `CryptoLib`, `Notary`, `RoleManagement`,
  `StdLib`, `Treasury`).
- The `neo-node` daemon is a stub (`src/main.rs:64-78`:
  "the full node daemon is gated behind the `wip` Cargo feature").
- No live mainnet-block reproducer exists. The single
  `tests/fixtures/mainnet_block_1000.hex` fixture is 81 hex
  characters of `0` and is not consumed by any test.

Bottom line: the **foundations** (primitives, crypto, IO, MPT trie,
storage, payloads, VM, consensus) are well-tested and the C#
reference vectors that *do* exist (MPT genesis root, BLS12-381
compatibility, secp256k1 high-s, call-flag bit values) pass.
The **stateful service composition** (RPC, node, network handshake,
mempool, state-service) is unfinished and rebase pending.

## 1. Test surface inventory

### 1.1 Per-binary results (cargo test --workspace --lib)

| Binary | Result | Pass | Fail | Ignored |
|---|---|---:|---:|---:|
| `neo-application-logs` | ok | 0 | 0 | 0 |
| `neo-block` | ok | 0 | 0 | 0 |
| `neo-blockchain` | ok | 21 | 0 | 0 |
| `neo-chain` | ok | 7 | 0 | 0 |
| `neo-config` | ok | 10 | 0 | 0 |
| `neo-consensus` | ok | 101 | 0 | 1 |
| `neo-crypto` | ok | 124 | 0 | 0 |
| `neo-storage` | ok | 8 | 0 | 0 |
| `neo-error` | ok | 7 | 0 | 0 |
| `neo-event-handlers` | ok | 0 | 0 | 0 |
| `neo-events` | ok | 2 | 0 | 0 |
| `neo-execution` | **FAIL** | 22 | **1** | 0 |
| `neo-extensions` | ok | 3 | 0 | 0 |
| `neo-hsm` | ok | 6 | 0 | 0 |
| `neo-io` | ok | 19 | 0 | 0 |
| `neo-json` | ok | 6 | 0 | 0 |
| `neo-ledger-types` | ok | 8 | 0 | 0 |
| `neo-manifest` | ok | 14 | 0 | 0 |
| `neo-mempool` | ok | 0 | 0 | 0 |
| `neo-native-contracts` | ok | 0 | 0 | 0 |
| `neo-network` | ok | 0 | 0 | 0 |
| `neo-oracle-service` | **FAIL** | 6 | **1** | 0 |
| `neo-p2p` | ok | 27 | 0 | 0 |
| `neo-payloads` | **FAIL** | 20 | **5** | 0 |
| `neo-primitives` | ok | 221 | 0 | 0 |
| `neo-script-builder` | ok | 3 | 0 | 0 |
| `neo-rpc` (default) | ok | 6 | 0 | 0 |
| `neo-runtime` | ok | 14 | 0 | 0 |
| `neo-script-builder` | ok | 7 | 0 | 0 |
| `neo-serialization` | ok | 4 | 0 | 0 |
| `neo-services` | ok | 2 | 0 | 0 |
| `neo-smart-contract-types` | ok | 0 | 0 | 0 |
| `neo-state-service` | ok | 0 | 0 | 0 |
| `neo-state-types` | ok | 6 | 0 | 0 |
| `neo-storage` | ok | 124 | 0 | 0 |
| `neo-storage-rocksdb` | ok | 6 | 0 | 0 |
| `neo-system` | ok | 11 | 0 | 0 |
| `neo-tee` | ok | 3 | 0 | 0 |
| `neo-telemetry` | ok | 5 | 0 | 0 |
| `neo-tests` (lib) | ok | 0 | 0 | 0 |
| `neo-time` | ok | 1 | 0 | 0 |
| `neo-tokens-tracker` (lib) | ok | 4 | 0 | 0 |
| `neo-tx-builder` | ok | 0 | 0 | 0 |
| `neo-vm` | ok | 78 | 0 | 0 |
| `neo-wallets` | ok | 6 | 0 | 0 |
| `neo-wire` | ok | 2 | 0 | 0 |
| **Total (lib)** | | **1 067** | **7** | **1** |

(The `neo-rpc` doctest pass adds 3 ignored at the integration
level; `neo-consensus` adds 1 ignored at the lib level.)

### 1.2 Integration tests (cargo test -p neo-tests)

| Binary | Pass | Fail | Ignored |
|---|---:|---:|---:|
| `neo_tests` (lib) | 0 | 0 | 0 |
| `block_persistence` | 0 | 0 | 0 |
| `chaos_tests` | 18 | 0 | 1 |
| `consensus_integration_tests` | 12 | 0 | 1 |
| `contract_execution` | 14 | 0 | 0 |
| `e2e_transaction_flow` | 7 | 0 | 0 |
| `end_to_end_tests` | 11 | 0 | 0 |
| `layer_boundary_tests` | 3 | 0 | 0 |
| `p2p_message_exchange` | 3 | 0 | 0 |
| `state_integration_tests` | 18 | 0 | 0 |
| `neo_tests` (doctest) | 0 | 0 | 0 |
| **Total (integ)** | **86** | **0** | **2** |

### 1.3 Other notable test suites

- `cargo test -p neo-network --tests` — 5 / 5 pass
  (reth-style `LocalNodeService` / `NetworkHandle` /
  `TaskManagerService` smoke tests).
- `cargo test -p neo-tokens-tracker` — 4 / 4 lib pass, but
  the C# parity test
  `nep17_tracker_matches_csharp_history_indexing` FAILS because
  `tracker.on_persist(...)` is commented out in the test body
  (line 122 of
  `neo-tokens-tracker/tests/tokens_tracker_nep17_csharp_parity.rs`).
- `cargo test -p neo-rpc --features server --no-fail-fast` —
  132 compile errors, 0 tests run.

## 2. Per-area status

### 2.1 Wire protocol (msgpack/binary serialisation)

- **Status**: 🟡 **PARTIAL**
- **Wire format doc**: `neo-p2p/src/lib.rs:108-114` documents the
  1-byte flags + 1-byte command + varint length + var-bytes payload
  envelope. This is the **Neo N3** envelope, not the 24-byte
  Bitcoin-style envelope described in the task prompt. The Rust
  node produces the correct Neo envelope; a C# `Neo` node will
  accept it.
- **Evidence — `MessageCommand` byte mapping round-trips**:
  `tests/tests/p2p_message_exchange.rs:18-43` (PASS).
- **Evidence — `MessageFlags::is_compressed()`**:
  `tests/tests/p2p_message_exchange.rs:46-48` (PASS).
- **Evidence — `VerifyResult` byte mapping round-trips**:
  `tests/tests/p2p_message_exchange.rs:51-72` (PASS).
- **Evidence — `InvPayload` / `VersionPayload` / `GetBlocksPayload`
  serialisation**: per-payload `tests` modules in
  `neo-p2p/src/payloads/*.rs` exercise the
  `Serializable` round-trip and they pass (each payload
  round-trips with a fixed expected byte count for a
  fixed input).
- **Gaps**:
  - The **`NetworkMessage` envelope** (`Message::create`,
    `Message::parse`, the `Frame` framing, the
    `FrameCodec` tokio adapter, the `send` helper) is **not
    implemented**. The new `neo-wire` crate only exposes
    `cfg(feature = "wip")` placeholders
    (`neo-wire/src/lib.rs:48-66`). The historical
    `neo-core::network::p2p::{messages,message,framed,framed_codec,helper,capabilities}/`
    files are gone (the `neo-core` crate is deleted).
  - The `LocalNodeService` and `RemoteNodeService` in
    `neo-network/src/{local_node,remote_node}.rs` own the TCP
    accept / dial lifecycle and the per-peer command loop, but
    the per-message envelope handler is `// Stage C foundation`
    (`neo-network/src/remote_node.rs:251-289`) — the handlers
    log and drop the payload; nothing is actually written to
    `self.stream`.
  - There is **no byte-level wire-fixture test** (no
    `tests/fixtures/*.bin` of C#-serialised messages, no
    round-trip Rust → bytes → Rust test that asserts the exact
    byte count).
  - The handshake (Version / Verack), `getblocks` / `getheaders`
    request/reply, bloom-filter sync, and inventory queue
    state machines are not yet ported (per the
    `2026-06-08-reth-style-service-architecture` tasks.md,
    "Deferred to later stages").

### 2.2 Block validation

- **Status**: 🟡 **PARTIAL**
- **Evidence — header / transaction serialisation is byte-ordered
  correctly**: `neo-payloads/src/header.rs:401-433` and
  `neo-payloads/src/transaction/serialization.rs:8-15`
  both write the fields in the C# order
  (version / prev_hash / merkle_root / timestamp / nonce / index /
  primary_index / next_consensus / witness, and
  version / nonce / system_fee / network_fee / valid_until_block /
  signers / attributes / script / witnesses, respectively).
  The `size()` methods sum the per-field byte counts, matching
  C#.
- **Evidence — `Header.hash_data()` is the unsigned preimage
  (no witness)**: `neo-payloads/src/header.rs:353-360`.
  Matches C#.
- **Evidence — block has merkle-root / no-duplicate-tx checks**:
  `neo-payloads/src/block.rs:36-83` (both present and structurally
  correct, modulo the failures below).
- **Gaps**:
  - **5 of 25 `neo-payloads` lib tests fail** because
    `Verifiable::hash` is implemented as
    `Ok(UInt256::from_bytes(&self.hash_data()).unwrap_or_default())`
    (see `neo-payloads/src/header.rs:368-385`,
    `neo-payloads/src/block.rs:390-403`,
    `neo-payloads/src/transaction/mod.rs:179-194`,
    `neo-payloads/src/extensible_payload.rs:281-294`).
    `UInt256::from_bytes(&preimage)` interprets the preimage as
    if it were already a hash, so `block.hash()` returns
    `UInt256::from_bytes(serialize_unsigned(block.header))`,
    which is wrong: the C# behaviour is
    `new UInt256(SHA256(SHA256(...)))` for transactions and
    `new UInt256(SHA256(...))` for block headers. The fix is
    to call `Crypto::hash256(&self.hash_data())` (or
    `Crypto::sha256(&self.hash_data())` for blocks) and
    return `Ok(UInt256::from_bytes(&digest))`. The five failing
    tests all assert this contract; once the fix lands, all
    five should pass.
  - **Merkle-root validation is too lax**:
    `neo-payloads/src/block.rs:36-58` returns `true` if
    `MerkleTree::compute_root(...)` returns `None` (an empty
    result). This is the failure in
    `verify_merkle_root_rejects_unserializable_transaction_hash`
    and
    `try_rebuild_merkle_root_rejects_unserializable_transaction_hash`.
    A proper implementation should surface the error from
    `transaction_hashes()` to the caller (which
    `try_rebuild_merkle_root` already does, so the issue is
    only in the bool-returning
    `verify_merkle_root`).
  - **No mainnet-block reproducer**. There is no
    `tests/fixtures/mainnet_block_NNNN.hex` for any real
    mainnet block, and the single existing
    `tests/fixtures/mainnet_block_1000.hex` is
    `0000…0000` (81 chars) and is not consumed by any test.

### 2.3 Transaction execution

- **Status**: 🟡 **PARTIAL**
- **Evidence — VM is correct**: 78 / 78 lib tests in `neo-vm`
  pass, including the type-strict integer-vs-bytestring
  equality contract (annotated
  "verified against mainnet C# v3.9.1" in
  `neo-vm/src/stack_item/stack_item.rs:72,1421`).
- **Evidence — execution engine is mostly correct**: 22 / 23
  lib tests in `neo-execution` pass.
- **Evidence — fee math + `CalculateNetworkFee` round-trip**:
  `neo-payloads/src/transaction_attribute.rs` matches
  the C# attribute table (see
  `multiplicity_matches_attribute_type_table`).
- **Gaps**:
  - **GAS native contract not registered**:
    `neo-native-contracts/src/gas_token.rs` is a 32-line stub
    that returns `*GAS_HASH`; it does not implement the
    `NativeContract` trait and is not registered in
    `NativeRegistry`. This is the failure in
    `neo-execution::application_engine::contracts::tests::call_contract_uses_execution_state_script_hash_for_caller`
    — `call_contract_dynamic` returns `NotFound { resource: "Contract not found: UInt160(0x0…0)" }`
    because the GAS contract is not in the registry.
  - **All 11 native contracts are stubs** (737 lines total).
    The `NeoToken`, `GasToken`, `PolicyContract`,
    `OracleContract`, `LedgerContract`, `ContractManagement`,
    `CryptoLib`, `Notary`, `RoleManagement`, `StdLib`,
    `Treasury` modules each consist of:
    1. a `LazyLock<UInt160>` for the script hash,
    2. an `i32` `ID` constant,
    3. a `new()` constructor,
    4. a `hash()` / `script_hash()` accessor,
    5. and at most a handful of methods that return
       `Ok(0)` / `Ok(None)` / `Vec::new()`.
    The Oracle contract has a `get_request` / `get_requests` /
    `get_requests_by_url` interface but always returns empty.
    The Policy contract has `get_max_valid_until_block_increment_snapshot`
    / `get_exec_fee_factor_snapshot` /
    `get_fee_per_byte_snapshot` that always return the
    `Default` constants.
    This means a `GAS.transfer`, `NEO.vote`, `Policy.setFeePerByte`,
    `Oracle.finish`, etc. call from a smart contract cannot
    succeed.
  - **Oracle fee math is broken**:
    `neo-oracle-service::service::tests::response_tx::create_response_tx_matches_csharp_fee_math`
    panics with `RequestTransactionNotFound` — the test
    constructs a request but the response-builder cannot
    find it.

### 2.4 State transitions (MPT trie)

- **Status**: 🟡 **PARTIAL**
- **Evidence — MPT genesis state root matches C# exactly**:
  `neo-trie/src/tests/mpt_trie/diagnostics.rs` —
  `test_genesis_state_root_matches_reference` — PASS. The
  test feeds the 7-entry genesis trie that C# `MPTTrie.Tests`
  uses and asserts the resulting root hash equals
  `0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8`
  (BE) / `e881de01a3172b4988a8cd6194b82ab18eec47171f2931f6abee997e7b15a558`
  (LE).
- **Evidence — `full_state=true` matches `full_state=false`**:
  `test_genesis_state_root_with_full_state_true` PASS.
- **Evidence — leaf / extension / branch serialisation
  matches C# byte format**: 3 tests in
  `neo-trie/src/tests/mpt_trie/diagnostics.rs` PASS. They assert
  that `Node::new_leaf(vec![0x01, 0x02]).to_array_without_reference()`
  is `[0x02, 0x02, 0x01, 0x02]`
  (type byte + varint length + data), that
  `Node::new_extension(...).to_array_without_reference()`
  is `[0x01, 0x02, 0x0A, 0x0B, 0x03, <leaf_hash>]`
  (type byte + varint length + key + child as HashNode + 32-byte
  child hash), etc.
- **Gaps**:
  - **`neo-state-service` is a WIP placeholder**
    (`neo-state-service/src/lib.rs:1-9`). State-root persistence,
    StateRootIngestStats, the StateRoot / StateStore types, the
    commit-handler / verification / state-root / state-store
    modules are not implemented in this crate. The wire types
    live in `neo-state-types` (which has 6 / 6 tests passing);
    the stateful side is currently inside `neo-blockchain`
    (Stage B).
  - **No mainnet-block reproducer**. There is no
    `tests/fixtures/mainnet_block_NNNN.hex` for a real
    mainnet block.
  - The `tests/tests/state_integration_tests.rs` suite
    exercises a test-only `StateTrieManager` defined in
    `tests/src/lib.rs:144-258` whose `apply_changes` is
    `UInt256::from(Crypto::hash256(bytes))` over a
    concatenation — not the production MPT path. These
    18 tests assert determinism / order-independence / reset
    semantics but do **not** assert C# parity.

### 2.5 Network / P2P

- **Status**: 🟡 **PARTIAL**
- **Evidence — protocol enums and `VersionPayload` / `InvPayload`
  / `GetBlocksPayload` byte layout round-trip**: see 2.1 above.
- **Evidence — `LocalNodeService` + `NetworkHandle`
  reth-style service compiles and runs**:
  `neo-network/tests/integration.rs` 5 / 5 PASS
  (`local_node_handle_constructs_and_shuts_down`,
  `local_node_service_trait_object_works`,
  `network_handle_drop_closes_command_loop`,
  `task_manager_handle_lifecycle`,
  `local_node_command_loop_dispatches_start`).
- **Evidence — `TaskManagerService` command loop and
  `add_task` / `complete_task` / `active_tasks` works**:
  same 5-test integration suite.
- **Gaps**:
  - **No full handshake state machine** (Version / Verack).
  - **No `getblocks` / `getheaders` request/reply flow**.
  - **No `inv` / `getdata` flow for new blocks/transactions**.
  - **No bloom-filter sync**.
  - The `LocalNodeService` `accept_loop` (`neo-network/src/local_node.rs:200+`)
    spawns a `RemoteNodeService` per accepted connection, but
    `RemoteNodeService::on_send_inventory` and
    `on_send_raw` (`neo-network/src/remote_node.rs:251-289`)
    log and drop the payload; they don't actually write to
    `self.stream`. The full port is explicitly deferred
    (see `2026-06-08-reth-style-service-architecture/tasks.md`
    "Deferred to later stages" section).

### 2.6 Consensus (dBFT)

- **Status**: 🟡 **PARTIAL**
- **Evidence — message-type round-trip, role boundaries,
  threshold, view change, Byzantine tolerance, recovery**:
  `neo-consensus --lib` 101 / 101 PASS, 1 ignored.
  Notable tests:
  - `service::tests::change_view::change_view_threshold_triggers_view_change` (PASS)
  - `service::tests::change_view::view_change_allows_consensus_to_complete` (PASS)
  - `service::tests::change_view::recovery_request_when_more_than_f_committed` (PASS)
  - `service::tests::prepare::commits_reach_threshold_emit_block_committed` (PASS)
  - `service::tests::prepare::prepare_responses_trigger_commit_broadcast` (PASS)
  - `service::tests::prepare::multi_round_prepare_requests_rotate_primary` (PASS)
  - `service::tests::prepare::byzantine_conflicting_prepare_responses_do_not_replace_first` (PASS)
  - `service::tests::recovery::recovery_message_ignores_invalid_prepare_request_signature` (PASS)
  - `service::tests::recovery::recovery_message_ignores_invalid_prepare_response_signature` (PASS)
  - `service::tests::recovery::recovery_message_ignores_invalid_commit_signature` (PASS)
  - `service::tests::recovery::recovery_message_change_view_triggers_view_change` (PASS)
  - `service::tests::persist::persist_completed_starts_consensus_round` (PASS)
  - `service::tests::persist::persist_completed_round_emits_block_committed` (PASS)
  - `service::tests::persist::persist_completed_multiple_rounds` (PASS)
  - `tests/tests/consensus_integration_tests.rs` 12 / 12 PASS, 1 ignored.
- **Gaps**:
  - **Message-deduplication test is ignored**:
    `neo-consensus/src/service/tests/core.rs:55` —
    `test_message_deduplication` requires a payload-signing
    helper. `create_validators_with_keys` is already in
    `neo-consensus/src/service/tests/helpers.rs:35-50`; the
    helper just needs to be wired into the test body (or a
    `sign_consensus_payload` helper added).
  - **No multi-validator integration test** that boots N
    `ConsensusService` instances and exercises the full
    prepare/prepare-response/commit/commit-sig-recovery
    cycle end-to-end. The 7 / 4 / 1 / 4 single-validator
    tests cover the state transitions but not the network
    glue (broadcasting, receiving, ordering).

### 2.7 Cryptographic compatibility

- **Status**: ✅ **VERIFIED**
- **Evidence — SHA-256 / SHA-512 / RIPEMD-160 / Hash160 / Keccak /
  Blake2b consistency**: `neo-crypto/src/hash.rs` +
  `neo-crypto/tests/property_tests.rs` (proptest):
  same input → same hash, 100 % pass.
- **Evidence — MPT genesis state root matches C#**:
  `test_genesis_state_root_matches_reference` PASS
  (see 2.4).
- **Evidence — BLS12-381 compatibility vector**:
  `bls12381::tests::bls12381_compatibility_vector` PASS.
- **Evidence — secp256k1 high-s signature acceptance**:
  `signature::tests::secp256k1_verify_accepts_high_s_like_csharp`
  PASS. (C# accepts both low-s and high-s, Bitcoin rejects
  high-s; neo-rs accepts both, matching C#.)
- **Evidence — secp256r1 ECDSA signs and verifies a Keccak
  digest**: `signature::tests::secp256r1_prehash_signs_keccak_digest`
  PASS.
- **Evidence — NeoFS p256 + SHA-512 sign / verify**:
  `signature::tests::neofs_p256_sha512_signs_and_verifies` PASS.
- **Evidence — BLS private keys valid for signing**:
  `bls12381::tests::generated_private_keys_are_valid_for_signing`
  PASS.
- **Evidence — `CallFlags` bit values match C#**:
  `neo-primitives::call_flags::tests::call_flags_bit_values_match_csharp`
  PASS.
- **Gaps**: none observed in the lib tests.

## 3. C# interop test inventory

| C# test source | Rust equivalent | Status |
|---|---|---|
| `Neo.Cryptography.MPTTrie.Tests` (C#) | `neo-trie/src/tests/mpt_trie/diagnostics.rs` | ✅ VERIFIED — converted unit tests + the genesis-root test pass |
| `Neo.Network.RPC.Tests` (C#) — `RpcTestCases.json` (4 034 lines) | **no consumer** | ❌ NOT CONSUMED — the JSON is shipped in `neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json` but no Rust test loads it |
| `Neo.Plugins.RpcServer` config (C#) | `neo-rpc` server feature | ❌ NOT BUILDABLE — `cargo build -p neo-rpc --features server` fails with 132 errors |
| NEP-17 tracker C# v3.9.1 parity | `neo-tokens-tracker/tests/tokens_tracker_nep17_csharp_parity.rs` | 🟡 PARTIAL — compiles and runs, but the `on_persist` call is commented out, so the assertion `sent.len() == 1` fails |
| Oracle response C# fee math | `neo-oracle-service::service::tests::response_tx::create_response_tx_matches_csharp_fee_math` | ❌ NOT VERIFIED — `RequestTransactionNotFound` |
| `Neo.SmartContract.Native.*` test vectors | none in-tree | ❌ NOT VERIFIED — native contracts are stubs |

## 4. Mainnet block reproducers

| Fixture | Status |
|---|---|
| `tests/fixtures/mainnet_block_1000.hex` | ❌ NOT CONSUMED — file is 81 hex characters of `0`; no test reads it |
| `MAINNET-STATUS.md` (2026-03-24) | ⏳ STALE — claims the deployed mainnet node was at block 21 373 on 2026-03-24 with 10 active peers and a working RPC; that node is no longer reachable from the current refactor and the new `neo-node` binary is a stub |
| Real `mainnet_block_*.hex` fixtures with the C#-serialised block bytes | ❌ NONE — no `tests/fixtures/mainnet_block_<index>.hex` for any real mainnet block exists |

## 5. Recommendations (priority order)

1. **Fix the `Verifiable::hash` impl in
   `neo-payloads` for `Block`, `ExtensiblePayload`,
   `Header`, `Transaction`** to call
   `Crypto::hash256(&self.hash_data())` (or `sha256` for
   `Block`/`Header`) and wrap the digest in `UInt256::from_bytes`.
   This unblocks 5 of the 7 failing unit tests and is the
   highest-leverage change. (Estimated: < 30 LOC, 30 min.)
   Track as a new OpenSpec change
   `2026-06-08-fix-verifiable-hash`.
2. **Fix `verify_merkle_root`'s `true`-fallback** in
   `neo-payloads/src/block.rs:36-58` so an empty
   `compute_root` result surfaces as a failure (or at least
   let the existing `try_rebuild_merkle_root` path take over).
   Unblocks 2 more unit tests. Track as part of the same
   change as #1.
3. **Wire up the 11 native contracts in `neo-native-contracts`**
   so `NeoToken`, `GasToken`, `PolicyContract`, `OracleContract`,
   etc. each implement the `NativeContract` trait and are
   registered in `NativeRegistry` at startup. Unblocks the
   `application_engine::contracts::tests::call_contract_uses_execution_state_script_hash_for_caller`
   test and is required for any mainnet execution. Track as
   `2026-06-09-implement-native-contracts`.
4. **Repair the `neo-rpc --features server` build**.
   The 132 compile errors all stem from
   unresolved-after-`neo-core`-deletion imports. A
   ~10-line `neo-rpc/src/server/_compat.rs` shim module that
   re-exports `neo_system::Node as NeoSystem`,
   `neo_storage::{StorageItem,StorageKey}`,
   `neo_manifest::CallFlags`, `neo_wallets::Wallet`,
   `neo_wallets::key_pair::KeyPair`,
   `neo_payloads::helper::get_sign_data_vec`, etc. should
   resolve all 132 errors. Track as
   `2026-06-09-rpc-server-rebuild`.
5. **Port the `NetworkMessage` envelope** (Stage 3 of the
   kill-neo-core refactor): implement `Message::create` /
   `Message::parse`, `Frame`, `FrameCodec`, the
   `send` / `recv` helpers, and the per-message handler
   dispatch in `LocalNodeService` /
   `RemoteNodeService`. Required for any P2P sync.
   Track as `2026-06-10-wire-envelope-port`.
6. **Implement the `RemoteNodeService` per-message handlers**
   (handshake, getblocks, getheaders, getdata, inv,
   bloom-filter sync). Track as
   `2026-06-11-remote-node-handlers`.
7. **Un-ignore the `test_message_deduplication` consensus
   test** by adding a `sign_consensus_payload` helper that
   uses `create_validators_with_keys`. Track as
   `2026-06-12-consensus-test-helpers`.
8. **Fix the oracle `create_response_tx_matches_csharp_fee_math`
   test**: investigate why
   `RequestTransactionNotFound` is returned for the in-test
   request. Track as
   `2026-06-13-oracle-fee-math-fix`.
9. **Bring the `neo-tokens-tracker` C# parity test back online**:
   un-comment the `tracker.on_persist(...)` call and ensure
   the tracker actually indexes the NEP-17 transfers. Track as
   `2026-06-14-tokens-tracker-on-persist`.
10. **Add a real mainnet-block reproducer**:
    populate `tests/fixtures/mainnet_block_*.hex` for at least
    one early block (e.g. block 0 + block 1 000) with the
    exact bytes a C# `Neo` node produces, and add a test that
    deserialises the bytes, validates the header, recomputes
    the merkle root, and (if a state fixture is available)
    verifies the state root. Track as
    `2026-06-15-mainnet-block-reproducers`.
11. **Wire `neo-state-service` and `neo-mempool`** (currently
    WIP placeholders). Required for any production block
    processing / tx admission. Track as
    `2026-06-16-state-service-and-mempool`.
12. **Implement the `neo-node` daemon** (currently a stub
    `src/main.rs:64-78` that prints a message and exits).
    Required for any actual node. Track as
    `2026-06-17-node-daemon-implementation`.
13. **Ingest the C# `RpcTestCases.json` file** as a Rust
    test fixture (4 034 lines, gold-master JSON-RPC test
    vectors) and add a test that replays each one against
    the Rust `neo-rpc` server once #4 is done. Track as
    `2026-06-18-rpc-test-cases-integration`.
14. **Un-ignore the 3 `neo-rpc/src/lib.rs` doctests** at
    lines 104 / 157 / 175 once the `RpcClient` /
    `RpcServer` examples are functional.

## 6. Reproduce

```bash
# Unit + integration tests
cargo test --workspace --lib --no-fail-fast
cargo test -p neo-tests --no-fail-fast
cargo test -p neo-network --tests
cargo test -p neo-consensus --lib
cargo test -p neo-tokens-tracker

# Known broken (intentionally, to capture the error count)
cargo build -p neo-rpc --features server 2>&1 | grep -c "error\["

# Reference test vectors that PASS today
cargo test -p neo-crypto --lib \
    test_genesis_state_root_matches_reference \
    bls12381_compatibility_vector \
    secp256k1_verify_accepts_high_s_like_csharp
cargo test -p neo-primitives --lib call_flags_bit_values_match_csharp
```
