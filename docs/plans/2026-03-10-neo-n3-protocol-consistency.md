# Neo N3 Protocol Consistency Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align the Rust full node's P2P relay, peer discovery, and dBFT behavior with Neo N3 `master-n3` and `v3.9.1`.

**Architecture:** Fix the protocol surface in three slices. First, restore N3-correct wire semantics for inventory types, relay paths, and address propagation. Second, tighten peer state tracking so scheduling decisions match the C# node. Third, repair consensus recovery and view-change behavior so missing transactions, recovery packets, and timer-driven transitions follow DBFTPlugin semantics.

**Tech Stack:** Rust, Tokio, actor-based P2P runtime, Neo dBFT consensus, cargo test

### Task 1: P2P Inventory And Relay Semantics

**Files:**
- Modify: `neo-primitives/src/inventory_type.rs`
- Modify: `neo-core/src/network/p2p/remote_node/inventory.rs`
- Modify: `neo-core/src/network/p2p/local_node/actor.rs`
- Modify: `neo-core/src/ledger/blockchain/handlers.rs`
- Test: `neo-core/tests/p2p_message_tests.rs`
- Test: `neo-core/tests/p2p_payloads_csharp_tests.rs`
- Test: `neo-core/src/network/p2p/local_node/tests.rs`

**Step 1: Write the failing tests**

Add tests that prove:
- `0x2d` inventory type is rejected on N3 payload decode.
- `Inv(Extensible)` schedules `GetData` instead of being dropped.
- Accepted extensible payloads are re-relayed.
- Direct relay uses `inv` semantics instead of broadcasting full payloads.

**Step 2: Run test to verify it fails**

Run: `cargo test -p neo-core --test p2p_message_tests -- --test-threads=1`
Expected: FAIL in the new inventory / relay compatibility assertions.

**Step 3: Write minimal implementation**

Implement:
- Remove `Consensus` from the N3 inventory enum and reject it on deserialization.
- Treat `Extensible` as a normal fetchable inventory in remote-node handling.
- Re-relay accepted extensibles from blockchain handlers.
- Make local-node direct relay announce inventories instead of pushing full objects.

**Step 4: Run test to verify it passes**

Run: `cargo test -p neo-core --test p2p_message_tests -- --test-threads=1`
Expected: PASS

**Step 5: Commit**

```bash
git add neo-primitives/src/inventory_type.rs neo-core/src/network/p2p/remote_node/inventory.rs neo-core/src/network/p2p/local_node/actor.rs neo-core/src/ledger/blockchain/handlers.rs neo-core/tests/p2p_message_tests.rs neo-core/tests/p2p_payloads_csharp_tests.rs neo-core/src/network/p2p/local_node/tests.rs
git commit -m "fix: align p2p inventory and relay behavior with neo n3"
```

### Task 2: Peer Discovery And Height Tracking

**Files:**
- Modify: `neo-core/src/network/p2p/remote_node.rs`
- Modify: `neo-core/src/network/p2p/payloads/network_address_with_time.rs`
- Modify: `neo-core/src/network/p2p/task_session.rs`
- Test: `neo-core/src/network/p2p/local_node/tests.rs`
- Test: `neo-core/tests/p2p_payloads_csharp_tests.rs`

**Step 1: Write the failing tests**

Add tests that prove:
- peers without `TcpServer` are not advertised as dialable TCP nodes,
- `WsServer` is not treated as a TCP endpoint,
- peer height updates are monotonic.

**Step 2: Run test to verify it fails**

Run: `cargo test -p neo-core --test p2p_payloads_csharp_tests -- --test-threads=1`
Expected: FAIL in the new address/height assertions.

**Step 3: Write minimal implementation**

Implement:
- no fallback listener port synthesis in remote snapshots,
- no synthetic TCP capability injection for non-listening peers,
- `endpoint()` returns only TCP server endpoints,
- `TaskSession::update_last_block_index()` only raises height.

**Step 4: Run test to verify it passes**

Run: `cargo test -p neo-core --test p2p_payloads_csharp_tests -- --test-threads=1`
Expected: PASS

**Step 5: Commit**

```bash
git add neo-core/src/network/p2p/remote_node.rs neo-core/src/network/p2p/payloads/network_address_with_time.rs neo-core/src/network/p2p/task_session.rs neo-core/src/network/p2p/local_node/tests.rs neo-core/tests/p2p_payloads_csharp_tests.rs
git commit -m "fix: align peer discovery and height tracking with neo n3"
```

### Task 3: Consensus Recovery And View Change

**Files:**
- Modify: `neo-core/src/network/p2p/task_manager.rs`
- Modify: `neo-node/src/consensus.rs`
- Modify: `neo-consensus/src/service/handlers/change_view.rs`
- Modify: `neo-consensus/src/service/handlers/recovery.rs`
- Modify: `neo-consensus/src/messages/recovery.rs`
- Test: `neo-consensus/src/service/tests/recovery.rs`
- Test: `neo-consensus/src/service/tests/change_view.rs`
- Test: `neo-core/src/network/p2p/task_manager.rs`

**Step 1: Write the failing tests**

Add tests that prove:
- consensus-originated `RestartTasks` sends `GetData`,
- quorum-driven higher-view transition emits `ChangeAgreement`,
- repeated timer ticks do not rebroadcast same-view change/recovery messages,
- higher-view recovery messages continue processing prepare/commit payloads,
- recovery validation rejects out-of-range validator indices and invalid embedded payloads.

**Step 2: Run test to verify it fails**

Run: `cargo test -p neo-consensus recovery change_view -- --test-threads=1`
Expected: FAIL in the new recovery / view-change assertions.

**Step 3: Write minimal implementation**

Implement:
- sender-independent restart-task broadcast behavior for consensus recovery,
- DBFT `ChangeAgreement` emission before local view advance,
- timer/backoff behavior matching DBFTPlugin,
- full recovery-message application across change-view, prepare, and commit sections,
- stronger recovery-message validation and reverify path.

**Step 4: Run test to verify it passes**

Run: `cargo test -p neo-consensus recovery change_view -- --test-threads=1`
Expected: PASS

**Step 5: Commit**

```bash
git add neo-core/src/network/p2p/task_manager.rs neo-node/src/consensus.rs neo-consensus/src/service/handlers/change_view.rs neo-consensus/src/service/handlers/recovery.rs neo-consensus/src/messages/recovery.rs neo-consensus/src/service/tests/recovery.rs neo-consensus/src/service/tests/change_view.rs
git commit -m "fix: align dbft recovery and view changes with neo n3"
```

### Task 4: Final Verification

**Files:**
- Modify: none unless a verification failure exposes another protocol mismatch.

**Step 1: Run focused protocol verification**

Run: `cargo test -p neo-core --test p2p_message_tests -- --test-threads=1`
Expected: PASS

**Step 2: Run payload parity suite**

Run: `cargo test -p neo-core --test p2p_payloads_csharp_tests -- --test-threads=1`
Expected: PASS

**Step 3: Run consensus suites**

Run: `cargo test -p neo-consensus recovery change_view -- --test-threads=1`
Expected: PASS

**Step 4: Run node-level consensus checks**

Run: `cargo test -p neo-node consensus -- --test-threads=1`
Expected: PASS or documented unrelated failures.

**Step 5: Commit**

```bash
git add .
git commit -m "test: verify neo n3 protocol consistency fixes"
```
