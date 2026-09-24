# ADR-005: Move Verifiable Trait to neo-primitives

## Status

Accepted (Completed) - LANDMARK DECISION

## Date

2024-09

## Context

The `Verifiable` trait is central to Neo's architecture - it defines how blocks, transactions, and consensus messages prove their authenticity through witness verification. Before refactoring:

- `Verifiable` was in neo-core
- neo-consensus needed to verify signatures → depended on neo-core
- neo-p2p needed to verify message signatures → depended on neo-core
- Smart contracts needed to verify witnesses → already in neo-core

This created a dependency bottleneck where everything authentication-related required pulling in neo-core.

### The Verifiable Contract

```rust
pub trait Verifiable {
    fn witnesses(&self) -> &[Witness];
    fn verify(&self, context: &VerificationContext) -> VerifyResult;
}
```

This trait needs:
- `Witness` type (signature data)
- `VerificationContext` (chain state for verification)
- `VerifyResult` (success/failure/policy)

## Decision

Move the base `Verifiable` trait to neo-primitives as a foundation trait, and extend it in neo-core with implementation-specific logic.

### Architecture

**neo-primitives** (base trait):
```rust
pub trait Verifiable {
    fn witnesses(&self) -> &[Witness];
    fn get_script_hashes_for_verifying(&self) -> Vec<UInt160>;
}
```

**neo-core** (extended trait):
```rust
pub trait VerifiableExt: Verifiable {
    fn verify(&self, snapshot: &DataCache) -> VerifyResult;
    fn verify_witnesses(&self, snapshot: &DataCache) -> bool;
}
```

This split separates:
- **Interface** (what things are verifiable) → neo-primitives
- **Implementation** (how to verify using blockchain state) → neo-core

### Supporting Types Moved to neo-primitives

- `Witness` type definition
- `VerificationContext` trait
- `VerifyResult` enum
- `UInt160`, `UInt256` (already there)

## Consequences

### Positive

- **Dependency Breakthrough**: neo-consensus can declare "I work with Verifiable things" without neo-core
- **Type Safety**: Trait bounds enforce verification contract at compile time
- **Modularity**: Basic verification interface is a primitive, complex logic is in core
- **Testability**: Can create mock Verifiable types for testing
- **Enables Phase 3**: Smart contract extraction becomes possible

### Negative

- **Trait Splitting**: Two traits instead of one (base + extension)
- **Import Complexity**: Must import from both neo-primitives and neo-core for full functionality
- **Method Resolution**: Compiler may need explicit trait qualification in some cases

### Migration Strategy

**Phase 1: Prepare neo-primitives**
- Move `Witness` struct to neo-primitives
- Move `VerificationContext` trait to neo-primitives
- Move `VerifyResult` enum to neo-primitives

**Phase 2: Split Verifiable trait**
- Define base `Verifiable` in neo-primitives (interface only)
- Define `VerifiableExt` in neo-core (implementation)
- All neo-core types implement both

**Phase 3: Update dependents**
- neo-consensus uses `Verifiable` trait bound
- neo-core uses `VerifiableExt` for full verification
- Update all trait implementations

**Phase 4: Verify**
- All tests pass (4245 tests)
- neo-consensus compiles without neo-core
- Verification logic unchanged

### Why This Is a Landmark

This decision is marked as **LANDMARK** because:

1. **Architectural Pivot**: Enables decoupling the verification contract from verification implementation
2. **Unlocks Phase 3**: Makes smart contract extraction feasible
3. **Pattern Template**: Establishes trait-splitting pattern for other cross-cutting concerns
4. **Dependency Breaking**: Removes the largest blocker for consensus/P2P independence

The refactoring progress notes identify this as the key enabler for the next phase of extraction work.

## Related Commits

- `db3936ac`: Move Verifiable base trait to neo-primitives (LANDMARK)
- `daab9b8d`: Resolve BlockLike and Witness type overlaps

## References

- ADR-001: Decouple neo-core (parent decision)
- ADR-003: Consensus independence (directly enabled by this decision)
- Memory: project_refactoring_progress.md notes this as "key enabler for Phase 3"
