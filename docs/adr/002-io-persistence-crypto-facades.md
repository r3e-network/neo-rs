# ADR-002: Extract IO/Persistence/Crypto to Separate Crates

## Status

Accepted (Completed)

## Date

2024-09

## Context

Before refactoring, neo-core contained its own implementations of I/O, persistence, and cryptographic primitives mixed with domain logic. This created circular dependencies where:

- neo-consensus needed crypto but depended on neo-core
- neo-p2p needed serialization but depended on neo-core
- Testing required mocking persistence even for pure logic tests

The `neo-io`, `neo-storage`, and `neo-crypto` crates existed but were underutilized, with neo-core reimplementing or wrapping their functionality.

## Decision

Extract all I/O, persistence, and cryptographic functionality to their respective focused crates:

### neo-io
- `BinaryWriter`, `MemoryReader`
- `Serializable` trait and implementations
- Compression utilities
- `WitnessCondition`, `WitnessRule`
- IO extension traits
- Variable-length integer encoding (`var_int`)
- Caching abstractions

### neo-storage
- `StorageKey`, `StorageItem`
- `DataCache` trait for state management
- `Store` and `StoreSnapshot` abstractions
- Storage backend interfaces

### neo-crypto
- Cryptographic primitives (hashing, signing)
- `MerkleTree` implementation
- ECC (Elliptic Curve Cryptography)
- `ECPoint` types
- MPT (Merkle Patricia) Trie
- `BloomFilter`

All facades and re-exports in neo-core were removed, forcing explicit dependencies on the specialized crates.

## Consequences

### Positive

- **Zero Circular Dependencies**: neo-consensus can depend on neo-crypto directly without neo-core
- **Testability**: Components can be tested with real implementations, not mocks
- **Performance**: Direct usage avoids indirection layers
- **Clarity**: Import paths clearly indicate which subsystem is being used
- **Reusability**: neo-crypto can be used in any Rust project needing Neo-compatible cryptography

### Negative

- **Import Churn**: All existing code needed updated imports
- **Build Coordination**: Changes to low-level crates require rebuilding dependents
- **Duplication Risk**: Must ensure no parallel implementations emerge

### Verification

The extraction was verified by:
1. Removing all I/O/persistence/crypto re-exports from neo-core
2. Updating all imports across the codebase
3. Running full test suite (4245 tests pass)
4. Verifying neo-consensus compiles without neo-core dependency

## Related Commits

- `8ee53268`: Remove IO/persistence/crypto facades
- `3b37bbb8`: Move IO extension traits to neo-io
- `d93f227c`: Move WitnessCondition/WitnessRule to neo-io
- `99c05a04`: Move compression to neo-io

## References

- ADR-001: Decouple neo-core (parent decision)
- ADR-003: Consensus independence (enabled by this decision)
