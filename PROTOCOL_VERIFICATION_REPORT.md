# Neo N3 Protocol Correctness Verification Report

**Date:** 2026-06-08
**Workspace Commit:** b16e16e (`refactor(workspace): complete kill-neo-core + reth-style service architecture`)
**Node:** neo-rs v0.7.2
**Platform:** darwin

---

## Executive Summary

| Metric | Value |
|---|---|
| Crates in workspace | 46 |
| `lib` tests, pass / fail / ignored | **1,110 / 0 / 6** |
| Integration tests, pass / fail / ignored | **90 / 0 / 2** |
| C# interop test suite | **None** (`csharp_compatibility_tests.rs` does not exist; `mainnet_block_1000.hex` fixture is 65 NUL bytes and is not referenced from any test) |
| Reference C# test corpus in tree | 1 file (`neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json`, 46 RPC cases, **0 used by Rust tests**) |
| `neo-node` binary built | **Yes**, with `--features wip`; **No** by default (the daemon `main` is gated behind the `wip` Cargo feature and prints an error + `exit(1)` otherwise) |

**Bottom line:** the workspace contains 1,200 passing tests across well-factored crates (VM, execution, crypto, consensus, p2p, wire, storage, blockchain, payloads, mempool, primitives, state). Every test exercises the Rust code in isolation **against itself** — there is no test that round-trips a payload, block, MPT root, or RPC response against the C# `neo` node, the live network, or a known mainnet block (other than the synthetic `block #1000 hash` claim in `MAINNET-STATUS.md`, which is **not** reproducible from the fixture in this tree).

The codebase is a real implementation of the Neo N3 wire/disk/protocol surface (not stubs), **except for the eleven `neo-native-contracts` handles (NeoToken / GasToken / PolicyContract / OracleContract / StdLib / CryptoLib / Notary / RoleManagement / Treasury / ContractManagement / hashes)**, of which 10 are documented stubs (their own doc comments say "stub", their methods return zero/empty) and have **0 tests**. `LedgerContract` is the only real native-contract handle (read-only).

The full `neo-node` daemon compiles, prints `--help`, builds a `neo_system::Node`, and waits for `Ctrl-C`, but on a non-trivial path (e.g. a real network or persistent storage) it falls back to an in-memory store and panics with `integrate neo-storage-rocksdb for production use` if `--storage-path` is supplied.

---

## Test Results Summary

| Area | Tests | Pass | Fail | Ignored |
|---|---:|---:|---:|---:|
| Wire Protocol (`neo-wire`) | 11 | 11 | 0 | 0 |
| P2P envelope (`neo-p2p`) | 27 | 27 | 0 | 0 |
| VM (`neo-vm`) | 78 | 78 | 0 | 0 |
| Execution engine (`neo-execution`) | 23 | 23 | 0 | 0 |
| Crypto + MPT (`neo-crypto`) | 124 | 124 | 0 | 0 |
| Consensus dBFT (`neo-consensus`) | 101 | 101 | 0 | 1 |
| Blockchain service (`neo-blockchain`) | 21 | 21 | 0 | 0 |
| Mempool (`neo-mempool`) | 12 | 12 | 0 | 0 |
| State service (`neo-state-service`) | 14 | 14 | 0 | 0 |
| Storage (`neo-storage`) | 124 | 124 | 0 | 0 |
| Primitives (`neo-primitives`) | 221 | 221 | 0 | 0 |
| Payloads (`neo-payloads`) | 25 | 25 | 0 | 0 |
| RPC (`neo-rpc` lib) | 6 | 6 | 0 | 0 |
| RPC doc tests | 3 | 3 | 0 | 0 |
| RPC integration (`neo-rpc/tests/*.rs`) | 0 | 0 | 0 | 0 |
| Native contracts (`neo-native-contracts`) | 0 | 0 | 0 | 0 |
| Network service (`neo-network`) | 0 | 0 | 0 | 0 |
| P2P integration | 3 | 3 | 0 | 0 |
| Consensus integration | 18 | 17 | 0 | 1 |
| State integration | 18 | 18 | 0 | 0 |
| E2E transaction flow | 14 | 14 | 0 | 0 |
| End-to-end | 7 | 7 | 0 | 0 |
| Layer boundary | 11 | 11 | 0 | 0 |
| Contract execution | 12 | 11 | 0 | 1 |
| Chaos | 7 | 7 | 0 | 0 |
| Block persistence | 0 | 0 | 0 | 0 |
| **Workspace lib total** | **1,110** | **1,110** | **0** | **6** |
| **Integration total** | **90** | **90** | **0** | **2** |
| **Combined** | **1,200** | **1,200** | **0** | **8** |

---

## 1. Wire Protocol (msgpack/binary)

### 1.1 MessageHeader Format
**Status:** REAL, PARTIAL
- **Code:** `neo-wire/src/codec.rs` (`MessageCodec`, 166 LoC), `neo-wire/src/message.rs` (182 LoC).
- **Tests:** `codec::tests` (5 tests: partial-frame rejection, encode/decode ping, two-frames-in-one-buffer, oversized-payload rejection).
- **Evidence:** `cargo test -p neo-wire --lib` -> 11 / 0 / 0.
- **Gap:** No test against a known C#-emitted `Message` byte stream.

### 1.2 NetworkMessage Envelope
**Status:** REAL, PARTIAL
- **Code:** `neo-wire/src/network_message.rs` (126 LoC), `neo-wire/src/protocol_message.rs` (263 LoC).
- **Tests:** `network_message_round_trip_verack`, `network_message_round_trip_ping`, `protocol_message_command_matches_variant`, `empty_command_round_trip`, `ping_round_trip`.
- **Gap:** No C#-interop vector; `VersionPayload`/`AddrPayload`/`InvPayload`/`HeadersPayload`/`GetBlocksPayload`/`GetBlockByIndexPayload`/`MerkleBlockPayload`/`FilterAddPayload`/`FilterLoadPayload` round-trips exist only against themselves.

### 1.3 Block Serialization
**Status:** REAL, UNVERIFIED AGAINST C#
- **Code:** `neo-payloads::Block` (covered by 221 `neo-primitives` lib tests + 25 `neo-payloads` lib tests).
- **Tests:** `tests/tests/e2e_transaction_flow.rs::test_transaction_serialization_roundtrip` (tx, not block).
- **Gap:** No test serializes a real mainnet block through `Block::deserialize` and compares the hash. The single fixture (`tests/fixtures/mainnet_block_1000.hex`) is 65 NUL bytes (`0000…`) and is **not** imported by any `.rs` test.

### 1.4 Transaction Serialization
**Status:** REAL, UNVERIFIED AGAINST C#
- **Code:** `neo-payloads::Transaction` (covered by 25 payload tests).
- **Tests:** `test_transaction_serialization_roundtrip` (round-trips its own bytes).

---

## 2. Block Validation

### 2.1 Header Validation
**Status:** PARTIAL
- **Code:** `neo-blockchain/src/header_cache.rs`, `neo-blockchain/src/handlers.rs` (handlers test header gap acceptance / truncation).
- **Tests:** `header_cache::tests::{add_appends_to_tail, get_returns_matching_header, remove_up_to_drops_lower_indices, empty_cache_has_no_last}`, `handlers::tests::{headers_in_sequence_are_accepted, headers_with_gap_are_truncated}`.
- **Gap:** No test exercises a real `BlockHeader` byte stream with a witness signature and verifies it against the C# `Block.Verify()` semantics.

### 2.2 Transaction Validation
**Status:** PARTIAL
- **Code:** `neo-blockchain/src/transaction.rs` (120 LoC).
- **Tests:** `transaction::tests::{validate_transaction_returns_succeed, transaction_exists_on_chain_returns_false, conflict_exists_on_chain_returns_false}` — the last two return `false` because the storage backend is a stub. Validate-returns-`Succeed` is a smoke test only.

### 2.3 Witness Validation
**Status:** PARTIAL
- **Code:** `neo-p2p/src/witness_rule.rs` (in 27 p2p tests) and `neo-redeem-script` (4 tests).
- **Tests:** `witness_rule_projects_to_neo_vm_rs_stack_value`, `group_condition_accepts_uncompressed_ecpoint_and_normalizes_to_compressed`, `boolean_condition_json_matches_csharp_structure`, `protocol_enum_guard_rejects_unknown_witness_condition_type_bytes`, `protocol_enum_guard_rejects_unknown_witness_rule_action_bytes`.
- **Gap:** `boolean_condition_json_matches_csharp_structure` is the only test that names C# in its assertion message. No test signs a real transaction and checks the witness is accepted.

---

## 3. Transaction Execution

### 3.1 ApplicationEngine Core
**Status:** REAL, PARTIAL
- **Code:** `neo-execution/src/application_engine_{runtime,storage,contract,helper,iterator,op_code_prices}.rs` (2,481 LoC) plus `application_engine/` subdir, `native_registry.rs` (538 LoC), `native_contract_cache.rs`, `native_contract_provider.rs`.
- **Tests:** 23 lib tests: `application_engine::contracts::tests::call_contract_uses_execution_state_script_hash_for_caller`, `application_engine_contract::tests::decode_native_result_*`, `contract_state::tests::contract_state_{read,project,stack_item_projection}_*`, `native_registry::tests::test_native_registry_starts_empty`.
- **Gap:** Native contracts are stubs (see §3.3). `ApplicationEngine::execute` cannot run a real NEO/GAS transfer end-to-end through the production native contract paths.

### 3.2 Opcodes
**Status:** REAL
- **Code:** `neo-vm/src/jump_table/` — `numeric.rs` (459 LoC), `compound.rs` (743 LoC), `control.rs` (499 LoC), `push.rs` (234 LoC), `slot.rs` (245 LoC), `splice.rs` (202 LoC), `stack.rs` (294 LoC), `types.rs` (164 LoC), `bitwisee.rs`.
- **Tests:** 78 VM lib tests + 12 `contract_execution` integration tests (arithmetic, gas, stack underflow, push/return).
- **Gap:** No C# VM corpus used as a fixture. Coverage of opcodes is by in-tree unit tests only.

### 3.3 Native Contracts
**Status:** STUBS, 0 TESTS

| Native | File | LoC | Test count | Real? |
|---|---|---:|---:|---|
| `NEO_TOKEN` (NeoToken) | `neo-native-contracts/src/neo_token.rs` | 44 | 0 | **Stub** (file doc: "NeoToken (NEO) native contract stub … returns empty/zero values from every storage query") |
| `GAS_TOKEN` (GasToken) | `gas_token.rs` | 32 | 0 | **Stub** |
| `PolicyContract` | `policy_contract.rs` | 77 | 0 | **Stub** (only constants + hash accessor) |
| `OracleContract` | `oracle_contract.rs` | 108 | 0 | **Stub** + `OracleRequest` struct |
| `LedgerContract` | `ledger_contract.rs` | 294 | 0 | **Real (read-only)** — file doc: "Concrete (non-stub) implementation … query surface" |
| `ContractManagement` | `contract_management.rs` | 141 | 0 | **Stub** |
| `CryptoLib` | `crypto_lib.rs` | 32 | 0 | **Stub** |
| `Notary` | `notary.rs` | 32 | 0 | **Stub** |
| `RoleManagement` | `role_management.rs` | 50 | 0 | **Stub** |
| `StdLib` | `std_lib.rs` | 32 | 0 | **Stub** |
| `Treasury` | `treasury.rs` | 32 | 0 | **Stub** |
| `hashes` | `hashes.rs` | 85 | 0 | Constants only |

**This is the largest single gap in the workspace.** A `cargo test -p neo-native-contracts --lib` returns **0 tests, 0 failures, 0 ignored**.

---

## 4. State Transitions (MPT)

### 4.1 MPT Insert / Delete
**Status:** REAL, UNVERIFIED AGAINST C#
- **Code:** `neo-crypto/src/mpt_trie/` (2,822 LoC: `trie.rs` 869, `node.rs` 490, `tests.rs` 1,187, `cache.rs` 172).
- **Tests:** 65 `mpt_trie::tests::mpt_tests::*` covering branch / extension / leaf / empty / cache / reference / delete / serialize / put.

### 4.2 State Root Calculation
**Status:** REAL
- **Code:** `neo-state-service/src/state_root.rs` (`unsigned_bytes_round_trip`, `state_root_hash_is_stable`, `different_roots_yield_different_hashes`); `neo-blockchain/src/header_cache.rs` (merkle root).
- **Tests:** 14 `neo-state-service` lib tests; 7 `end_to_end_tests` integration tests (state root determinism, MPT performance, merkle root, block hash).

### 4.3 MPT Compatibility with C#
**Status:** NOT TESTED
- The C# `Neo.SmartContract` reference is not in the tree (the only `neo_csharp/` content is the RPC plugin JSON). No MPT fixture file is checked in. No test asserts `root_hash` against a known C# value for a known mainnet block.

---

## 5. Network / P2P

### 5.1 Handshake
**Status:** REAL, NOT EXERCISED
- **Code:** `neo-p2p/src/payloads/version_payload.rs`, `neo-wire/src/protocol_message.rs::ProtocolMessage::Version`.
- **Tests:** Round-trip only. No test negotiates `Version` / `Verack` between two endpoints.

### 5.2 Message Encoding
**Status:** REAL, NOT EXERCISED
- **Code:** `neo-p2p::MessageCommand` (27 p2p tests cover all 18+ commands and `MessageFlags`).
- **Tests:** `p2p_message_exchange::message_command_byte_conversion_round_trips`, `verify_result_variants_round_trip`, `message_flags_compression_predicate` (3/3 in integration).

### 5.3 Block Sync
**Status:** NOT EXERCISED
- **Code:** `neo-blockchain/src/handlers.rs` (290 LoC, `dispatch_command_variants_is_exhaustive` test exists), `neo-blockchain/src/import.rs` (22 LoC), `neo-blockchain/src/header_cache.rs` (149 LoC), `neo-network/src/remote_node.rs` (312 LoC, **0 tests**).
- **Gap:** `neo-network` has 0 lib tests. `import.rs` / `persist_completed.rs` / `preverify_completed.rs` are 7–25 LoC stubs.

---

## 6. Consensus (dBFT)

### 6.1 Message Exchange
**Status:** REAL
- **Code:** `neo-consensus/src/messages/` (PrepareRequest 188 LoC + tests, PrepareResponse 184 + tests, Commit 188 + tests, ChangeView 78 + tests, Recovery 99 + tests); `neo-consensus/src/service/tests/{prepare,recovery,change_view,core,persist,helpers}.rs` (2,049 LoC).
- **Tests:** 101 lib + 18 integration.
- **Coverage:** `consensus_merkle_root_matches_core_merkle_tree`, `recovery_request_broadcasts_recovery_message`, `multi_validator_prepare_response_collection`, `primary_requests_transactions_on_start`, `transactions_received_triggers_prepare_request`, `timeout_triggers_view_change`, `consensus_handles_empty_transaction_list`, `count_failed_threshold` / `count_committed` / `more_than_f_nodes_*` / `f_and_m_calculations`, `message_cache_*` (replay, clear on new block, LRU), `save_atomic_write` / `load_corrupted_file` / `load_nonexistent_file`.

### 6.2 Block Production
**Status:** REAL, NOT WIRED
- **Code:** `neo-consensus/src/service/proposal.rs` issues a real `PrepareRequestMessage` with a real merkle root, real `next_consensus` script hash (`compute_next_consensus_address`), and real `compute_header_hash` (real SHA256 of the 8-field header). `service/block_data.rs` returns a `BlockData` struct.
- **Gap:** No test exercises a 4-validator dBFT round to a committed `Block`. The proposal flow is tested up to broadcasting the `PrepareRequest` event; the actual signing -> recovery -> `Block` assembly is covered only by `recovery_message_roundtrip_minimal_without_prepare_request` and `compute_merkle_root` / `compute_header_hash` helpers.

---

## 7. Cryptographic Compatibility

### 7.1 MPT
**Status:** REAL, NOT C#-VECTORED — see §4.3.

### 7.2 BLS12-381
**Status:** REAL
- **Code:** `neo-crypto/src/bls12381.rs` (covered by `invalid_private_key_scalars_are_rejected`).
- **Tests:** `cargo test -p neo-crypto --lib` -> 124 / 0 / 0.
- **Gap:** No test against the IETF BLS12-381 draft / known test vectors.

### 7.3 Secp256k1 / Secp256r1 / Ed25519
**Status:** REAL
- **Code:** `neo-crypto/src/ecc.rs` (890 LoC), `neo-crypto/src/signature.rs` (665 LoC), `neo-crypto/src/named_curve_hash.rs`.
- **Tests:** `test_ec_curve_sizes`, `test_ec_point_*` (8 tests), `test_verify_ed25519_signature`, `bip32::tests::*` (3 tests).
- **Gap:** No Wycheproof / RFC 6979 / BIP-32 / secp256k1 test-vector corpus.

### 7.4 Hashes
**Status:** REAL
- **Code:** `neo-crypto/src/hash.rs` (543 LoC), `neo-crypto/src/murmur.rs`.
- **Tests:** `test_hash160`, `test_blake2b{,_256,_512,_s}`, `test_constant_time_*`.
- **Gap:** No test against a known Neo N3 mainnet transaction hash.

---

## 8. RPC

### 8.1–8.3 Methods
**Status:** REAL HANDLERS, NEAR-ZERO TEST COVERAGE

- **Code:** `neo-rpc/` — server subdirs for `application_logs`, `blockchain`, `node`, `oracle`, `state`, `tokens_tracker`, `wallet`; client subdirs for `nep17_api`, `policy_api`, `state_api`, `wallet_api`, `rpc_client`, `models`.
- **Lib tests:** 6 (`error_code::tests::test_{from_code,display,is_standard,message,standard_error_codes,neo_error_codes}`).
- **Integration tests:** 6 files, **0 tests each** (`jsonrpsee_adapter.rs`, `rate_limiter_governor.rs`, `rpc_blockchain_getrawtransaction_vmstate.rs`, `rpc_handler_registration.rs`, `validate_address.rs`, `ws_events.rs`). 3 doc-tests pass.
- **C# interop:** The 46 RPC test cases in `neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json` are **not consumed by any test** in the Rust tree. `cargo test --workspace --test csharp_compatibility_tests` reports `error: no test target named 'csharp_compatibility_tests'`; the file does not exist.
- **Coverage of the 46 C# RPC methods in the Rust test suite:** 0.

| Method group | Rust impl | Rust test |
|---|---|---|
| `getbestblockhash` / `getblockcount` / `getblockhash` | yes | none |
| `getblock` / `getblockheader` / `getblockheaderhex` | yes | none |
| `getblocksysfee` / `getconnectioncount` / `getpeers` | yes | none |
| `getcontractstate` / `getnativecontracts` | yes | none |
| `getnep17balances` / `getnep17transfers` | yes | none |
| `getrawmempool` / `getrawtransaction` / `getrawtransactionhex` | yes | 1 ignored doc-test (server feature only) |
| `getstorage` / `getapplicationlog` | yes | none |
| `getnextblockvalidators` / `getcommittee` | yes | none |
| `getunclaimedgas` / `getversion` | yes | none |
| `invokefunction` / `invokescript` | yes | none |
| `sendrawtransaction` / `submitblock` | yes | none |
| wallet methods (12) | yes | none |
| `listplugins` | yes | none |

---

## 9. Network Formation

### 9.1 Can the node start?
**Status:** PARTIAL
- `cargo build -p neo-node` (default) produces a stub `main` that prints an error and `exit(1)`.
- `cargo build -p neo-node --features wip` (or `--features full`) succeeds in 0.35 s.
- `target/debug/neo-node --help` prints the help message.
- **Default behaviour is intentional:** the default `main` exists solely to keep `cargo build --workspace` green while the full daemon is in flight. The full `node::run()` parses CLI, loads `ProtocolSettings` (JSON, falls back to `ProtocolSettings::default()` if the file is missing), builds a `neo_system::Node`, spawns services, and waits for `Ctrl-C`.

### 9.2 Can it accept incoming connections?
**Status:** NOT VERIFIED
- The build path is verified; **no test** in the workspace binds a TCP listener on port 10333 and exchanges a single byte with the daemon. `neo-network/src/local_node.rs` (383 LoC) and `remote_node.rs` (312 LoC) are not covered by any test.

### 9.3 Can it sync from a C# node?
**Status:** NOT VERIFIED LOCALLY
- `MAINNET-STATUS.md` claims a deployed node at `89.167.120.122` reached block 21,373 with 10 active peers and that block #1000 hash `0xe31ad93809a2ac112b066e50a72ad4883cf9f94a155a7dea2f05e69417b2b9aa` was verified — **but** the only fixture in the tree (`tests/fixtures/mainnet_block_1000.hex`) is 65 NUL bytes, no test in the tree computes or asserts that hash, and the production server is unreachable from this sandbox. The C# node tree in `neo_csharp/` contains only an `RpcServer.json` config and a `RpcTestCases.json` fixture — no buildable C# solution.

---

## 10. Honest Assessment

### Real vs. stub (workstream-by-workstream)

| Area | Real? | Tested vs. C#? | Known gap |
|---|---|---|---|
| Wire envelope | Real | No | No real payload byte streams |
| Block / tx payloads | Real | No | No real mainnet block round-trip |
| Headers / blockchain | Real | No | `import.rs` / `persist_completed.rs` / `preverify_completed.rs` are 7–25 LoC stubs |
| Mempool | Real | No | No live tx traffic |
| dBFT consensus | Real (messages, recovery, view-change) | No | No end-to-end round producing a `Block` |
| VM | Real (3,556 LoC + jump tables 3,342) | No | No C# VM corpus |
| Execution engine | Real surface (6,276 LoC) | No | Native contracts are stubs (§3.3) |
| Native contracts | **10 of 11 are documented stubs** | No | Largest single gap |
| MPT | Real (2,822 LoC) | No | No C# MPT root vectors |
| Crypto (BLS, ECC, hashes) | Real | No | No RFC / Wycheproof vectors |
| RPC server | Real (handlers, registry, jsonrpsee adapter) | **No** | 6 integration files have 0 tests; 46 C# test cases unused |
| RPC client | Real (nep17, policy, state, wallet, models) | No | Same |
| P2P network service | Real (`local_node.rs` 383, `remote_node.rs` 312, `task_manager.rs` 295) | No | 0 lib tests in `neo-network` |
| Node daemon | Real but gated behind `wip` | No | `--storage-path` panics; only in-memory store is wired |

### What the workspace genuinely proves

1. Every public enum/struct round-trips its own `serialize`/`deserialize` (wire, payloads, p2p messages, consensus messages).
2. The dBFT 2.0 message-recovery / view-change / prepare-commit state machine is correctly implemented at the message-handling level.
3. The MPT trie handles 1,000 keys deterministically and the state root verifier rejects mismatched claims.
4. The VM executes jump-table opcodes through `interpret` and tracks gas up to a configured limit.
5. The storage backend (in-memory) and key-builder match the C# prefix/length semantics.
6. The dependency graph is acyclic and respects the Layer 0–4 contract.

### What the workspace does **not** prove

1. That any byte produced by `neo-rs` would be accepted by a C# `neo` node.
2. That the block / transaction / state hash for any real mainnet block round-trips.
3. That a single NEP-17 transfer (GAS, NEO) executes to the same state root under both implementations.
4. That a multi-sig witness verifies under either implementation's `CheckMultisig` syscall.
5. That the RPC interface returns the same shape / value as the C# `RpcServer` for any of the 46 known `RpcTestCases.json` cases.
6. That the node can connect to the live mainnet and progress beyond the first few thousand blocks.

---

## Recommendations (work needed to reach 100% protocol compliance)

1. **Implement the 10 native-contract stubs** (`NeoToken`, `GasToken`, `PolicyContract`, `OracleContract`, `ContractManagement`, `CryptoLib`, `Notary`, `RoleManagement`, `StdLib`, `Treasury`) and add at least one transfer / mint / policy test per contract.
2. **Add a `csharp_compatibility_tests.rs` integration test** that loads `tests/fixtures/mainnet_block_1000.hex` (currently 65 NUL bytes — replace with a real mainnet block 1,000 hex), `Block::deserialize`s it, asserts the header hash matches `0xe31ad…b2b9aa`, and asserts the MPT root matches the C# node's reported root.
3. **Add a JSON-driven `RpcTestCases` harness** that consumes `neo_csharp/node/tests/Neo.Network.RPC.Tests/RpcTestCases.json` and asserts Rust responses match the C# responses.
4. **Promote the `wip` feature to default** and add a smoke test that spawns `neo-node` in-process, hits `getblockcount` over JSON-RPC, and asserts a response.
5. **Add network tests for `neo-network`** (LocalNode bind + accept; RemoteNode handshake with a fake peer emitting `Version`/`Verack`).
6. **Add a C#-compatible MPT vector** to `neo-crypto/src/mpt_trie/tests.rs`.
7. **Add BLS12-381 draft-04 test vectors** and **RFC 6979 / BIP-32 vectors** to `neo-crypto`.
8. **Promote `block_persistence.rs` from empty** (currently a 7-line doc-comment) and `import.rs` / `persist_completed.rs` from stubs.

### Estimated effort to reach "production-grade protocol compliance"

| Item | Effort | Confidence |
|---|---|---|
| Wire round-trip with C# `Message` byte stream (1 vector) | 1–2 days | High |
| Mainnet block 1,000 round-trip + MPT root | 2–3 days | High |
| Implement 10 native contracts (real, not stub) | 4–6 weeks | Medium |
| `RpcTestCases.json` conformance harness | 2 weeks | Medium |
| 4-validator dBFT end-to-end producing a `Block` | 1 week | High |
| Persistent storage path (RocksDB) + restart/resume | 1–2 weeks | High |
| BLS12-381 / RFC 6979 / BIP-32 test vectors | 1 week | High |
| **`TOTAL` to 100% C# wire/ledger parity (no native contracts)** | **6–10 weeks** | |
| **To production-ready (incl. native contracts + mainnet sync)** | **3–6 months** | |

---

## Files Created / Modified

This verification run created exactly one new file:

- `PROTOCOL_VERIFICATION_REPORT.md` (this file, at workspace root)

No code or test was modified.

### Artifacts inspected (read-only)

- `Cargo.toml` (workspace + 46 sub-crates)
- `tests/Cargo.toml`, `tests/src/lib.rs`, `tests/tests/*.rs` (9 files, 2,070 LoC)
- `neo-csharp/` (2 JSON fixtures, 0 source)
- `tests/fixtures/mainnet_block_1000.hex` (65 NUL bytes)
- `MAINNET-STATUS.md` (94 lines)
- Every `Cargo.toml` in the 46 sub-crates
- Source trees of `neo-vm`, `neo-execution`, `neo-crypto`, `neo-consensus`, `neo-blockchain`, `neo-mempool`, `neo-p2p`, `neo-wire`, `neo-storage`, `neo-state-service`, `neo-network`, `neo-rpc`, `neo-native-contracts`, `neo-node`, `neo-system`
