# Neo-rs Security Documentation

> **Version**: 0.7.0  
> **Last Updated**: 2026-01-28  
> **Target Compatibility**: Neo N3 v3.9.2

This document provides comprehensive security information for the neo-rs project, a professional Rust implementation of the Neo N3 blockchain node.

## Table of Contents

- [Security Model](#security-model)
  - [Threat Model](#threat-model)
  - [Trust Assumptions](#trust-assumptions)
  - [Attack Surfaces](#attack-surfaces)
- [Cryptographic Practices](#cryptographic-practices)
  - [Signature Schemes](#signature-schemes)
  - [Hash Functions](#hash-functions)
  - [Key Management](#key-management)
- [Consensus Security](#consensus-security)
  - [dBFT Safety Properties](#dbft-safety-properties)
  - [Byzantine Fault Tolerance](#byzantine-fault-tolerance)
- [Network Security](#network-security)
  - [P2P Security Measures](#p2p-security-measures)
  - [DoS Protections](#dos-protections)
- [Smart Contract Security](#smart-contract-security)
  - [VM Sandboxing](#vm-sandboxing)
  - [Gas Limits](#gas-limits)
- [Reporting Vulnerabilities](#reporting-vulnerabilities)
  - [Contact Information](#contact-information)
  - [Disclosure Policy](#disclosure-policy)

---

## Security Model

### Threat Model

Neo-rs is designed to operate in adversarial environments where threats can originate from various actors:

#### Threat Actors

| Actor | Capability | Motivation | Impact |
|-------|------------|------------|--------|
| **External Attackers** | Network-level access, transaction crafting | Double-spend, DoS, data extraction | High |
| **Byzantine Validators** | Up to `f = (n-1)/3` malicious consensus nodes | Block censorship, consensus disruption | Critical |
| **Malicious Contract Authors** | Smart contract deployment | Resource exhaustion, state corruption | Medium |
| **Compromised Nodes** | Partial node control | Eclipse attacks, data leakage | Medium |
| **Supply Chain** | Dependency compromise | Backdoors, weakened cryptography | Critical |

#### Threat Categories

1. **Network Attacks**
   - Eclipse attacks (isolating nodes from honest peers)
   - Sybil attacks (fake identity flooding)
   - Man-in-the-middle attacks on P2P connections
   - DDoS against node infrastructure

2. **Consensus Attacks**
   - Equivocation (conflicting messages from validators)
   - Long-range attacks (rewriting history)
   - Liveness attacks (preventing block production)
   - Censorship (selective transaction exclusion)

3. **VM/Contract Attacks**
   - Resource exhaustion (infinite loops, memory bombs)
   - Reentrancy attacks
   - Type confusion exploits
   - Stack overflow attacks

4. **Cryptographic Attacks**
   - Weak key generation
   - Side-channel attacks (timing, power analysis)
   - Hash collision attempts
   - Signature forgery

### Trust Assumptions

#### Consensus Trust Model

- **Validator Honesty Majority**: dBFT assumes fewer than `f = (n-1)/3` validators are Byzantine (malicious or faulty)
- **Network Synchrony**: Partial synchrony required for liveness; safety holds regardless
- **Key Security**: Validators' private keys are securely managed and not compromised

#### Node Operator Trust Model

| Component | Trust Level | Rationale |
|-----------|-------------|-----------|
| Core Cryptography | High | Well-audited external crates (ring, ed25519-dalek) |
| VM Execution | Medium | Sandboxed with resource limits |
| P2P Network | Low | Untrusted peers, all inputs validated |
| RPC Interface | Low | Externally accessible, requires authentication |
| Smart Contracts | None | Arbitrary code, fully sandboxed |

#### Deployment Trust Boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│                        UNTRUSTED ZONE                            │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐         │
│  │   P2P    │  │   RPC    │  │  Oracle  │  │ External │         │
│  │  Network │  │ Clients  │  │  Service │  │  APIs    │         │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘         │
│       │             │             │             │                │
│       ▼             ▼             ▼             ▼                │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    VALIDATION LAYER                       │   │
│  │         (Input sanitization, rate limiting)               │   │
│  └─────────────────────────────┬────────────────────────────┘   │
│                                │                                 │
│  ╔═════════════════════════════▼════════════════════════════╗   │
│  ║                    TRUSTED COMPUTE ZONE                   ║   │
│  ║  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ║   │
│  ║  │Consensus │  │    VM    │  │  Crypto  │  │  Wallet  │  ║   │
│  ║  │  Engine  │  │ Executor │  │  Engine  │  │  Store   │  ║   │
│  ║  └──────────┘  └──────────┘  └──────────┘  └──────────┘  ║   │
│  ╚══════════════════════════════════════════════════════════╝   │
│                                │                                 │
│       ┌────────────────────────┼────────────────────────┐        │
│       ▼                        ▼                        ▼        │
│  ┌──────────┐           ┌──────────┐           ┌──────────┐     │
│  │  HSM/TEE │           │  Encrypted│           │  Secure  │     │
│  │ (Option) │           │  Storage  │           │  Memory  │     │
│  └──────────┘           └──────────┘           └──────────┘     │
└─────────────────────────────────────────────────────────────────┘
```

### Attack Surfaces

#### 1. Network Layer

| Surface | Protocol | Risk Level | Mitigations |
|---------|----------|------------|-------------|
| P2P Messages | TCP + Custom | High | Message size limits, magic validation, rate limiting |
| RPC Interface | HTTP/JSON-RPC | Critical | Authentication, CORS, method filtering, rate limiting |
| WebSocket | ws/wss | Medium | Origin validation, message framing |

#### 2. Consensus Layer

| Surface | Input Type | Risk Level | Mitigations |
|---------|------------|------------|-------------|
| Consensus Messages | Signed payloads | Critical | Signature verification, view validation, timeout handling |
| Block Proposals | Block data | Critical | Multi-signature verification, transaction validation |
| View Changes | ChangeView msgs | High | M-of-N voting, signature verification |

#### 3. VM Layer

| Surface | Operation | Risk Level | Mitigations |
|---------|-----------|------------|-------------|
| Stack Operations | PUSH/POP/DUP | Medium | Stack depth limits, overflow checks |
| Arithmetic | ADD/MUL/POW | High | BigInt size limits, overflow protection |
| Memory | NEWARRAY/NEWSTRUCT | High | Item size limits, reference counting |
| Syscalls | Interop calls | Critical | Gas metering, syscall whitelist |

#### 4. Storage Layer

| Surface | Operation | Risk Level | Mitigations |
|---------|-----------|------------|-------------|
| State Reads | DB queries | Low | Read-only snapshots |
| State Writes | Commit batches | Medium | ACID transactions, atomic commits |
| MPT Operations | Trie updates | Medium | Node validation, root verification |

---

## Cryptographic Practices

### Signature Schemes

Neo-rs supports multiple elliptic curve signature schemes for different use cases:

#### Primary: ECDSA with secp256r1 (P-256)

```rust
// Primary curve for Neo N3
use neo_crypto::Secp256r1Crypto;

// Generate keypair
let private_key = Secp256r1Crypto::generate_private_key()?;
let public_key = Secp256r1Crypto::derive_public_key(&private_key)?;

// Sign and verify
let message = b"transaction data";
let signature = Secp256r1Crypto::sign(message, &private_key)?;
let valid = Secp256r1Crypto::verify(message, &signature, &public_key)?;
```

- **Curve**: NIST P-256 (secp256r1)
- **Use Case**: Transaction signing, consensus messages
- **Security Level**: ~128 bits
- **Rationale**: NIST standard, widely supported in HSMs

#### Secondary: ECDSA with secp256k1

```rust
// Bitcoin/Ethereum compatible
use neo_crypto::Secp256k1Crypto;

let signature = Secp256k1Crypto::sign(message, &private_key)?;
```

- **Curve**: secp256k1
- **Use Case**: Cross-chain compatibility
- **Security Level**: ~128 bits
- **Rationale**: Bitcoin/Ethereum compatibility

#### Alternative: Ed25519

```rust
// EdDSA signatures
use neo_crypto::Ed25519Crypto;

let signature = Ed25519Crypto::sign(message, &private_key)?;
```

- **Curve**: Curve25519
- **Use Case**: High-performance signing
- **Security Level**: ~128 bits
- **Rationale**: Fast verification, compact signatures

#### Signature Verification Requirements

| Context | Required Verifications | Security Property |
|---------|------------------------|-------------------|
| Transactions | ECDSA (secp256r1) | Non-repudiation |
| Consensus Messages | ECDSA (secp256r1) | Byzantine accountability |
| Block Signing | Multi-sig threshold | Distributed trust |
| Cross-chain | secp256k1 | Interoperability |

### Hash Functions

Neo-rs implements multiple hash functions with constant-time comparison support:

#### Primary Hash Functions

```rust
use neo_crypto::Crypto;

// SHA-256: Primary hash for block/transaction IDs
let hash = Crypto::sha256(data);

// Hash256: Double SHA-256 for transaction/block hashes
let tx_hash = Crypto::hash256(transaction_data);

// Hash160: RIPEMD160(SHA256(data)) for script hashes/addresses
let script_hash = Crypto::hash160(script);
```

#### Hash Function Summary

| Function | Output Size | Use Case | Security |
|----------|-------------|----------|----------|
| SHA-256 | 32 bytes | Block/Tx IDs, general hashing | SHA-2 family |
| SHA-512 | 64 bytes | Key derivation | SHA-2 family |
| Hash256 | 32 bytes | Neo transaction hashes | Double SHA-256 |
| Hash160 | 20 bytes | Script hashes, addresses | RIPEMD-160 ∘ SHA-256 |
| Keccak-256 | 32 bytes | Ethereum compatibility | SHA-3 variant |
| Blake2b | 64 bytes | Fast hashing | Modern design |
| Blake2s | 32 bytes | Fast hashing (32-bit) | Modern design |

#### Constant-Time Comparison

```rust
use neo_crypto::hash::ct_hash_eq;

// Prevent timing attacks when comparing hashes
if ct_hash_eq(&computed_hash, &expected_hash) {
    // Match - constant time regardless of position of difference
}
```

### Key Management

#### Key Generation

```rust
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// Secure key generation using OS CSPRNG
pub fn generate_private_key() -> Result<[u8; 32], String> {
    let mut rng = OsRng;
    for _ in 0..MAX_KEY_GEN_ATTEMPTS {
        let mut candidate = Zeroizing::new([u8; 32]);
        rng.fill_bytes(candidate.as_mut());
        if let Ok(secret_key) = SecretKey::from_slice(candidate.as_ref()) {
            return Ok(secret_key.secret_bytes());
        }
        // candidate is automatically zeroized on drop
    }
    Err("Key generation failed".to_string())
}
```

Security properties:
- **CSPRNG**: Uses `OsRng` (operating system's cryptographically secure RNG)
- **Zeroization**: Keys are wrapped in `Zeroizing` to clear memory on drop
- **Validation**: Generated keys are validated against curve requirements
- **Retry Logic**: Bounded retry attempts with failure reporting

#### Key Storage

| Storage Type | Use Case | Protection |
|--------------|----------|------------|
| Memory (Zeroizing) | Runtime operations | Auto-clear on drop |
| Encrypted Wallet File | At-rest storage | AES-256-GCM + password |
| HSM (optional) | High-security operations | Hardware isolation |
| TEE (optional) | Secure enclaves | Intel SGX/AMD SEV |

#### Address Derivation

```
Private Key (32 bytes)
       │
       ▼
Public Key (33 bytes compressed)
       │
       ▼
Script Hash (20 bytes) = Hash160(public_key)
       │
       ▼
Neo Address (34 chars) = Base58Check(script_hash)
```

---

## Consensus Security

### dBFT Safety Properties

Neo-rs implements dBFT 2.0 (Delegated Byzantine Fault Tolerance), providing:

#### Safety Guarantees

1. **Single-Block Finality**: Once a block is committed, it cannot be reverted
2. **No Forks**: The consensus protocol ensures no competing valid chains exist
3. **Consistency**: All honest nodes agree on the same block at each height

```rust
// Safety threshold: M = (n + f) / 2 + 1 = 2f + 1
// Where:
//   n = total validators
//   f = max Byzantine = floor((n-1)/3)
//   M = minimum signatures needed
```

#### Consensus Message Flow

```
┌────────────────────────────────────────────────────────────────────┐
│                     dBFT 2.0 Consensus Flow                         │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   Speaker (Primary)              Validators (Backups)               │
│        │                                │                           │
│   T+0  │───── PrepareRequest ──────────>│  (propose block)          │
│        │  [block, txs, timestamp]       │                           │
│        │                                │                           │
│   T+?  │<──────── PrepareResponse ─────│  M = (n+f)/2 responses    │
│        │        [signature]             │                           │
│        │                                │                           │
│   T+?  │<──────── Commit ──────────────│  (when M responses)       │
│        │        [signature]             │                           │
│        │                                │                           │
│   T+?  │         Block Committed        │  (when M commits)         │
│        │                                │                           │
└────────────────────────────────────────────────────────────────────┘
```

#### Message Validation

Each consensus message undergoes rigorous validation:

```rust
// PrepareResponse validation
async fn on_prepare_response_received(
    &mut self,
    payload: ExtensiblePayload,
    message: PrepareResponse,
) {
    // 1. Verify view number matches current view
    if message.view_number() != context.view_number() {
        return;
    }
    
    // 2. Verify validator index is valid
    let index = message.validator_index() as usize;
    if index >= context.validators.len() {
        return;
    }
    
    // 3. Verify signature using validator's public key
    let validator_pubkey = context.validators.get(index)
        .expect("Validator exists");
    
    if !verify_payload_signature(&payload, validator_pubkey).await {
        return; // Invalid signature - reject
    }
    
    // 4. Verify preparation hash matches
    if !verify_preparation_hash(&message, &context) {
        return;
    }
    
    // Accept message
    context.preparation_payloads[index] = Some(payload);
}
```

### Byzantine Fault Tolerance

#### Fault Tolerance Formula

| Validators (n) | Byzantine (f) | Minimum Signatures (M) | Fault Tolerance |
|----------------|---------------|------------------------|-----------------|
| 4 | 1 | 3 | 25% |
| 7 | 2 | 5 | 28.5% |
| 10 | 3 | 7 | 30% |
| 21 | 7 | 15 | 33.3% |

#### View Change Mechanism

When the primary (speaker) fails or is malicious, validators trigger a view change:

```
Validator detects timeout/invalid block
            │
            ▼
     Send ChangeView
            │
            ▼
     Wait for M ChangeViews
            │
            ▼
     New Primary = validators[view % n]
            │
            ▼
     Start new view
```

#### Change View Reasons

| Reason | Trigger | Response |
|--------|---------|----------|
| `Timeout` | No PrepareRequest received | Request new speaker |
| `TxNotFound` | Missing transaction in proposal | Reject block, change view |
| `TxInvalid` | Invalid transaction in block | Reject block, change view |
| `BlockInvalid` | Block verification failed | Reject block, change view |
| `ChangeAgreement` | Agreed with other validators | Participate in view change |

#### Recovery Mechanism

Nodes can request and receive consensus state to recover from network partitions:

```rust
// Recovery request - request state from peers
pub struct RecoveryRequest {
    pub timestamp: u64,
    pub validator_index: u16,
}

// Recovery message - contains full consensus state
pub struct RecoveryMessage {
    pub change_view_messages: Vec<ChangeViewMessage>,
    pub prepare_request: Option<PrepareRequestMessage>,
    pub prepare_responses: Vec<PrepareResponseMessage>,
    pub commit_messages: Vec<CommitMessage>,
}
```

---

## Network Security

### P2P Security Measures

#### Connection Management

```rust
pub struct P2PConfig {
    /// Maximum connections per IP address
    pub max_connections_per_address: usize,  // Default: 10
    /// Maximum total peer connections
    pub max_connections: usize,              // Default: 100
    /// Connection timeout
    pub handshake_timeout: Duration,         // Default: 10s
    /// Per-peer memory quota
    pub per_peer_memory_quota: usize,        // Default: 4MB
}
```

#### Message Validation

All P2P messages undergo validation before processing:

```rust
pub fn validate_message(header: &MessageHeader, payload: &[u8]) -> Result<(), P2PError> {
    // 1. Validate magic number (network identification)
    if header.magic != expected_magic {
        return Err(P2PError::InvalidMagic);
    }
    
    // 2. Validate message size limits
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(P2PError::MessageTooLarge);
    }
    
    // 3. Validate checksum
    let computed_checksum = Crypto::hash256(payload)[..4];
    if computed_checksum != header.checksum {
        return Err(P2PError::InvalidChecksum);
    }
    
    // 4. Validate command type
    if !is_valid_command(header.command) {
        return Err(P2PError::InvalidCommand);
    }
    
    Ok(())
}
```

#### Peer Reputation System

```rust
pub struct PeerReputation {
    score: i32,
    violations: VecDeque<Violation>,
}

pub enum Violation {
    InvalidMessage,      // -10 points
    InvalidChecksum,     // -20 points
    ProtocolViolation,   // -50 points
    Spam,                // -5 points
    MisbehavingConsensus,// -100 points
}
```

### DoS Protections

#### Rate Limiting

```rust
pub struct RateLimiter {
    /// Token bucket per IP
    buckets: DashMap<IpAddr, TokenBucket>,
    config: RateLimitConfig,
}

pub struct RateLimitConfig {
    /// Requests per second sustained
    pub requests_per_second: f64,  // Default: 10.0
    /// Burst capacity
    pub burst_size: u32,           // Default: 100
}
```

#### Resource Limits

| Resource | Limit | Purpose |
|----------|-------|---------|
| Message size | 32 MB | Prevent memory exhaustion |
| Per-peer memory | 4 MB | Limit per-peer resource usage |
| Max stack size | 2,048 items | VM stack overflow prevention |
| Max item size | 65,535 bytes | Prevent large object allocation |
| Max invocation depth | 1,024 | Call stack protection |
| Max instructions | 1,000,000 | Infinite loop prevention |

#### Memory Management

```rust
pub struct FramedSocket {
    stream: TcpStream,
    config: FramedConfig,
    memory_used: AtomicUsize,  // Track memory usage per peer
}

impl FramedSocket {
    pub async fn read_message(&mut self) -> NetworkResult<Vec<u8>> {
        let (payload_length, _) = self.read_var_int().await?;
        let payload_len = payload_length as usize;
        
        // Check memory quota before allocation
        let current_usage = self.memory_used.load(Ordering::Relaxed);
        if current_usage + payload_len > PER_PEER_MEMORY_QUOTA {
            return Err(NetworkError::ResourceExhausted(
                "Peer memory quota exceeded".to_string()
            ));
        }
        
        self.memory_used.fetch_add(payload_len, Ordering::Relaxed);
        // ... process message
    }
}
```

---

## Smart Contract Security

### VM Sandboxing

The Neo VM provides multiple layers of sandboxing:

#### Execution Context Isolation

```rust
pub struct ExecutionEngine {
    /// Current execution context stack
    invocation_stack: Vec<ExecutionContext>,
    /// Operand stack
    evaluation_stack: EvaluationStack,
    /// Reference counter for compound types
    reference_counter: ReferenceCounter,
    /// Execution limits
    limits: ExecutionEngineLimits,
    /// Gas counter
    gas_consumed: i64,
}
```

#### Syscall Whitelisting

Only approved syscalls can be invoked from smart contracts:

```rust
pub enum Syscall {
    // Runtime services
    RuntimeGetTrigger,
    RuntimeGetTime,
    RuntimeGetScriptContainer,
    
    // Storage services
    StorageGet,
    StoragePut,
    StorageDelete,
    StorageFind,
    
    // Crypto services
    CryptoVerify,
    CryptoHash160,
    CryptoHash256,
    
    // Contract services
    ContractCall,
    ContractCreate,
    ContractUpdate,
    ContractDestroy,
    
    // ... (30+ whitelisted syscalls)
}
```

#### Exception Handling

```rust
pub struct ExceptionHandlingContext {
    pub state: ExceptionHandlingState,
    pub try_offset: i32,
    pub catch_offset: i32,
    pub finally_offset: i32,
    pub end_offset: i32,
}

// Maximum nesting depth for try-catch-finally
const MAX_TRY_NESTING_DEPTH: u32 = 16;
```

### Gas Limits

Gas metering prevents resource exhaustion attacks:

#### Gas Cost Model

| Operation Type | Base Cost | Description |
|----------------|-----------|-------------|
| Instruction | 1 gas | Per VM instruction executed |
| Storage write | 1,000 gas | Per byte written |
| Storage read | 100 gas | Per byte read |
| Syscall | Variable | Depends on syscall complexity |
| Contract call | 10,000 gas | Base cost for external call |

#### Execution Limits

```rust
pub struct ExecutionEngineLimits {
    /// Maximum shift operations
    pub max_shift: i32,                      // 256
    /// Maximum stack size
    pub max_stack_size: u32,                 // 2,048
    /// Maximum item size
    pub max_item_size: u32,                  // 65,535
    /// Maximum comparable size
    pub max_comparable_size: u32,            // 65,535
    /// Maximum invocation stack depth
    pub max_invocation_stack_size: u32,      // 1,024
    /// Maximum try-catch nesting
    pub max_try_nesting_depth: u32,          // 16
    /// Maximum instructions per execution
    pub max_instructions: u64,               // 1,000,000
}
```

#### BigInt Protection

```rust
/// Maximum size for BigInt results (256 bits)
const MAX_BIGINT_SIZE: usize = 32;

fn check_bigint_size(value: &BigInt, limits: &ExecutionEngineLimits) -> VmResult<()> {
    let byte_len = value.to_signed_bytes_le().len();
    if byte_len > MAX_BIGINT_SIZE {
        return Err(VmError::invalid_operation_msg(format!(
            "BigInt size {} exceeds maximum {}",
            byte_len, MAX_BIGINT_SIZE
        )));
    }
    Ok(())
}
```

This prevents memory exhaustion attacks through unbounded BigInt growth in arithmetic operations.

---

## Reporting Vulnerabilities

### Contact Information

We take security vulnerabilities seriously. If you discover a security issue, please report it through the appropriate channels:

| Severity | Response Time | Contact |
|----------|---------------|---------|
| Critical (P0) | 24 hours | security@r3e.network |
| High (P1) | 72 hours | security@r3e.network |
| Medium (P2) | 1 week | security@r3e.network |
| Low (P3) | 2 weeks | GitHub Issues |

**Email**: `security@r3e.network`

Please include:
1. Clear description of the vulnerability
2. Steps to reproduce
3. Potential impact assessment
4. Any proof-of-concept code
5. Suggested fix (if available)

### Disclosure Policy

We follow a coordinated disclosure process:

```
Day 0:   Vulnerability reported
Day 1-5: Acknowledgment and initial assessment
Day 5-30: Fix development and testing
Day 30:  Security patch released
Day 37:  Public disclosure (7 days after fix)
```

#### Disclosure Principles

1. **Coordinated Disclosure**: We work with reporters to coordinate public disclosure timing
2. **Credit**: Reporters will be credited in security advisories (unless anonymity is requested)
3. **Transparency**: Security issues are documented in `SECURITY_PATCHES.md`
4. **No Legal Action**: We will not pursue legal action against security researchers who:
   - Follow responsible disclosure practices
   - Do not exploit vulnerabilities beyond minimal proof-of-concept
   - Do not access, modify, or delete data belonging to others

#### Security Checklist for Deployments

- [ ] All CRITICAL security patches applied
- [ ] All HIGH security patches applied
- [ ] RPC interface hardened (authentication, rate limiting)
- [ ] P2P connections limited per IP
- [ ] VM execution limits configured
- [ ] Consensus messages validated
- [ ] TLS termination at reverse proxy
- [ ] Regular security audits scheduled

---

## Additional Resources

- [Security Patches](../SECURITY_PATCHES.md) - Recent security fixes
- [RPC Hardening](./RPC_HARDENING.md) - RPC security configuration
- [Architecture](./ARCHITECTURE.md) - System architecture overview
- [Neo N3 Documentation](https://developers.neo.org/) - Official Neo documentation

---

**Last Updated**: 2026-01-28  
**Document Version**: 0.7.0
