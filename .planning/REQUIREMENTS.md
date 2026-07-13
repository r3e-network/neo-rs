# Requirements: neo-rs Production Node

**Defined:** 2026-07-13
**Core Value:** Import, validate, execute, persist, and expose canonical Neo N3 state exactly as Neo v3.10.1 does.

## v1 Requirements

### Protocol Baseline

- [x] **PROTO-01**: Canonical execution uses only hardfork-aware semantics proven against Neo v3.10.1, with every consensus-sensitive external dependency pinned to an immutable revision.
- [ ] **BUILD-01**: A clean checkout passes locked workspace, fuzz, container, and dependency-policy builds without a sibling repository or undeclared local input.
- [x] **CONSENSUS-01**: State-root votes aggregate only when version, block index, and root hash all match, with adversarial tests for every competing identity.

### Storage and Lifecycle

- [ ] **STORAGE-01**: Backend read and snapshot-open failures propagate as errors and abort canonical mutation instead of appearing as absent state or empty iteration.
- [ ] **STORAGE-02**: Every persistent store validates network magic, genesis hash, schema version, and store role before reading or mutating chain data.
- [ ] **OPS-01**: RPC and P2P startup failures abort node startup, and graceful shutdown joins critical tasks before the final durable flush.

### Differential Compatibility

- [ ] **PROTO-02**: A versioned differential corpus compares consensus-critical behavior with official Neo v3.10.1 immediately before and after every scheduled MainNet and TestNet hardfork.
- [ ] **VM-01**: Script results, faults, fees, notifications, and storage writes match Neo v3.10.1 under the active hardfork rules.
- [ ] **NATIVE-01**: Native-contract state transitions and externally observable results match Neo v3.10.1 using a documented canonical comparison encoding.

### Networking and Canonical Sync

- [ ] **P2P-01**: The node exchanges Neo N3 v3.10.1 handshakes, addresses, headers, inventories, requests, blocks, transactions, and extensible payloads with real peers.
- [ ] **SYNC-01**: Header and block synchronization remains ordered, bounded, restartable, and advancing under disconnects, malformed peers, slow peers, and backpressure.
- [ ] **REORG-01**: Fork and reorganization handling never violates canonical mutation ownership or leaves persisted state inconsistent with the selected chain.
- [ ] **VALIDATION-01**: Every imported block passes policy-aware, stateful validation inside the ordered canonical command loop before execution or persistence.
- [ ] **VALIDATION-02**: Peer-import validity matches Neo v3.10.1, preserves authoritative header witness verification, and never treats producer-only transaction-count policy as peer hard validity.

### Full MainNet Replay

- [ ] **REPLAY-01**: Starting from an empty database, the node executes every transaction-bearing MainNet block from genesis to a recorded tip through the normal canonical import path.
- [ ] **STATE-01**: Retained reports prove block hashes, transaction outcomes, fees, notifications, and state roots match trusted Neo v3.10.1 references at hardfork boundaries and deterministic samples.
- [ ] **RECOVERY-01**: An interrupted replay resumes from the last atomic commit and reaches the same final state root as an uninterrupted replay.

### Verified Fast Sync

- [ ] **FASTSYNC-01**: Checkpoint manifests and state content are authenticated with an explicit trust policy and strong cryptographic hashes; MD5 is never accepted as authenticity proof.
- [ ] **FASTSYNC-02**: Checkpoint state is network/schema checked, root verified, staged, and atomically promoted, then all later blocks execute through normal ordered validation.
- [ ] **SECURITY-01**: Corrupt, equivocated, stale, wrong-network, truncated, and unavailable checkpoint inputs fail closed with actionable diagnostics and no partial installation.

### Production Release

- [ ] **SECURITY-02**: Fuzz, property, concurrency, crash-recovery, resource-exhaustion, dependency audit, SBOM, and threat-model gates pass with retained evidence.
- [ ] **PERF-01**: Reproducible profiles and benchmarks demonstrate bounded resource use and measured sync, execution, persistence, and checkpoint performance.
- [ ] **RELEASE-01**: Reproducible release artifacts and operator documentation cite full replay, live interoperability, soak, recovery, and fast-sync evidence for every production claim.

## v2 Requirements

No requirements are deferred to a later milestone. Any production-readiness gap
discovered during implementation must be added to this milestone or explicitly
accepted through the milestone audit.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Rewrite the node from scratch | This is a brownfield correctness and architecture program; replacement would discard tested behavior and delay parity evidence. |
| Replace Neo semantics with reth or Substrate semantics | Neo v3.10.1 is the protocol authority; other nodes are architecture references only. |
| Treat block-production policy as generic peer-import validity | Neo compatibility requires preserving the distinction between producer policy and consensus validity. |
| Trust an optimized VM, downloaded archive, or reference RPC implicitly | Every trust boundary requires explicit authentication or differential evidence. |
| Cosmetic crate reshuffling | Structural changes must improve correctness, verification, operability, or measured maintainability. |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| PROTO-01 | Phase 1 | In Progress |
| BUILD-01 | Phase 1 | In Progress |
| CONSENSUS-01 | Phase 1 | In Progress |
| STORAGE-01 | Phase 2 | Pending |
| STORAGE-02 | Phase 2 | Pending |
| OPS-01 | Phase 2 | Pending |
| PROTO-02 | Phase 3 | Pending |
| VM-01 | Phase 3 | Pending |
| NATIVE-01 | Phase 3 | Pending |
| P2P-01 | Phase 4 | Pending |
| SYNC-01 | Phase 4 | Pending |
| REORG-01 | Phase 4 | Pending |
| VALIDATION-01 | Phase 4 | Pending |
| VALIDATION-02 | Phase 4 | Pending |
| REPLAY-01 | Phase 5 | Pending |
| STATE-01 | Phase 5 | Pending |
| RECOVERY-01 | Phase 5 | Pending |
| FASTSYNC-01 | Phase 6 | Pending |
| FASTSYNC-02 | Phase 6 | Pending |
| SECURITY-01 | Phase 6 | Pending |
| SECURITY-02 | Phase 7 | Pending |
| PERF-01 | Phase 7 | Pending |
| RELEASE-01 | Phase 7 | Pending |

**Coverage:**

- v1 requirements: 23 total
- Mapped to phases: 23
- Unmapped: 0

---
*Requirements defined: 2026-07-13*
*Last updated: 2026-07-13 after production-readiness roadmap reconciliation*
