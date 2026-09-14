# Security Best Practices

<cite>
**Referenced Files in This Document**
- [SECURITY.md](file://SECURITY.md)
- [ARCHITECTURE.md](file://docs/ARCHITECTURE.md)
- [security_fixes.rs](file://neo-core/src/smart_contract/native/security_fixes.rs)
- [witness_rule.rs](file://neo-core/src/witness_rule.rs)
- [witness_rule_action.rs](file://neo-primitives/src/witness_rule_action.rs)
- [pkcs11_signer.rs](file://neo-hsm/src/pkcs11/pkcs11_signer.rs)
- [sealing.rs](file://neo-tee/src/enclave/sealing.rs)
- [fees_events_native.rs](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs)
- [smart-contract-native-audit.md](file://docs/audits/smart-contract-native-audit.md)
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
This document provides comprehensive security guidance for developing smart contracts on Neo-RS. It focuses on preventing common vulnerabilities such as reentrancy, integer overflow/underflow, access control issues, and gas limit exploitation. It also covers secure coding practices, input validation, proper use of witness rules for authorization, cryptographic best practices, key management patterns, safe interaction with native contracts, secure contract patterns, vulnerability detection methods, audit checklists, gas optimization security implications, and safe upgrade strategies.

## Project Structure
Neo-RS organizes security-related functionality across several modules:
- Native contract security utilities (reentrancy guards, safe arithmetic, state consistency checks)
- Witness rule system for fine-grained authorization
- HSM and TEE integrations for secure key handling and data sealing
- Gas accounting and enforcement to prevent overconsumption
- Audit reports and architectural security mechanisms

```mermaid
graph TB
subgraph "Smart Contract Runtime"
AE["Application Engine"]
NF["Native Contracts"]
end
subgraph "Security Utilities"
SF["Security Fixes<br/>Reentrancy Guards & Safe Arithmetic"]
WR["Witness Rules"]
end
subgraph "Cryptography & Keys"
HSM["HSM Signer"]
TEE["TEE Sealing"]
end
subgraph "Gas & Limits"
GAS["Gas Accounting & Limits"]
end
AE --> NF
NF --> SF
AE --> GAS
AE --> WR
NF --> WR
HSM --> AE
TEE --> AE
```

**Diagram sources**
- [security_fixes.rs:1-30](file://neo-core/src/smart_contract/native/security_fixes.rs#L1-L30)
- [witness_rule.rs:1-24](file://neo-core/src/witness_rule.rs#L1-L24)
- [pkcs11_signer.rs:137-258](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L137-L258)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)

**Section sources**
- [security_fixes.rs:1-30](file://neo-core/src/smart_contract/native/security_fixes.rs#L1-L30)
- [witness_rule.rs:1-24](file://neo-core/src/witness_rule.rs#L1-L24)
- [pkcs11_signer.rs:137-258](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L137-L258)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)

## Core Components
- Reentrancy protection and safe arithmetic for native operations
- Witness scopes and rules for precise authorization
- Secure key management via HSM and TEE
- Gas consumption tracking and enforcement
- Audit-driven hardening of native contracts

Key responsibilities:
- Prevent reentrant calls into sensitive native operations
- Ensure all numeric operations are checked for overflow/underflow
- Enforce strict authorization using witness conditions and scopes
- Safely derive, store, and use keys within HSM/TEE boundaries
- Track and enforce gas limits to avoid DoS or unexpected behavior

**Section sources**
- [security_fixes.rs:21-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L21-L109)
- [witness_rule_action.rs:1-96](file://neo-primitives/src/witness_rule_action.rs#L1-L96)
- [pkcs11_signer.rs:183-225](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L183-L225)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)

## Architecture Overview
The runtime enforces security through layered controls:
- Application Engine coordinates execution and gas accounting
- Native contracts perform state changes under reentrancy guards and safe arithmetic
- Witness rules validate authorization per call context
- HSM/TEE provide secure key material and sealed storage

```mermaid
sequenceDiagram
participant Caller as "Caller Contract"
participant AE as "Application Engine"
participant NF as "Native Contract"
participant SEC as "Security Context"
participant GAS as "Gas Manager"
participant WR as "Witness Rules"
Caller->>AE : Invoke method
AE->>WR : Evaluate witness scope/rules
WR-->>AE : Authorized?
AE->>NF : Enter guarded section
NF->>SEC : enter_guard(type)
SEC-->>NF : Guard acquired
NF->>GAS : consume_gas(amount)
GAS-->>NF : OK / Out-of-gas
NF->>NF : Safe arithmetic ops
NF-->>AE : State change committed
AE-->>Caller : Result
```

**Diagram sources**
- [security_fixes.rs:55-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L55-L109)
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)
- [witness_rule.rs:1-24](file://neo-core/src/witness_rule.rs#L1-L24)

## Detailed Component Analysis

### Reentrancy Protection and Safe Arithmetic
- Thread-local guard set tracks active reentrancy contexts for critical operations (e.g., token transfers, mint/burn, contract lifecycle).
- RAII guard ensures automatic cleanup; entering an already-guarded operation fails fast.
- Safe arithmetic helpers prevent overflow/underflow and validate balance/supply changes.

```mermaid
flowchart TD
Start(["Enter Sensitive Operation"]) --> CheckGuard["Check if guard active"]
CheckGuard --> |Active| Deny["Reject: Reentrancy detected"]
CheckGuard --> |Inactive| Acquire["Acquire guard"]
Acquire --> Ops["Perform safe arithmetic & state updates"]
Ops --> Release["Release guard on exit"]
Release --> End(["Exit safely"])
```

**Diagram sources**
- [security_fixes.rs:21-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L21-L109)
- [security_fixes.rs:160-193](file://neo-core/src/smart_contract/native/security_fixes.rs#L160-L193)

**Section sources**
- [security_fixes.rs:21-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L21-L109)
- [security_fixes.rs:160-193](file://neo-core/src/smart_contract/native/security_fixes.rs#L160-L193)

### Access Control via Witness Scopes and Rules
- Witness scopes restrict what a transaction signature can authorize (e.g., entry-only, specific contracts/groups, global discouraged).
- Witness rules enable fine-grained conditional authorization combining boolean logic, script hashes, groups, and call context.
- Default action is deny when unknown values are encountered, ensuring fail-safe behavior.

```mermaid
classDiagram
class WitnessScope {
+None
+CalledByEntry
+CustomContracts
+CustomGroups
+Global
}
class WitnessCondition {
+Boolean
+Not
+And
+Or
+ScriptHash
+Group
+CalledByEntry
+CalledByContract
+CalledByGroup
}
class WitnessRuleAction {
+Deny
+Allow
}
WitnessScope <.. WitnessCondition : "used by"
WitnessRuleAction <.. WitnessCondition : "evaluated against"
```

**Diagram sources**
- [ARCHITECTURE.md:945-986](file://docs/ARCHITECTURE.md#L945-L986)
- [witness_rule_action.rs:1-96](file://neo-primitives/src/witness_rule_action.rs#L1-L96)

**Section sources**
- [ARCHITECTURE.md:945-986](file://docs/ARCHITECTURE.md#L945-L986)
- [witness_rule_action.rs:1-96](file://neo-primitives/src/witness_rule_action.rs#L1-L96)

### Cryptographic Best Practices and Key Management
- HSM integration validates EC parameters, normalizes public keys, and derives script hashes from securely stored keys.
- TEE sealing uses AES-GCM with AAD binding and counter inclusion to prevent tampering; derived keys are zeroized after use.
- Prefer HSM/TEE for private key usage and sensitive data; never expose raw secrets to VM or user space.

```mermaid
sequenceDiagram
participant App as "App"
participant HSM as "HSM Signer"
participant TEE as "TEE Sealing"
App->>HSM : Request signing with key_id
HSM->>HSM : Validate EC params & normalize key
HSM-->>App : Signature
App->>TEE : Seal data with context & AAD
TEE-->>App : Sealed payload
App->>TEE : Unseal with same key/context
TEE-->>App : Plaintext (or error on tamper)
```

**Diagram sources**
- [pkcs11_signer.rs:183-225](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L183-L225)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)

**Section sources**
- [pkcs11_signer.rs:183-225](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L183-L225)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)

### Gas Limit Exploitation Prevention
- Gas consumption is validated for non-negative values and checked for overflow during accumulation.
- VM gas counters are enforced to prevent exceeding configured limits.
- Always bound loops and unbounded operations; prefer bounded iteration and early exits.

```mermaid
flowchart TD
Ingest["consume_gas(gas)"] --> NegCheck{"gas >= 0?"}
NegCheck --> |No| ErrNeg["Error: Negative gas"]
NegCheck --> |Yes| Mul["Multiply by factor (checked)"]
Mul --> Add["Add to consumed (checked)"]
Add --> Limit{"Exceeds limit?"}
Limit --> |Yes| ErrGas["Error: Out of gas"]
Limit --> |No| Update["Update counters"]
Update --> Done(["OK"])
```

**Diagram sources**
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)

**Section sources**
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)

### Secure Interaction with Native Contracts
- Use reentrancy guards around any state-changing native calls.
- Apply safe arithmetic for balances and supplies; validate constraints before committing state.
- Restrict access via witness rules to minimize attack surface.

```mermaid
sequenceDiagram
participant C as "Contract"
participant N as "Native Contract"
participant G as "Guard"
C->>N : Transfer/Mint/Burn
N->>G : enter_guard(Transfer)
G-->>N : Guard acquired
N->>N : Safe arithmetic & validations
N-->>C : Commit or revert
Note over N,G : Guard released automatically
```

**Diagram sources**
- [security_fixes.rs:55-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L55-L109)
- [security_fixes.rs:160-193](file://neo-core/src/smart_contract/native/security_fixes.rs#L160-L193)

**Section sources**
- [security_fixes.rs:55-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L55-L109)
- [security_fixes.rs:160-193](file://neo-core/src/smart_contract/native/security_fixes.rs#L160-L193)

### Vulnerability Detection Methods
- Enable strict security mode via environment flag to enforce guards in tests and controlled environments.
- Run protocol parity and native compatibility checks to detect deviations that may indicate vulnerabilities.
- Use audits and comparison scripts to validate behavior across implementations.

**Section sources**
- [security_fixes.rs:26-30](file://neo-core/src/smart_contract/native/security_fixes.rs#L26-L30)
- [smart-contract-native-audit.md:1-14](file://docs/audits/smart-contract-native-audit.md#L1-L14)

### Audit Checklist for Smart Contracts
- Reentrancy: Ensure all state-changing paths are guarded; no external calls before finalization.
- Integer Safety: All arithmetic uses checked operations; validate bounds and non-negativity.
- Access Control: Enforce witness scopes/rules; avoid Global scope unless absolutely necessary.
- Gas: Bound loops; check gas consumption; ensure no path exceeds limits.
- Crypto: Use approved primitives; validate curves and points; never handle raw private keys outside HSM/TEE.
- Upgrades: Implement immutable upgrades with clear migration steps; preserve state invariants.
- Testing: Include fuzzing, property-based tests, and cross-implementation parity checks.

[No sources needed since this section provides general guidance]

### Gas Optimization Security Implications
- Optimizations must not bypass safety checks; always preserve overflow/underflow checks and gas limits.
- Avoid introducing unbounded recursion or dynamic allocations that could be abused.
- Profile gas usage to ensure worst-case paths remain within limits.

[No sources needed since this section provides general guidance]

### Safe Contract Upgrade Strategies
- Use versioned storage prefixes and migration functions; validate invariants post-migration.
- Employ multi-step upgrades with governance or time-locks; allow rollback windows where feasible.
- Keep upgrade logic minimal and well-tested; rely on native reentrancy guards and safe arithmetic.

[No sources needed since this section provides general guidance]

## Dependency Analysis
```mermaid
graph LR
AE["Application Engine"] --> GAS["Gas Accounting"]
AE --> NF["Native Contracts"]
NF --> SEC["Security Fixes"]
NF --> WR["Witness Rules"]
HSM["HSM Signer"] --> AE
TEE["TEE Sealing"] --> AE
```

**Diagram sources**
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)
- [security_fixes.rs:1-30](file://neo-core/src/smart_contract/native/security_fixes.rs#L1-L30)
- [witness_rule.rs:1-24](file://neo-core/src/witness_rule.rs#L1-L24)
- [pkcs11_signer.rs:137-258](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L137-L258)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)

**Section sources**
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)
- [security_fixes.rs:1-30](file://neo-core/src/smart_contract/native/security_fixes.rs#L1-L30)
- [witness_rule.rs:1-24](file://neo-core/src/witness_rule.rs#L1-L24)
- [pkcs11_signer.rs:137-258](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L137-L258)
- [sealing.rs:182-228](file://neo-tee/src/enclave/sealing.rs#L182-L228)

## Performance Considerations
- Prefer constant-time crypto operations where applicable; avoid branching on secret data.
- Minimize memory allocations inside hot paths; reuse buffers when safe.
- Batch operations to reduce syscall overhead while maintaining atomicity and invariants.
- Monitor gas usage closely; optimize loops and storage reads/writes.

[No sources needed since this section provides general guidance]

## Troubleshooting Guide
Common issues and mitigations:
- Reentrancy detected: Ensure each sensitive operation acquires a guard exactly once; verify call chains do not loop back.
- Out of gas: Reduce complexity; add early returns; profile worst-case paths.
- Overflow/Underflow: Replace unchecked math with checked variants; validate inputs and intermediate results.
- Invalid witness rule/action: Ensure only known enum values are used; default to deny for unknowns.
- Key errors: Validate EC parameters; confirm HSM key presence and correct ID/label mapping.

**Section sources**
- [security_fixes.rs:55-109](file://neo-core/src/smart_contract/native/security_fixes.rs#L55-L109)
- [fees_events_native.rs:389-432](file://neo-core/src/smart_contract/application_engine/fees_events_native.rs#L389-L432)
- [witness_rule_action.rs:1-96](file://neo-primitives/src/witness_rule_action.rs#L1-L96)
- [pkcs11_signer.rs:183-225](file://neo-hsm/src/pkcs11/pkcs11_signer.rs#L183-L225)

## Conclusion
Secure smart contract development on Neo-RS requires disciplined use of reentrancy guards, safe arithmetic, strict access control via witness rules, robust gas accounting, and secure key management through HSM/TEE. Adopting the patterns and checklists in this document will significantly reduce risk and improve resilience against common vulnerabilities.

[No sources needed since this section summarizes without analyzing specific files]

## Appendices

### Reporting Vulnerabilities
Follow coordinated disclosure practices and contact the security team with detailed reproduction steps.

**Section sources**
- [SECURITY.md:1-16](file://SECURITY.md#L1-L16)