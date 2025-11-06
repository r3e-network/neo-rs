# Neo Consensus Parity Checklist (Rust vs. C#)

## Summary

The Rust `neo-consensus` crate currently models only the simplest dBFT message flow (prepare/commit/change-view) and quorum accounting. The official C# implementation (DBFTPlugin / `ConsensusContext`, `ConsensusService`, `RecoveryMessage`, etc.) includes rich message types, view-change recovery, payload caching, signature aggregation, timers, and persistence that enable interoperability with the Neo network. This document outlines the gaps and an implementation plan to reach parity while keeping Rust idioms.

## Components to Map

1. **Consensus Messages**
   - `PrepareRequest` / `PrepareResponse` / `Commit` / `ChangeView`.
   - Recovery messages (`RecoveryMessage`, `PrepareRequest2`, `ChangeViewPayloadCompact`) and extended payloads (block data, signatures, next consensus, timestamp).
   - Message serialization format (`ConsensusPayload`, `ConsensusMessage`), including validators index, view number, block height.

2. **Consensus Context (C# `ConsensusContext.cs`)**
   - Block header assembly, transaction list, Merkle root computation.
   - Validator list, F/M calculations, primary/backup roles, view number tracking.
   - Payload caches (`PreparationPayloads`, `CommitPayloads`, `ChangeViewPayloads`, `LastChangeViewPayloads`).
   - Transaction verification context, witness size, and snapshot handling.
   - Recovery log persistence (loading/saving consensus state).

3. **Consensus Service (`ConsensusService.cs`)**
   - Timer management for view change and block timeout.
   - Message processing pipeline (`OnCommitReceived`, `OnPrepareResponseReceived`, `OnChangeViewReceived`, recovery handlers).
   - Signature aggregation and multisig witness creation.
   - Transaction pool fetching, verification, re-broadcast.
   - Diagnostics and consensus events.

4. **Extensible Payload (`Neo.Network.P2P.Payloads.ExtensiblePayload`)**
   - Category `dBFT`, witness structure, serialization, signature verification.
   - `ConsensusPayload` structure with `ValidatorIndex`, `ViewNumber`, `InvocationScript`, `VerificationScript`.

5. **Networking & Integration**
   - Message broadcasting via P2P layer (`Inventory` or dedicated consensus messages).
   - Recovery message distribution when nodes miss payloads.
   - State persistence across restart (`ConsensusStateKey`, `RecoveryLogs`).

6. **Error Handling / Logging**
   - Equivalents for `ConsensusError`, state rollback, logging policies.

## Current Rust Implementation Gaps

- **Messages:** `neo-consensus/src/message.rs` only defines `PrepareRequest`, `PrepareResponse`, `Commit`, `ChangeView`. No recovery messages, no payloads carrying block data, signatures, or next consensus information.
- **State machine:** `ConsensusState` tracks participation counts but lacks timer logic, payload caches, transaction verification context, and support for multiple views/blocks. No persistence of consensus state to storage (besides simple snapshot).
- **Signature handling:** No aggregation of commit signatures into multisig witness; no integration with block header witness creation.
- **Recovery flow:** No handling of nodes that missed prepare responses or commit messages; no `RecoveryMessage` or `PrepareRequest2`.
- **Timers:** No block timeout / view-change timers, no handler for timeouts or manual change view triggers.
- **Networking:** `DbftEngine` does not integrate with P2P payloads; messages are not wrapped in `ExtensiblePayload` nor broadcast.
- **Persistence:** Snapshot support is limited; no recovery logs or persistent state using storage column `ConsensusStateKey`.
- **Validator context:** Lacks view-change logic across multiple validators, F/M calculations, `LastSeenMessage`, or `MoreThanFNodesCommittedOrLost` heuristics.
- **Diagnostics:** No logging of consensus stages, no metrics (only counts). Recent work added change-view reason tracking to snapshots/telemetry, but other diagnostics (timer lag, recovery hints) remain missing.

## Recently Implemented

| Feature | Rust Status | Notes |
| --- | --- | --- |
| Change-view reason tracking | ✅ Implemented | Reasons captured in `ConsensusState` snapshots, with per-reason counters exposed via node/CLI telemetry. |

## Rust Implementation Plan

1. **Message Layer**
   - Create Rust equivalents of `ConsensusMessage` variants from C# (including `RecoveryMessage`, `PrepareRequest2`, `ChangeViewPayloadCompact`).
   - Define serialization/deserialization matching `ConsensusPayload` and `ExtensiblePayload`.
   - Introduce struct for `ConsensusPayload` with validator index, view number, and witness scripts.

2. **State Context**
   - Implement `ConsensusContext` struct storing block info, payload caches, transaction hashes, verification context, view number, timers.
   - Add F/M calculations, `LastSeenMessage`, fail/commit counts, `MoreThanFNodesCommittedOrLost`.
   - Provide methods to create blocks, reassemble transactions, compute merkle root, and manage view changes.
   - Integrate persistence via `neo-store` using the same `ConsensusStateKey`.

3. **Service Layer**
   - Build `ConsensusService` (async task or actor) handling timers, message processing, and broadcasting.
   - Add handlers for each message type, including recovery flow and manual change view.
   - Hook into transaction pool (from `neo-runtime`) for selecting and verifying transactions.
   - Manage `ContractParametersContext` (multisig) for commit witness.

4. **Interop with P2P**
   - Wrap consensus messages into `ExtensiblePayload` with `Witness` similar to C#.
   - Broadcast via P2P (requires `neo-node` integration).
   - Handle inbound payload verification (signature checks, view validations).

5. **Timers and Configuration**
   - Implement consensus timers (block timer, view timer) with configurable `TimePerBlock`, `MinimumBlockTime`.
   - Support manual change view requests and plugin-style configuration.

6. **Testing & Parity Validation**
   - Port C# dBFT unit tests or create new ones that simulate a multi-node environment.
   - Add golden serialization tests for messages/payloads using C# reference outputs.
   - Build integration tests with simulated validators to ensure consensus reaches commit.

7. **Diagnostics**
   - Add structured logging, metrics (committed nodes, view changes, failures).
   - Provide hooks for CLI/telemetry to inspect consensus status similar to C# `consensus` RPC.

## Deliverables

- `neo-consensus` crate with full message set, context, service, and recovery support.
- Integrations with `neo-runtime` for block creation, `neo-p2p` for message propagation, and `neo-store` for persistence.
- Test harness comparing consensus behavior with C# dBFT plugin.
- Documentation describing any intentional divergences and providing developer onboarding instructions.

## Next Steps

- Produce similar parity documents for runtime/storage, networking/RPC, wallet/contract, and crypto.
- Once specs are approved, break implementation into milestones (message layer, context, service, integration) with corresponding PRs and tests. 
