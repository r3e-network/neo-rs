# NEO-RS Full Node Code Review Plan

## Project Overview
- **Total Files**: 3,112 Rust files
- **Total Lines**: ~973,420 lines of code
- **Crates**: 18 crates across 4 architectural layers
- **Version**: 0.7.0

---

## Review Strategy

### Phase 1: Foundation Layer (No neo-* dependencies)
Critical base layer - bugs here propagate everywhere.

| Crate | Priority | Focus Areas |
|-------|----------|-------------|
| neo-primitives | P0 | UInt160/UInt256 arithmetic, overflow handling, serialization |
| neo-crypto | P0 | ECC correctness, hash functions, signature verification |
| neo-storage | P1 | Trait design, key builder correctness |
| neo-io | P1 | Binary serialization, cache implementations, memory safety |
| neo-json | P2 | JSON parsing edge cases, path traversal |

### Phase 2: Core Layer
Heart of the blockchain - consensus, VM, contracts.

| Crate | Priority | Focus Areas |
|-------|----------|-------------|
| neo-vm | P0 | Stack safety, opcode correctness, gas metering, reference counting |
| neo-contract | P0 | Contract execution, storage isolation, trigger handling |
| neo-p2p | P1 | Message parsing, inventory handling, DoS resistance |
| neo-rpc | P1 | Error handling, input validation |
| neo-consensus | P0 | dBFT correctness, view change logic, message validation |
| neo-core | P0 | Ledger integrity, mempool, network protocol, native contracts |

### Phase 3: Infrastructure Layer
Services and plugins - attack surface for external interactions.

| Crate | Priority | Focus Areas |
|-------|----------|-------------|
| neo-akka | P1 | Actor lifecycle, message ordering, supervision |
| neo-services | P2 | Trait completeness |
| neo-rpc-client | P1 | Request validation, response parsing, error handling |
| neo-plugins | P0 | RPC server security, RocksDB operations, dBFT plugin |
| neo-tee | P1 | Enclave boundaries, attestation, key protection |

### Phase 4: Application Layer
User-facing - input validation critical.

| Crate | Priority | Focus Areas |
|-------|----------|-------------|
| neo-cli | P2 | Command parsing, user input validation |
| neo-node | P1 | Configuration security, health checks, metrics |

---

## Review Checklist Per Module

### Security
- [ ] Integer overflow/underflow handling
- [ ] Buffer bounds checking
- [ ] Input validation and sanitization
- [ ] Cryptographic correctness
- [ ] DoS resistance (resource limits)
- [ ] Race conditions in async code
- [ ] Proper error handling (no panics in production paths)

### Code Quality
- [ ] SOLID principles adherence
- [ ] Error handling patterns (Result vs panic)
- [ ] Unsafe code audit
- [ ] Test coverage adequacy
- [ ] Documentation completeness
- [ ] API consistency

### Performance
- [ ] Unnecessary allocations
- [ ] Lock contention points
- [ ] Cache efficiency
- [ ] Async/await correctness

### Blockchain-Specific
- [ ] Consensus safety properties
- [ ] Transaction validation completeness
- [ ] State transition correctness
- [ ] Fork handling
- [ ] Replay protection

---

## Execution Order

1. **neo-primitives** → Foundation types
2. **neo-crypto** → Cryptographic operations
3. **neo-vm** → Virtual machine (critical)
4. **neo-consensus** → Consensus protocol (critical)
5. **neo-contract** → Smart contract execution
6. **neo-core** → Core blockchain logic
7. **neo-p2p** → Network protocol
8. **neo-plugins** → Plugin security
9. **neo-storage** → Storage layer
10. **neo-io** → I/O operations
11. **neo-akka** → Actor system
12. **neo-rpc** + **neo-rpc-client** → RPC layer
13. **neo-json** → JSON handling
14. **neo-services** → Service traits
15. **neo-tee** → TEE support
16. **neo-cli** → CLI application
17. **neo-node** → Node application

---

## Review Sessions

### Session 1: Critical Security (P0)
- neo-primitives, neo-crypto, neo-vm, neo-consensus, neo-contract

### Session 2: Core Infrastructure (P0-P1)
- neo-core, neo-p2p, neo-plugins

### Session 3: Supporting Systems (P1-P2)
- neo-storage, neo-io, neo-akka, neo-rpc, neo-rpc-client

### Session 4: Applications & Utilities (P2)
- neo-json, neo-services, neo-tee, neo-cli, neo-node

---

## Output Format

For each crate, generate:
1. **Summary**: Purpose and architecture
2. **Findings**: Issues categorized by severity (Critical/High/Medium/Low)
3. **Recommendations**: Specific fixes with code references
4. **Test Coverage**: Assessment of test adequacy
