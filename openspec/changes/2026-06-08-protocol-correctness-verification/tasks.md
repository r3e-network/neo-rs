# Tasks — Protocol Correctness Verification

> Run the verification harness top-to-bottom. Each step records
> raw command output for the verification report. All commands run
> from the workspace root with no special environment.

## 1. Inventory the test surface

- [x] 1.1 List all `[[test]]` integration suites declared in
      `tests/Cargo.toml`. **9 suites** (see
      `tests/Cargo.toml` `[[test]]` table; all under
      `tests/tests/*.rs`).
- [x] 1.2 List all `cargo test --workspace --lib` binaries.
      **46 binaries** (one per library crate).
- [x] 1.3 Identify which test files reference mainnet / C# /
      parity vectors. **2 files**:
      - `neo-tokens-tracker/tests/tokens_tracker_nep17_csharp_parity.rs`
        (FAILS — see verification report).
      - `neo-crypto/src/mpt_trie/tests.rs` (PASSES — C#-converted
        unit tests + `test_genesis_state_root_matches_reference`).
- [x] 1.4 Identify C# reference data shipped in-tree. **1 file**:
      - `neo_csharp/node/plugins/RpcServer/RpcServer.json`
        (C# RpcServer default config; not consumed by tests).
      - `neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json`
        (4 034 lines of C# JSON-RPC test vectors; **not consumed**
        by any Rust test).
- [x] 1.5 Confirm `neo-core` is gone.
      `find . -path ./target -prune -o -name "neo-core" -type d -print`
      returns nothing. ✅

## 2. Build the workspace

- [x] 2.1 `cargo check --workspace` — **green**, 0 errors.
      Warnings only (mostly in `neo-oracle-service` and
      `neo-storage-rocksdb`).
- [x] 2.2 `cargo build -p neo-rpc` (default features) — **green**.
- [x] 2.3 `cargo build -p neo-rpc --features server` —
      **RED**, 132 compile errors. The server module still
      references `NeoSystem` (deleted with `neo-core`),
      `neo_crypto::KeyPair`, `get_sign_data_vec`, `CallFlags`,
      `AssetDescriptor`, `StorageItem`/`StorageKey` (in the wrong
      crate), `Wallet` trait, etc. See verification report.

## 3. Run the unit tests

- [x] 3.1 `cargo test --workspace --lib --no-fail-fast` — full
      results in `/tmp/all_tests.log`. **46 binaries, 1 067 passed,
      7 failed, 4 ignored**.
      Failing tests (all currently NOT VERIFIED for production use):
      - `neo-execution` (1):
        `application_engine::contracts::tests::call_contract_uses_execution_state_script_hash_for_caller`
        (GAS native contract not registered).
      - `neo-oracle-service` (1):
        `service::tests::response_tx::create_response_tx_matches_csharp_fee_math`
        (`RequestTransactionNotFound`).
      - `neo-payloads` (5):
        - `block::tests::iverifiable_block_hash_uses_try_hash`
        - `block::tests::verify_merkle_root_rejects_unserializable_transaction_hash`
        - `block::tests::try_rebuild_merkle_root_rejects_unserializable_transaction_hash`
        - `extensible_payload::tests::iverifiable_extensible_hash_uses_try_hash`
        - `transaction::core::tests::verifiable_hash_rejects_oversized_script`
        (All five stem from the same root cause: the
        `Verifiable::hash` impl on `Block` / `ExtensiblePayload` /
        `Transaction` interprets `hash_data()` (the unsigned
        preimage) as if it were already the hash. The correct
        implementation is `SHA256(hash_data())`. The C# behaviour
        is `SHA256(SHA256(...))` for transactions and `SHA256(...)`
        for blocks; the unit tests assert the double-hash-or-single-
        hash contract.)
      Ignored tests (4):
      - `neo-consensus` (1):
        `service::tests::core::test_message_deduplication`
        (requires a signing helper; `create_validators_with_keys`
        already exists, this is a follow-up).
      - `neo-rpc` (3 doctests) — all `ignore`d doctests in
        `neo-rpc/src/lib.rs`.
- [x] 3.2 `cargo test -p neo-tests --no-fail-fast` — **green**,
      9 binaries, 0 failures. Detailed count in verification report.
- [x] 3.3 `cargo test -p neo-network --tests` — **green**, 5
      integration tests pass.
- [x] 3.4 `cargo test -p neo-rpc --no-fail-fast` — **green**
      (default features, server is off; 6 lib tests pass).
- [x] 3.5 `cargo test -p neo-rpc --features server --no-fail-fast`
      — **RED**, 132 compile errors (see step 2.3).
- [x] 3.6 `cargo test -p neo-consensus --lib` — **green**, 101
      passed, 1 ignored (the deduplication test).
- [x] 3.7 `cargo test -p neo-tokens-tracker` — **PARTIAL**,
      4 lib tests pass; the C# parity test
      `nep17_tracker_matches_csharp_history_indexing` FAILS because
      the test's `tracker.on_persist(...)` call is commented out
      (the `tracker.on_persist` body is still a stub).

## 4. Spot-check the wire format

- [x] 4.1 `MessageCommand` byte values match the C# enum.
      See `neo-p2p/src/message_command.rs` and the
      `Message command | Value` table in
      `neo-p2p/src/lib.rs:120-142`. Round-trip test in
      `tests/tests/p2p_message_exchange.rs:18-43` (`PASS`).
- [x] 4.2 `MessageFlags` `is_compressed()` predicate.
      `tests/tests/p2p_message_exchange.rs:46-48` (`PASS`).
- [x] 4.3 `VerifyResult` enum round-trip.
      `tests/tests/p2p_message_exchange.rs:51-72` (`PASS`).
- [x] 4.4 `InvPayload` / `VersionPayload` / `GetBlocksPayload`
      serialization. `Serializable` impls in
      `neo-p2p/src/payloads/{inv,version,get_blocks}_payload.rs`
      are byte-compatible with C# (each `size` + `serialize` +
      `deserialize` round-trip is exercised by the per-payload
      `tests` modules in `neo-p2p/src/payloads/*.rs`).
- [x] 4.5 **`NetworkMessage` envelope is NOT YET IMPLEMENTED.**
      The wire envelope (`neo_p2p::Message::create`,
      `framed::Frame`, `framed_codec::FrameCodec`, `helper::send`)
      is explicitly deferred to Stage 3 of the kill-neo-core
      refactor and currently `neo-wire/src/lib.rs:48-66` exposes
      only `cfg(feature = "wip")` placeholders. The
      `LocalNodeService` / `RemoteNodeService` in
      `neo-network/src/{local_node,remote_node}.rs` own the
      *connection lifecycle* but the per-message envelope handler
      is not yet wired up (see the `// Stage C foundation` comments
      in `neo-network/src/remote_node.rs:251-289`).
- [x] 4.6 The Neo framing format is documented in
      `neo-p2p/src/lib.rs:108-114`:
      ```text
      ┌──────────┬──────────┬──────────────┬──────────┐
      │  Flags   │ Command  │    Length    │ Payload  │
      │ (1 byte) │ (1 byte) │ (var_int LE) │  (var)   │
      └──────────┴──────────┴──────────────┴──────────┘
      ```
      This is the documented Neo N3 envelope (no magic prefix,
      no checksum). The `MessageHeader` of 24 bytes described in
      the task prompt is the **Bitcoin** envelope, not Neo's;
      Neo's envelope is 1+1+varint+var = at least 3 bytes per
      message. The wire format the Rust node produces is correct
      for Neo, not for Bitcoin.

## 5. Spot-check the dBFT state machine

- [x] 5.1 All six message types are defined and distinct.
      `tests/tests/consensus_integration_tests.rs:381-394`
      (`PASS`).
- [x] 5.2 `PrepareRequest` from a non-primary is rejected.
      `tests/tests/consensus_integration_tests.rs:122-143`
      (`PASS`).
- [x] 5.3 `PrepareResponse` with a future block is ignored,
      wrong view is ignored, mismatched hash is rejected, and
      duplicate responses from the same validator are rejected.
      `neo-consensus --lib` (4 tests in
      `neo-consensus/src/service/tests/prepare.rs`) — all `PASS`.
- [x] 5.4 `Commit` reaches the M threshold and emits
      `BlockCommitted`. `neo-consensus/src/service/tests/prepare.rs`
      (`commits_reach_threshold_emit_block_committed`,
      `prepare_responses_trigger_commit_broadcast`,
      `consensus_round_emits_block_committed`) — `PASS`.
- [x] 5.5 `ChangeView` threshold triggers a view change and
      multi-round prepare requests rotate the primary.
      `neo-consensus/src/service/tests/change_view.rs` and
      `neo-consensus/src/service/tests/prepare.rs` —
      `multi_round_prepare_requests_rotate_primary` — `PASS`.
- [x] 5.6 Byzantine conflicting PrepareResponses do not
      overwrite the first response.
      `neo-consensus/src/service/tests/prepare.rs` —
      `byzantine_conflicting_prepare_responses_do_not_replace_first` — `PASS`.
- [x] 5.7 `RecoveryMessage` ignores invalid signatures
      (PrepareRequest, PrepareResponse, Commit).
      `neo-consensus/src/service/tests/recovery.rs` — `PASS`.
- [x] 5.8 `RecoveryRequest` is ignored by non-selected validators
      and broadcasts a `RecoveryMessage` from the selected one.
      `neo-consensus/src/service/tests/recovery.rs` — `PASS`.
- [x] 5.9 `PersistCompleted` starts a new round and emits
      `BlockCommitted`. `neo-consensus/src/service/tests/persist.rs`
      — `PASS` (3 tests).
- [x] 5.10 `ConsensusPayload` sign-data is deterministic
      (`get_sign_data()`) and the same on both sides.
      `tests/tests/consensus_integration_tests.rs:354-372` — `PASS`.
- [x] 5.11 The `test_message_deduplication` test is **ignored**
      (`neo-consensus/src/service/tests/core.rs:55`). The
      `create_validators_with_keys` helper is already present
      (`neo-consensus/src/service/tests/helpers.rs:35-50`); the
      test still needs the additional signer helper called out in
      its `#[ignore = "..."]` reason.

## 6. Spot-check MPT / state root

- [x] 6.1 MPT genesis state root matches the C# reference.
      `neo-crypto/src/mpt_trie/tests.rs:1063-1101` —
      `test_genesis_state_root_matches_reference` — `PASS`
      (LE: `e881de01a3172b4988a8cd6194b82ab18eec47171f2931f6abee997e7b15a558`,
       BE: `0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8`).
- [x] 6.2 `full_state=true` produces the same root as
      `full_state=false` for the genesis entries.
      `neo-crypto/src/mpt_trie/tests.rs:1102+` —
      `test_genesis_state_root_with_full_state_true` — `PASS`.
- [x] 6.3 Leaf / branch / extension node serialisation matches
      the C# byte format.
      `neo-crypto/src/mpt_trie/tests.rs:807-920` — 3 tests — `PASS`.
- [x] 6.4 `neo-state-service` is a WIP placeholder (see
      `neo-state-service/src/lib.rs:1-9`). State-root persistence,
      StateRootIngestStats, and the `state_service` payload
      category are not yet implemented at the `neo-state-service`
      layer. The wire types are in `neo-state-types` and the
      stateful side is currently in `neo-blockchain` (Stage B).
- [x] 6.5 The `tests/tests/state_integration_tests.rs` suite
      exercises a **test-only** `StateTrieManager` defined in
      `tests/src/lib.rs:144-258` whose `apply_changes` computes
      `UInt256::from(Crypto::hash256(bytes))` over a concatenation
      of the changes; this is **not** the production MPT path.
      These tests are `PASS` (18 of 18) but they do not assert
      C# parity — they only assert determinism / order-independence
      / reset semantics.

## 7. Spot-check execution

- [x] 7.1 The VM (`neo-vm`) passes 78 of 78 lib tests, including
      the integer-vs-bytestring equality contract that
      `neo-vm/src/stack_item/stack_item.rs:72,1421` calls out as
      "verified against mainnet C# v3.9.1".
- [x] 7.2 The execution engine (`neo-execution`) has 22 of 23
      tests passing. The one failure is the GAS native-contract
      lookup (see 3.1 above).
- [x] 7.3 `neo-native-contracts` is **a stub crate**. Every
      native contract (`NEO`, `GAS`, `Policy`, `Oracle`,
      `Ledger`, `ContractManagement`, `CryptoLib`, `Notary`,
      `RoleManagement`, `StdLib`, `Treasury`) is a struct with
      `ID`, `hash`, `script_hash` and a few helper methods that
      return empty / zero values. There is **no real
      NEP-17 transfer / NEO vote / policy enforcement / oracle
      request** logic — see `neo-native-contracts/src/*.rs`. The
      11 contract modules sum to **737 lines total** including
      doc comments.
- [x] 7.4 The `ContractManagement`, `LedgerContract`, `StdLib`,
      `RoleManagement`, `CryptoLib`, `Notary`, `Treasury` modules
      are 32–55 lines each and expose only the public surface
      needed by name lookups.

## 8. Spot-check storage

- [x] 8.1 `neo-storage --lib` — 124 of 124 tests pass. Covers
      `StorageKey` construction, equality, ordering, hashing,
      `TrackState` variants, etc.
- [x] 8.2 `neo-storage-rocksdb --lib` — 6 of 6 tests pass.
      Covers snapshot reads over pending writes, batched commits,
      read-cache invalidation.
- [x] 8.3 No live mainnet-block reproducer writes to the
      RocksDB backend. The `tests/fixtures/mainnet_block_1000.hex`
      file is **81 hex characters of all zeros** and is not
      consumed by any test (a grep across `tests/`,
      `neo-blockchain/`, `neo-execution/`, `neo-crypto/` returns
      no matches).

## 9. Spot-check the RPC build

- [x] 9.1 `cargo build -p neo-rpc` (default features) — green.
- [x] 9.2 `cargo build -p neo-rpc --features server` — 132
      errors. The top unresolved symbols, in order of occurrence:
      - `NeoSystem` (39 occurrences) — used to live in
        `neo-core`; needs a thin re-export shim or
        replacement path (probably `neo_system::Node`).
      - `CallFlags` (13 occurrences total) — should be in
        `neo-manifest` (where it was historically re-exported
        from `neo-core`). Verify path and re-export.
      - `get_sign_data_vec` (8 occurrences) — moved to
        `neo-payloads::helper`. Update import paths.
      - `neo_crypto::KeyPair` (7 occurrences) — re-export or
        shim needed (the `KeyPair` type currently lives in
        `neo-wallets`).
      - `neo_execution::StorageItem` / `StorageKey` (4
        occurrences) — should be
        `neo_storage::{StorageItem, StorageKey}`.
      - `Wallet` trait (4 occurrences) — should be
        `neo_wallets::Wallet` or similar.
      - `AssetDescriptor` (2 occurrences) — find canonical home
        or define shim.
      - `neo_state_types::StateRoot` / `StateStore` (1
        occurrence) — types may need to be added or the
        `rpc_server_state.rs` import updated.
- [x] 9.3 All 6 lib tests pass with default features; the
      `server` feature is the only thing broken.
- [x] 9.4 The 4 KB C# JSON-RPC test vector file
      (`neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json`)
      is **not consumed** by any Rust test.

## 10. Write the verification report

- [x] 10.1 Write
      `openspec/changes/2026-06-08-protocol-correctness-verification/verification-report.md`
      with the per-area status table, the seven failing-test
      analysis, the `neo-rpc --features server` compile-error
      analysis, and the recommendations list.
