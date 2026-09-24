# ADR-001: Decouple neo-core into Focused Crates

## Status

Accepted (In Progress)

## Date

2024-09 (Started)

## Context

The `neo-core` crate mirrored the C# Neo.csproj monolith, containing networking, smart contracts, wallets, persistence, telemetry, VM bridge, and more - all in one crate with 530+ files. This created several problems:

1. **Tight Coupling**: All components depended on each other through neo-core, making it impossible to use individual features independently
2. **Build Times**: Changes to any part of neo-core required recompiling the entire crate
3. **Testing Complexity**: Unit testing required pulling in unrelated dependencies
4. **Responsibility Blur**: Single crate violated single responsibility principle
5. **Reusability**: Components couldn't be reused in other projects without bringing the entire neo-core dependency

## Decision

Break neo-core into focused crates organized in clear architectural layers:

```
Layer 0 (Foundation, zero neo-* deps):
  neo-primitives: UInt160, UInt256, basic types, traits
  neo-json: JSON types
  neo-config: Configuration types

Layer 0.5 (Foundation with inter-deps):
  neo-io: Binary I/O, serialization
  neo-storage: Storage abstractions
  neo-crypto: Cryptographic primitives

Layer 1 (Core):
  neo-vm: Virtual machine
  neo-core: ApplicationEngine, NativeContracts, Block, Transaction

Layer 2 (Protocol):
  neo-consensus: dBFT 2.0 (zero neo-core dep)
  neo-p2p: P2P networking (zero neo-core dep)
  neo-rpc: JSON-RPC server/client

Infrastructure:
  neo-telemetry, neo-hsm, neo-tee (zero neo-core dep)
```

Each crate has:
- Single, well-defined responsibility
- Minimal dependencies on other neo-* crates
- Clear API boundaries
- Independent versioning capability

## Consequences

### Positive

- **Modularity**: Components can be used independently
- **Faster Builds**: Changes only rebuild affected crates
- **Easier Testing**: Unit tests have minimal dependencies
- **Better Documentation**: Each crate documents its specific domain
- **Reusability**: neo-crypto, neo-vm, etc. can be used in other Rust projects
- **Clearer Architecture**: Dependencies flow in one direction (no cycles)

### Negative

- **Migration Effort**: 70+ commits required to move types safely
- **Import Changes**: Code needs updated import paths
- **Type Resolution**: Some types had overlapping definitions requiring careful resolution
- **Coordination**: Changes may touch multiple crates simultaneously

### Current Progress

- Started: 530+ files in neo-core
- Current: 476 files in neo-core (54 files extracted)
- Completed: IO/persistence/crypto facades, consensus independence, P2P extraction analysis
- Remaining: smart_contract (158), oracle_service (90), network (84), ledger (26)

## References

- Memory: project_refactoring_progress.md (tracks 70 commits of refactoring work)
- Related ADRs: ADR-002, ADR-003, ADR-004, ADR-005
