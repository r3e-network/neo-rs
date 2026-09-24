# ADR-003: Make neo-consensus Independent from neo-core

## Status

Accepted (Completed)

## Date

2024-09

## Context

The `neo-consensus` crate implements the dBFT 2.0 consensus algorithm, which is a self-contained protocol that should be reusable in any blockchain project. However, it had a dependency on `neo-core`, creating a circular dependency problem:

- neo-consensus needed Block, Transaction types → depended on neo-core
- neo-core needed consensus logic → depended on neo-consensus
- This prevented using the consensus algorithm independently

The consensus algorithm itself is mathematically pure and should only depend on:
- Basic types (hashes, signatures, peer IDs)
- Serialization (for message encoding)
- Cryptography (for signature verification)

## Decision

Make neo-consensus completely independent from neo-core by:

1. **Move consensus-specific types to neo-primitives**:
   - `SerializablePayload` trait for consensus messages
   - `Inventory` trait for blocks and transactions
   - `Verifiable` base trait (verification signatures)
   - `BlockLike`, `HeaderLike`, `TransactionLike` traits
   - `NetworkMessage` trait for P2P communication
   - `PeerInfo`, `PeerId` types

2. **Use trait bounds instead of concrete types**:
   - Consensus operates on traits, not concrete Block/Transaction
   - Generic over `T: BlockLike + Verifiable`
   - Allows any implementation satisfying the trait contract

3. **Direct dependencies on foundation crates**:
   - neo-primitives (types and traits)
   - neo-io (serialization)
   - neo-crypto (signature verification)
   - No dependency on neo-core

## Consequences

### Positive

- **Reusability**: dBFT 2.0 can be used in any Rust blockchain project
- **No Circular Dependencies**: Clean unidirectional dependency flow
- **Testability**: Can test consensus with mock blocks/transactions
- **Clarity**: Consensus contract is explicit (trait bounds)
- **Modularity**: Consensus is a standalone library

### Negative

- **Trait Complexity**: Code uses more generic bounds
- **Type Resolution**: Required careful resolution of overlapping types (BlockLike existed in multiple places)
- **Migration**: neo-core had to implement the traits on its concrete types

### Implementation Strategy

The migration happened in phases:

**Phase 1: Extract traits to neo-primitives**
- Move `SerializablePayload`, `Inventory`, `Verifiable`
- Move `BlockLike`, `HeaderLike`, `TransactionLike`
- Move `NetworkMessage`

**Phase 2: Update neo-consensus**
- Replace concrete types with trait bounds
- Use traits for all operations
- Remove neo-core dependency

**Phase 3: Implement traits in neo-core**
- Make Block implement BlockLike
- Make Transaction implement TransactionLike
- Wire up consensus using concrete implementations

### Verification

Independence verified by:
1. neo-consensus Cargo.toml has no neo-core dependency
2. Full test suite passes (114 consensus tests)
3. Multi-node integration tests work (requires infrastructure)

## Related Commits

- `8ee53268`: Decouple neo-consensus (landmark)
- `b7c6488b`: Add SerializablePayload trait
- `a2ce790a`: Move Inventory trait to neo-primitives
- `daab9b8d`: Resolve BlockLike and Witness type overlaps
- `00365580`: Resolve NetworkMessage type overlap

## References

- ADR-001: Decouple neo-core (parent decision)
- ADR-002: IO/persistence/crypto facades (enabled this decision)
- ADR-005: Verifiable trait (key trait for consensus verification)
