# Security & Cryptography

<cite>
**Referenced Files in This Document**
- [SECURITY.md](file://SECURITY.md)
- [docs/SECURITY.md](file://docs/SECURITY.md)
- [neo-crypto/src/lib.rs](file://neo-crypto/src/lib.rs)
- [neo-crypto/src/hash.rs](file://neo-crypto/src/hash.rs)
- [neo-crypto/src/signature.rs](file://neo-crypto/src/signature.rs)
- [neo-crypto/src/ecc.rs](file://neo-crypto/src/ecc.rs)
- [neo-crypto/src/bls12381.rs](file://neo-crypto/src/bls12381.rs)
- [neo-core/src/witness.rs](file://neo-core/src/witness.rs)
- [neo-core/src/validation.rs](file://neo-core/src/validation.rs)
- [neo-core/src/network/p2p/payloads/transaction/verification.rs](file://neo-core/src/network/p2p/payloads/transaction/verification.rs)
- [neo-hsm/src/lib.rs](file://neo-hsm/src/lib.rs)
- [neo-tee/src/lib.rs](file://neo-tee/src/lib.rs)
</cite>

## Table of Contents
1. Introduction
2. Project Structure
3. Core Components
4. Architecture Overview
5. Detailed Component Analysis
6. Dependency Analysis
7. Performance Considerations
8. Troubleshooting Guide
9. Conclusion
10. Appendices

## Introduction
This document provides comprehensive security and cryptography guidance for the Neo-RS implementation. It covers cryptographic primitives (ECDSA, hash functions including SHA-256, RIPEMD-160, Keccak-256), BLS signature aggregation used in consensus, witness validation, transaction signing, multi-signature support, HSM integration, and TEE support. It also includes threat analysis, mitigation strategies, audit references, secure deployment guidance, certificate management, and monitoring recommendations.

## Project Structure
Neo-RS organizes security-related functionality across dedicated crates:
- neo-crypto: Hashing, ECDSA, Ed25519, ECC utilities, and BLS12-381 helpers
- neo-core: Witness handling, block and transaction validation, transaction verification logic
- neo-hsm: Hardware-backed key management and signing abstractions
- neo-tee: Trusted Execution Environment support with enclave runtime, sealed wallet storage, fair mempool ordering, and attestation

```mermaid
graph TB
subgraph "Crypto Primitives"
C1["neo-crypto<br/>hash.rs"]
C2["neo-crypto<br/>signature.rs"]
C3["neo-crypto<br/>ecc.rs"]
C4["neo-crypto<br/>bls12381.rs"]
end
subgraph "Core Validation"
V1["neo-core<br/>witness.rs"]
V2["neo-core<br/>validation.rs"]
V3["neo-core<br/>transaction verification.rs"]
end
subgraph "Secure Enclaves & HSM"
H1["neo-hsm<br/>lib.rs"]
T1["neo-tee<br/>lib.rs"]
end
C1 --> V1
C2 --> V1
C3 --> V1
C4 --> V3
V1 --> V2
V2 --> V3
H1 --> V1
T1 --> V1
```

**Diagram sources**
- [neo-crypto/src/hash.rs:1-544](file://neo-crypto/src/hash.rs#L1-L544)
- [neo-crypto/src/signature.rs:1-703](file://neo-crypto/src/signature.rs#L1-L703)
- [neo-crypto/src/ecc.rs:1-909](file://neo-crypto/src/ecc.rs#L1-L909)
- [neo-crypto/src/bls12381.rs:1-284](file://neo-crypto/src/bls12381.rs#L1-L284)
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-core/src/validation.rs:1-685](file://neo-core/src/validation.rs#L1-L685)
- [neo-core/src/network/p2p/payloads/transaction/verification.rs:261-303](file://neo-core/src/network/p2p/payloads/transaction/verification.rs#L261-L303)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

**Section sources**
- [neo-crypto/src/lib.rs:1-96](file://neo-crypto/src/lib.rs#L1-L96)
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-core/src/validation.rs:1-685](file://neo-core/src/validation.rs#L1-L685)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

## Core Components
- Hashing: SHA-256, SHA-512, Keccak-256, RIPEMD-160, Blake2b/s, constant-time comparison helpers
- ECDSA: secp256r1 (primary), secp256k1 (compatibility), Ed25519; prehash APIs for protocol parity
- ECC: Point validation, curve selection, safe encoding/decoding, constant-time equality
- BLS12-381: Sign, verify, aggregate signatures for dBFT consensus
- Witness: Single-sig and multi-sig verification, script parsing, size limits
- Validation: Block size, transaction count, timestamps, merkle root, duplicate tx detection, witness script checks
- HSM: Unified interface for Ledger/PKCS#11/simulation signers
- TEE: Enclave runtime, sealed wallet, fair mempool ordering, attestation

**Section sources**
- [neo-crypto/src/hash.rs:1-544](file://neo-crypto/src/hash.rs#L1-L544)
- [neo-crypto/src/signature.rs:1-703](file://neo-crypto/src/signature.rs#L1-L703)
- [neo-crypto/src/ecc.rs:1-909](file://neo-crypto/src/ecc.rs#L1-L909)
- [neo-crypto/src/bls12381.rs:1-284](file://neo-crypto/src/bls12381.rs#L1-L284)
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-core/src/validation.rs:1-685](file://neo-core/src/validation.rs#L1-L685)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

## Architecture Overview
The system enforces a layered defense: untrusted inputs enter through P2P/RPC, pass strict validation, then reach the trusted compute zone where consensus, VM execution, crypto operations, and wallet/HSM/TEE interactions occur.

```mermaid
graph TB
A["Untrusted Inputs<br/>P2P / RPC"] --> B["Validation Layer<br/>Size, Timestamps, Merkle, Witnesses"]
B --> C["Trusted Compute Zone<br/>Consensus / VM / Crypto"]
C --> D["HSM / TEE<br/>Key Isolation & Attestation"]
C --> E["Storage / State"]
```

[No sources needed since this diagram shows conceptual workflow, not actual code structure]

## Detailed Component Analysis

### Cryptographic Primitives
- Hash Functions
  - SHA-256, SHA-512, Keccak-256, RIPEMD-160, Blake2b/s
  - Constant-time comparison utilities to prevent timing side-channels
- ECDSA
  - secp256r1 (primary): sign, verify, prehash variants for protocol compatibility
  - secp256k1: sign/verify/recover for cross-chain compatibility
  - Ed25519: fast deterministic signatures
- ECC Utilities
  - Curve-aware point construction with on-curve validation
  - Compressed/uncompressed encodings, infinity handling, constant-time equality
- BLS12-381
  - Sign, verify, aggregate signatures using minimal-signature-size scheme
  - Domain separation tag aligned with reference implementation

```mermaid
classDiagram
class Crypto {
+sha256(data) [u8;32]
+sha512(data) [u8;64]
+keccak256(data) [u8;32]
+ripemd160(data) [u8;20]
+hash(algorithm, data) Vec<u8>
}
class Secp256r1Crypto {
+generate_private_key() [u8;32]
+derive_public_key(private_key) Vec<u8>
+sign(message, private_key) [u8;64]
+sign_prehash(digest, private_key) [u8;64]
+verify(message, signature, public_key) bool
+verify_prehash(digest, signature, public_key) bool
}
class Secp256k1Crypto {
+generate_private_key() [u8;32]
+derive_public_key(private_key) [u8;33]
+sign(message, private_key) [u8;64]
+verify(message, signature, public_key) bool
+recover_public_key(message_hash, signature) Vec<u8>
}
class Ed25519Crypto {
+generate_private_key() [u8;32]
+derive_public_key(private_key) [u8;32]
+sign(message, private_key) [u8;64]
+verify(message, signature, public_key) bool
}
class Bls12381Crypto {
+generate_private_key() Zeroizing<[u8;32]>
+derive_public_key(private_key) [u8;96]
+sign(message, private_key) [u8;48]
+verify(message, signature, public_key) bool
+aggregate_signatures(signatures) [u8;48]
+verify_aggregated(message, aggregated_signature, public_keys) bool
}
Crypto --> Secp256r1Crypto : "uses"
Crypto --> Secp256k1Crypto : "uses"
Crypto --> Ed25519Crypto : "uses"
Crypto --> Bls12381Crypto : "uses"
```

**Diagram sources**
- [neo-crypto/src/hash.rs:1-544](file://neo-crypto/src/hash.rs#L1-L544)
- [neo-crypto/src/signature.rs:1-703](file://neo-crypto/src/signature.rs#L1-L703)
- [neo-crypto/src/bls12381.rs:1-284](file://neo-crypto/src/bls12381.rs#L1-L284)

**Section sources**
- [neo-crypto/src/hash.rs:1-544](file://neo-crypto/src/hash.rs#L1-L544)
- [neo-crypto/src/signature.rs:1-703](file://neo-crypto/src/signature.rs#L1-L703)
- [neo-crypto/src/ecc.rs:1-909](file://neo-crypto/src/ecc.rs#L1-L909)
- [neo-crypto/src/bls12381.rs:1-284](file://neo-crypto/src/bls12381.rs#L1-L284)

### Witness Validation and Transaction Signing
- Single-signature witnesses extract public key from verification script and signature from invocation script, then verify via secp256r1 prehash over SHA-256 digest
- Multi-signature witnesses validate m-of-n thresholds, sort keys, and verify each signature against the message digest
- Transaction verification integrates per-witness checks and computes fees based on contract types

```mermaid
sequenceDiagram
participant TX as "Transaction"
participant W as "Witness"
participant C as "Crypto"
participant S as "Secp256r1Crypto"
TX->>W : verify_signature(hash_data, account)
W->>W : extract_public_key_from_verification_script()
W->>W : extract_signature_from_invocation_script()
W->>C : sha256(hash_data)
C-->>W : digest
W->>S : verify_prehash(digest, signature, public_key)
S-->>W : bool
W->>W : compute_script_hash_from_public_key() == account?
W-->>TX : verified
```

**Diagram sources**
- [neo-core/src/witness.rs:171-351](file://neo-core/src/witness.rs#L171-L351)
- [neo-crypto/src/signature.rs:172-235](file://neo-crypto/src/signature.rs#L172-L235)
- [neo-crypto/src/hash.rs:84-134](file://neo-crypto/src/hash.rs#L84-L134)

**Section sources**
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-core/src/network/p2p/payloads/transaction/verification.rs:261-303](file://neo-core/src/network/p2p/payloads/transaction/verification.rs#L261-L303)

### Multi-Signature Support
- Multi-sig verification enforces required signature count, sorts public keys, validates each 64-byte signature against the message digest, and ensures remaining keys are sufficient to meet threshold
- Fee calculation accounts for multi-sig contract cost scaling with m and n

```mermaid
flowchart TD
Start(["Start Multi-Sig Verify"]) --> CheckInputs["Validate m, keys, signatures lengths"]
CheckInputs --> BuildScript["Build multi-sig redeem script"]
BuildScript --> MatchAccount{"Script hash matches account?"}
MatchAccount --> |No| Fail["Reject"]
MatchAccount --> |Yes| SortKeys["Sort public keys"]
SortKeys --> Loop{"For each signature"}
Loop --> VerifySig["Verify signature against SHA-256 digest"]
VerifySig --> UpdateCounters["Update sig_index, key_index"]
UpdateCounters --> ThresholdCheck{"Remaining keys >= remaining sigs?"}
ThresholdCheck --> |No| Fail
ThresholdCheck --> |Yes| Next["Next signature"]
Next --> Loop
Loop --> Done{"All sigs verified?"}
Done --> |Yes| Success["Accept"]
Done --> |No| Fail
```

**Diagram sources**
- [neo-core/src/witness.rs:190-255](file://neo-core/src/witness.rs#L190-L255)

**Section sources**
- [neo-core/src/witness.rs:190-255](file://neo-core/src/witness.rs#L190-L255)
- [neo-core/src/network/p2p/payloads/transaction/verification.rs:261-303](file://neo-core/src/network/p2p/payloads/transaction/verification.rs#L261-L303)

### BLS Signature Aggregation (dBFT)
- BLS12-381 supports sign, verify, and aggregate signatures for consensus messages
- Uses minimal-signature-size scheme with domain separation tag matching reference implementation
- Aggregation validates subgroup membership for both signatures and public keys

```mermaid
sequenceDiagram
participant V as "Validator"
participant B as "Bls12381Crypto"
V->>B : sign(message, private_key)
B-->>V : signature (48 bytes)
V->>B : aggregate_signatures([sig1, sig2, ...])
B-->>V : aggregated_signature
V->>B : verify_aggregated(message, aggregated_signature, [pk1, pk2, ...])
B-->>V : bool
```

**Diagram sources**
- [neo-crypto/src/bls12381.rs:90-198](file://neo-crypto/src/bls12381.rs#L90-L198)

**Section sources**
- [neo-crypto/src/bls12381.rs:1-284](file://neo-crypto/src/bls12381.rs#L1-L284)

### Block and Transaction Validation
- Enforces block size, transaction count, timestamp bounds and progression, merkle root integrity, duplicate transactions, witness script sizes, and primary index validity
- Provides structured error types for precise diagnostics

```mermaid
flowchart TD
A["Receive Block"] --> Size["Validate block size"]
Size --> TxCount["Validate transaction count"]
TxCount --> Time["Validate timestamps (bounds + progression)"]
Time --> Merkle["Recompute and compare merkle root"]
Merkle --> Dupes["Check duplicate transactions"]
Dupes --> Witness["Validate witness scripts"]
Witness --> Primary["Validate primary index"]
Primary --> Accept["Block accepted"]
```

**Diagram sources**
- [neo-core/src/validation.rs:128-409](file://neo-core/src/validation.rs#L128-L409)

**Section sources**
- [neo-core/src/validation.rs:1-685](file://neo-core/src/validation.rs#L1-L685)

### HSM Integration
- Unified interface supporting Ledger devices, PKCS#11 modules, and simulation mode
- Exposes device info, configuration, PIN prompting, and signer abstraction for secure key operations

```mermaid
graph LR
App["Node Application"] --> HSM["HSM Runtime"]
HSM --> Ledger["LedgerSigner"]
HSM --> PKCS["Pkcs11Signer"]
HSM --> Sim["SimulationSigner"]
```

**Diagram sources**
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)

**Section sources**
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)

### TEE Support
- Provides enclave runtime, sealed wallet storage, fair mempool ordering, and remote attestation
- Supports SGX hardware feature flags and simulation mode; emphasizes host-side sealing and attestation verification boundaries

```mermaid
graph TB
Host["Host Process"] --> Bridge["TEE Bridge"]
Bridge --> Enclave["Enclave Runtime"]
Enclave --> Wallet["Sealed Wallet"]
Enclave --> Mempool["Fair Ordering"]
Enclave --> Attest["Attestation Service"]
```

**Diagram sources**
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

**Section sources**
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

## Dependency Analysis
- neo-core depends on neo-crypto for hashing and signature verification
- neo-core validation uses merkle tree and time provider utilities
- neo-hsm and neo-tee integrate with core workflows to isolate sensitive operations
- External crates provide cryptographic primitives (e.g., blst for BLS, p256/secp256k1/ed25519-dalek)

```mermaid
graph TB
NC["neo-core"] --> NCR["neo-crypto"]
NC --> NV["neo-primitives"]
NH["neo-hsm"] --> NC
NT["neo-tee"] --> NC
```

**Diagram sources**
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-crypto/src/lib.rs:1-96](file://neo-crypto/src/lib.rs#L1-L96)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

**Section sources**
- [neo-crypto/src/lib.rs:1-96](file://neo-crypto/src/lib.rs#L1-L96)
- [neo-core/src/witness.rs:1-520](file://neo-core/src/witness.rs#L1-L520)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

## Performance Considerations
- Prefer prehash APIs for ECDSA to avoid redundant hashing in hot paths
- Use constant-time comparisons for hashes and secret material
- Limit witness script sizes to mitigate DoS vectors
- Aggregate BLS signatures in consensus to reduce bandwidth and verification overhead
- Configure rate limiting and resource quotas at network boundaries

[No sources needed since this section provides general guidance]

## Troubleshooting Guide
- Invalid signature or public key formats: check compressed key prefixes and length constraints
- Multi-sig failures: ensure correct m-of-n threshold, sorted keys, and matching account script hash
- Block validation errors: inspect size, transaction count, timestamps, merkle root mismatches, and witness script sizes
- HSM/TEE issues: verify device availability, PIN prompts, feature flags, and attestation results

**Section sources**
- [neo-core/src/witness.rs:257-351](file://neo-core/src/witness.rs#L257-L351)
- [neo-core/src/validation.rs:128-409](file://neo-core/src/validation.rs#L128-L409)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)

## Conclusion
Neo-RS implements robust cryptographic primitives, rigorous validation, and secure key management via HSM and TEE. By adhering to the outlined best practices and configurations, operators can deploy nodes that maintain consensus safety, protect secrets, and resist common attack vectors.

[No sources needed since this section summarizes without analyzing specific files]

## Appendices

### Threat Model and Mitigations
- Network attacks: enforce message size limits, magic validation, checksums, rate limiting, peer reputation
- Consensus attacks: validate view numbers, validator indices, signatures, and timeouts; use change-view mechanisms
- VM/contract attacks: sandbox execution, gas metering, syscall whitelisting, stack and memory limits
- Cryptographic attacks: use validated curves, constant-time comparisons, secure RNG, zeroization of secrets

**Section sources**
- [docs/SECURITY.md:40-168](file://docs/SECURITY.md#L40-L168)

### Key Management Best Practices
- Generate keys with OS CSPRNG; wrap secrets in zeroizing containers
- Store keys in encrypted wallets or HSM/TEE; prefer hardware isolation for production
- Derive addresses deterministically and validate script hashes match expected accounts

**Section sources**
- [docs/SECURITY.md:276-333](file://docs/SECURITY.md#L276-L333)
- [neo-crypto/src/signature.rs:32-50](file://neo-crypto/src/signature.rs#L32-L50)
- [neo-crypto/src/ecc.rs:664-698](file://neo-crypto/src/ecc.rs#L664-L698)

### Secure Deployment and Access Control
- Harden RPC: authentication, CORS, method filtering, rate limiting
- Limit P2P connections per IP; enforce handshake timeouts and per-peer memory quotas
- Enable strict native checks only after parity validation for production consensus

**Section sources**
- [docs/SECURITY.md:483-607](file://docs/SECURITY.md#L483-L607)
- [docs/SECURITY.md:9-14](file://docs/SECURITY.md#L9-L14)

### Monitoring and Incident Response
- Monitor consensus message validation failures, witness rejections, and block validation errors
- Track rate limit triggers, peer reputation changes, and resource quota breaches
- Integrate alerting for TEE attestation failures and HSM connectivity issues

**Section sources**
- [neo-core/src/validation.rs:36-126](file://neo-core/src/validation.rs#L36-L126)
- [neo-tee/src/lib.rs:1-63](file://neo-tee/src/lib.rs#L1-L63)
- [neo-hsm/src/lib.rs:1-67](file://neo-hsm/src/lib.rs#L1-L67)

### Certificate Management
- Terminate TLS at reverse proxy; manage certificates centrally
- Rotate certificates regularly and monitor expiration
- Validate client certificates for admin endpoints if enabled

[No sources needed since this section provides general guidance]

### Audit Findings and References
- Multiple audits cover architecture, RPC compliance, full project, consensus, smart contracts, and transaction validation
- Refer to repository audit documents for detailed findings and remediation status

**Section sources**
- [docs/audits/2026-08-30-architecture-audit.md](file://docs/audits/2026-08-30-architecture-audit.md)
- [docs/audits/2026-09-01-rpc-json-rpc-compliance-audit.md](file://docs/audits/2026-09-01-rpc-json-rpc-compliance-audit.md)
- [docs/audits/2026-09-07-full-project-audit.md](file://docs/audits/2026-09-07-full-project-audit.md)
- [docs/audits/consensus-audit.md](file://docs/audits/consensus-audit.md)
- [docs/audits/smart-contract-native-audit.md](file://docs/audits/smart-contract-native-audit.md)
- [docs/audits/transaction-signature-audit.md](file://docs/audits/transaction-signature-audit.md)
- [docs/audits/transaction-validation-audit.md](file://docs/audits/transaction-validation-audit.md)

### Reporting Vulnerabilities
- Follow coordinated disclosure policy and contact information provided by the project

**Section sources**
- [SECURITY.md:1-16](file://SECURITY.md#L1-L16)
- [docs/SECURITY.md:737-780](file://docs/SECURITY.md#L737-L780)