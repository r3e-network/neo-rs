# ADR-004: Extract P2P Networking Layer

## Status

Accepted (In Progress)

## Date

2024-09

## Context

The P2P networking layer was embedded within neo-core under `network/p2p/`, tightly coupled to the rest of the system. This prevented:

- Using the P2P layer independently
- Testing networking logic without blockchain logic
- Clear separation between protocol wire format and application logic
- Reusing the P2P implementation in other Neo-related tools

The networking layer has distinct responsibilities:
- **Wire Protocol**: Message framing, encoding, serialization
- **Connection Management**: TCP connections, peer discovery, handshakes
- **Message Routing**: Dispatching messages to appropriate handlers
- **Error Handling**: Network-level vs protocol-level errors

## Decision

Extract P2P networking into a standalone `neo-p2p` crate with zero dependency on neo-core:

### neo-p2p Structure

**Wire Format Layer**:
- `MessageCommand`: Enum of all P2P message types
- `MessageFlags`: Protocol flags for message headers
- `RawMessage`: Wire-format message framing (header + payload)
- Message serialization/deserialization

**Protocol Layer**:
- 10 simple payload types (Version, Verack, GetBlocks, etc.)
- `ChannelsConfig`: Connection pool configuration
- Timeout configurations
- Network error types

**Error Handling**:
- `P2PError`: Network-specific errors
- `ProtocolViolation`: Enhanced error type for malformed messages
- `NetworkError`: Moved to neo-primitives for shared usage

### Dependency Structure

```
neo-p2p dependencies:
  ├─ neo-primitives (types, NetworkError)
  ├─ neo-io (serialization)
  └─ neo-crypto (message verification)
  
  NO dependency on neo-core
```

### Integration Point

neo-core implements application-level handlers that consume `RawMessage` from neo-p2p, decode payloads, and execute business logic.

## Consequences

### Positive

- **Independence**: P2P layer usable in diagnostic tools, proxies, monitors
- **Testability**: Can test protocol compliance without blockchain state
- **Clear Boundaries**: Wire format vs application logic separation
- **Reusability**: Other Neo tools can use the same P2P implementation
- **Protocol Evolution**: P2P protocol can evolve independently

### Negative

- **Message Handling Split**: Message definitions in neo-p2p, handlers in neo-core
- **Type Duplication Risk**: Must ensure payload types don't diverge
- **Integration Complexity**: More crates to coordinate during protocol changes

### Migration Approach

**Phase 1: Wire Format (Completed)**
- Move `RawMessage` with framing logic
- Move `MessageCommand` with full protocol support
- Add `P2PError` with `ProtocolViolation`

**Phase 2: Payload Types (Completed)**
- Move 10 simple payload types (Version, Verack, Ping, etc.)
- Move `ChannelsConfig` and timeout configuration
- Move `NetworkError` to neo-primitives

**Phase 3: Connection Management (In Progress)**
- Extract connection pooling logic
- Extract peer discovery
- Extract handshake protocol

**Phase 4: Integration (Pending)**
- Update neo-core to use neo-p2p APIs
- Remove duplicate networking code from neo-core
- Verify full protocol compatibility

### Current State

- Wire format and payload types extracted
- `MessageCommand` unified with full protocol
- Connection management still in neo-core (84 files in network/)

## Related Commits

- `88c9364d`: Add RawMessage wire-format framing to neo-p2p
- `16d5aedb`: Enhance P2PError with ProtocolViolation
- `8709e899`: Move 10 simple payload types to neo-p2p
- `257116eb`: Move ChannelsConfig and timeouts to neo-p2p
- `e85f1b1f`: Move NetworkError to neo-primitives
- `2810c95a`: Unify MessageCommand with full protocol support

## References

- ADR-001: Decouple neo-core (parent decision)
- ADR-002: IO/persistence/crypto facades (enabled this decision)
