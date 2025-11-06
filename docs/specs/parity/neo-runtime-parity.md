# Neo Runtime Parity Checklist (Rust vs. C#)

## Summary

The Rust `neo-runtime` crate currently tracks block height, a toy block summary, and a synthetic transaction queue. The official C# ledger/runtime (`Neo.Ledger`, `Neo.SmartContract.Native`, `Neo.Persistence`) orchestrates block persistence, storage snapshots, contract execution, GAS accounting, policy/oracle logic, and integration with native contracts. This document captures the main capabilities we must add to reach parity while preserving idiomatic Rust architecture.

## Components to Map

1. **Blockchain Actor (`Neo.Ledger.Blockchain`)**
   - Block import/persist pipeline, verification, caching of unverified blocks.
   - Event hooks (`Committing`, `Committed`, `ApplicationExecuted`).
   - Memory pool coordination (`FillMemoryPool`, transaction re-verification).
   - Integration with Akka actors for async message handling.

2. **Store & Snapshot (`Neo.Persistence.*`)**
   - Data caches (`DataCache`, `StoreCache`), snapshot clone/dispose semantics.
   - Storage prefixes (blocks, transactions, contracts, storage items, headers).
   - Recovery log / state root handling.

3. **Native Contracts (`Neo.SmartContract.Native`)**
   - Execution of `OnPersist` / `PostPersist` scripts for NeoToken, GasToken, Policy, Oracle, Ledger, Role, StdLib, etc.
   - GAS and policy adjustments (fees, maximum block size, minimum gas, etc.).
   - Oracle/State root services (requests, attestations, Merkle proofs).

4. **Smart Contract Execution (`ApplicationEngine`)**
   - Trigger handling (Application, Verification, Oracle, System).
   - Resource limits, gas accounting, `ContractParametersContext`.
   - Engine integration with stored `Snapshot` and `ScriptContainer`.

5. **Transaction Processing**
   - Verification pipeline (`Verify` calls, signature checks, policy) and fee handling.
   - Mempool structure with prioritization, conflict checks, block assembly.
   - Double-spend prevention, contract call restrictions, Notary/Oracle features.

6. **State Roots / Storage**
   - State root commit/verification, `StateService` integration.
   - Patricia Merkle tree / `StorageItem` serialization, contract storage updates.
   - Persistent contract deployment/upgrade/destroy flows mirroring C#.

7. **Configuration / Settings**
   - Network magic, validators, block time, storage path, log settings (NeoSettings).
   - Policy adjustments (fee per byte, block size, exec time).

## Current Rust Gaps

- Ledger keeps only `height`, `committed: Vec<BlockSummary>`, `pending: VecDeque<PendingTransaction>` (`neo-runtime/src/blockchain.rs`).
- No persistence (`neo-store` only stubs `memory`/`sled` with minimal column definitions).
- No state storage, contract storage, GAS/policy/oracle logic.
- No native contract executions (`OnPersist`, `PostPersist`).
- No mempool prioritization or verification pipeline.
- No transaction verification context (signatures, fees, policy).
- No state roots, snapshots, or storage key prefixes.
- No event hooks for `ApplicationExecuted` or notifications.
- No integration with `neo-vm` or `neo-consensus` beyond toy interfaces.

## Implementation Plan

1. **Storage Schema & Snapshots**
   - Define storage prefixes matching C# (e.g., `System`, `CN`, `CS`, `ST`, `TR`, `BlockHashIndex`, etc.).
   - Implement `DataCache`, `StoreCache`, and snapshot mechanics in `neo-store`, ensuring copy-on-write semantics and proper disposal.
   - Add RocksDB (or sled-equivalent) columns for headers, blocks, transactions, contracts, storage items.

2. **Blockchain Orchestrator**
   - Introduce an async service (Tokio task or actor) modeling `Blockchain` responsibilities: block import, verification, caching, mempool coordination.
   - Provide events/callbacks (committing/committed) to native contract runtime.
   - Manage block file persistence and indexing.

3. **Mempool & Transaction Verification**
   - Implement mempool with fee prioritization, conflict detection, size/age limits.
   - Integrate verification pipeline (signature checks, policy, witness verification) using `neo-crypto` and `neo-vm`.
   - Track `TransactionVerificationContext` analogous to C#.

4. **Native Contract Engine**
   - Port native contract APIs (NeoToken, GasToken, Policy, Oracle, Ledger, Role, Management, StdLib).
   - Execute `OnPersist` / `PostPersist` phases with GAS accounting.
   - Manage GAS mint/burn, policy settings, oracle requests/resolves.

5. **Contract Execution Integration**
   - Wire `neo-vm` ApplicationEngine with snapshots, triggers, call flags, and script containers.
   - Ensure contracts can deploy/upgrade/destroy with storage updates.
   - Support contract events/notifications to be surfaced via runtime events.

6. **State Root / Oracle / Notary**
   - Implement state root service (Merkle Patricia tree, proofs).
   - Add oracle request/response processing and fees.
   - Handle notary services if targeted (optional milestone).

7. **Telemetry & Hooks**
   - Emit notifications similar to C# (ApplicationExecuted, block persisted).
   - Provide metrics for mempool size, block time, GAS consumption.
   - Expose runtime status to `neo-node` for RPC/CLI endpoints.

8. **Integration & Testing**
   - Build regression tests for block import/persist using C# generated fixtures.
   - Simulate full block execution with native contracts and verify state roots.
   - Add mempool/verification unit tests and concurrency stress tests.

## Deliverables & Milestones

1. Storage foundation (prefixes, snapshots, RocksDB support).
2. Blockchain orchestrator with block import/persist and simple mempool.
3. Transaction verification pipeline and contract execution tie-in.
4. Native contract set (NeoToken, GasToken, Policy, Oracle, StdLib).
5. State root/oracle/policy features.
6. Integration with consensus (block assembly, recovery) and P2P (block relay).

Each milestone should include parity tests against C# behavior (serialization, state transitions, GAS usage).

## Next Steps

- Finalize parity documents for networking/RPC, wallet/contract, and crypto crates.
- Agree on storage/contract data models before implementation to avoid churn.
- Establish shared test vectors generated from the C# node to validate Rust runtime behavior. 
