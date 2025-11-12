# Neo Contract Manifest & Runtime Parity Checklist (Rust vs. C#)

## Summary

The Rust `neo-contract` crate offers a trimmed-down manifest (name, groups, methods, permissions) and a basic runtime context. The C# implementation (`Neo.SmartContract.Manifest`, `Neo.SmartContract.ContractState`, `Neo.SmartContract.Native`, `Neo.SmartContract.Managers`) provides a rich Manifest model, ABI metadata, permissions, features, contract migration, and runtime integration. This document enumerates gaps and outlines the work needed for parity.

## Components to Map

1. **Contract Manifest (C# `ContractManifest.cs`)**
   - Fields: `Name`, `Groups`, `Features`, `SupportedStandards`, `Abi` (methods/events), `Permissions`, `Trusts`, `Extra`.
   - ABI elements: `ContractAbi`, `ContractMethodDescriptor`, `ContractParameterDefinition`, `ContractEventDescriptor`.
   - Serialization (JSON, binary) with validation rules (no duplicate methods, safe method flags).

2. **Contract State (`ContractState.cs`)**
   - Storage of script, manifest, contract ID, update counter, nef (Neo Executable Format).
   - Nef file structure (tokens, stack item types, checksum).

3. **Contract Management (`NativeContract.Management`)**
   - Deploy, update, destroy logic.
   - Nef and manifest validation, ACL updates (trusts, permissions).
   - Contract migration and event notifications.

4. **Contract Features**
   - `features.callFlags`, `features.storage`, `features.payable`.
   - `SupportedStandards` tracking.
   - Signer and witness rules integration.

5. **Contract Runtime**
   - Execution context (`ApplicationEngine`), call flags enforcement, storage context.
   - Event emission and notification structure.

6. **Manifest Permissions**
   - Call and notify permissions (`ContractPermission` type), wildcards, specific methods.
   - Trusts (authorized contracts/groups).
   - Witness rules (additions from N3).

7. **Testing & Validation**
   - Manifest JSON schema validation.
   - NEF validation (checksum, tokens).
   - Contract deployment/invocation flows.

## Current Rust Gaps

- Manifest lacks: `features`, `supported_standards`, `abi` (method/event descriptors), `trusts`, `extra`, event descriptors, parameter types beyond Boolean/Integer/ByteArray/String.
- No NEF handling, contract bytecode packaging, or checksum verification.
- No contract management functions (deploy/update/destroy).
- Runtime lacks call flag enforcement, storage access, event notifications.
- Permissions limited to a single enum; no wildcard or method-scoped permissions.
- No JSON serialization/deserialization for manifest parity.

## Implementation Plan

1. **Manifest Expansion**
   - Extend `ContractManifest` struct with all C# fields (features, standards, abi, trusts, extra).
   - Define ABI descriptors for methods and events, including parameter types enumerating all `StackItemType`s.
   - Implement JSON (de)serialization conforming to C# schema; enforce validation rules.

2. **NEF & Contract State**
   - Introduce NEF struct with header, checksum, tokens.
   - Expand contract state to include NEF, manifest, update counter, ID.
   - Provide functions to compute checksum and validate NEF size limits.

3. **Permissions & Trusts**
   - Model `ContractPermission` (contract hash/group wildcards + methods wildcard).
   - Implement trusts (allowed `UInt160` or groups).
   - Support witness rules/wildcards as in C#.

4. **Features & Standards**
   - Add `ContractFeatures` flags (storage, payable, dynamic invoke).
   - Track supported standards (e.g., NEP-17, NEP-11).

5. **Runtime Integration**
   - Enforce call flags when invoking native contracts and storage operations.
   - Provide event notification API for contracts, mirroring C# `Notify`.
   - Plug into `neo-runtime` to store contract states and handle migration/destroy events.

6. **Management & Native Contracts**
   - Implement contract deployment/update/destroy flows similar to `NativeContract.Management`.
   - Validate manifest changes during update (permissions/trust restrictions).
   - Emit notifications and update contract indexes.

7. **Testing**
   - Port manifest validation tests from C# (json parsing and constraints).
   - Add NEF checksum tests using known C# outputs.
   - Build integration tests for contract deployment, invoking methods/events, updating contracts.

## Deliverables

1. Manifest/NEF compatibility (structures + serialization).
2. Permissions/trusts/ABI features matching C#.
3. Runtime enforcement (call flags, storage, events).
4. Contract management flow integrated with runtime.

## Status Snapshot

- ✅ Manifest/ABI/NEF validation mirrors C# (checksum + manifest rules).
- ✅ Runtime host enforces storage context access and call flags for storage/notify syscalls; iterator handles feed VM `System.Storage.Find/Next`.
- ⏳ `System.Storage.Find` still lacks `DeserializeValues`/`PickField*` semantics, and call flags are not yet threaded through contract invocation/deploy flows.

## Next Steps

- Align manifest changes with wallet signer scopes (see wallet parity doc).
- Coordinate runtime development to support contract storage and event logging.
- Gather reference manifests/NEFs from the C# node to use as golden fixtures for testing.
