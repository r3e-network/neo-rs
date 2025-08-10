### Overview

This document tracks all current “In production, this would …” placeholders and outlines the concrete implementation plan to replace them. We will remove placeholders incrementally, prioritizing mainnet-critical paths (consensus, ledger, persistence, network health, VM correctness). Each entry lists the file/location, the current placeholder intent, and the planned real behavior.

### High-priority placeholders (consensus/ledger/vm/network)

- crates/consensus/src/service.rs
  - Status: DONE — balance lookup wired to ledger snapshot (GAS), clamped to u64

- crates/consensus/src/dbft/engine.rs
  - Status: DONE — mempool adapter integration in place; no mock tx fabrication

- crates/ledger/src/block/block.rs (validation paths)
  - Status: PARTIAL — multisig witness m-of-n implemented with p256, further contract-verify integration pending

- crates/ledger/src/block/verification.rs
  - Status: PARTIAL — minimal `ApplicationEngine` (Verification) provided; designated validators fallback to committee-derived set; TODO: invoke RoleManagement.getDesignatedByRole via snapshot

- crates/ledger/src/blockchain/persistence.rs
  - Placeholder: transaction loading/indexing by block
  - Plan: implement column family index and prefix scans within `neo_persistence` RocksDB module

- crates/ledger/src/blockchain/state.rs
  - Placeholder: RocksDB queries and iteration
  - Plan: use `neo_persistence` abstractions; implement iterators with well-defined prefixes and limits

- crates/network/src/peers.rs
  - Status: PARTIAL — ban info with expiry and async discovery kick implemented; TODO: DNS seeds and GetAddr broadcast

- crates/persistence/src/migration.rs
  - Placeholder: migration script parsing/execution/rollback
  - Plan: define migration DSL; implement planner/executor with idempotency and crash-safe checkpoints

- crates/persistence/src/rocksdb/mod.rs
  - Status: DONE — snapshot commit retry with exponential backoff

- crates/vm/src/execution_context.rs
  - Placeholder: exception flags bookkeeping
  - Plan: model explicit exception state transitions; expose to debugger hooks

- crates/vm/src/jump_table/control/storage.rs
  - Status: DONE — storage context extraction implemented via downcast

### Additional placeholders

- crates/consensus/src/proposal.rs: Status: DONE — fallback to first signer account when script hash missing
- crates/cryptography/src/crypto.rs: real ECDSA context/state
- crates/smart_contract/src/native/neo_token.rs: balance/supply metrics updates
- crates/smart_contract/src/native/role_management.rs: Designation event emission
- node/src/main.rs: storage repair hooks on failure
- node/src/vm_integration.rs: robust Any→StackItem conversion
- crates/vm/tests/application_engine_tests.rs: storage ops success path note

### Implementation approach

1) Wire read-only behaviors to live state (ledger snapshot, RocksDB) where safe and deterministic.
2) Replace mock return values with validated queries; propagate typed errors.
3) Add tests mirroring C# behaviors for each replaced placeholder.
4) Update this audit on each edit; remove entries when production behavior is implemented and covered.

### Current status

- neo-vm clippy/test cleanup in progress to ensure a green baseline before invasive changes.
- Next up: consensus service balance lookup → ledger snapshot; network peer ban metadata → TTL logic.


