# Neo Wallet & Crypto Parity Checklist (Rust vs. C#)

## Summary

The Rust `neo-wallet` and `neo-crypto` crates currently offer minimal keystore/account primitives and low-level crypto helpers. The C# codebase (`Neo.Wallets`, `Neo.Cryptography`, `Neo.Wallets.NEP6`, `Neo.Wallets.SQLite`) provides full NEP-6 wallet functionality, NEP-2 key management, signer scopes, contract scripts, transactions assembly, deterministic signatures, and script-hash utilities. This document captures the outstanding work required for parity.

## Components to Map

### Wallet Features (C# `Neo.Wallets`)
- **Account Model**
  - `WalletAccount` with script hash, contract, watcher flag, lock status.
  - Default account semantics, watch-only accounts, multi-sig support.
  - Contract script storage and parameter types.
- **NEP-6 Wallet**
  - `NEP6Wallet` JSON schema (name, version, scrypt parameters, accounts, extra).
  - `NEP6Account` structure with `address`, `label`, `isDefault`, `lock`, `key`, `contract`, `extra`.
  - Import/export of NEP-2 encrypted keys (`SecureString`, passphrase).
- **Signer / Witness**
  - `Signer` structure with scopes (`CalledByEntry`, `Global`, `CustomContracts`, `CustomGroups`, `WitnessRules`).
  - Witness rule expressions, groups, and contract-based restrictions.
- **Transaction Assembly**
  - Helper methods to build witnesses, add Cosigners, compute script hashes.
  - Integration with `ContractParametersContext`.
- **Persistence**
  - Storage backends (JSON NEP-6, SQLite).
  - Wallet migration and backup operations.
- **CLI integration**
  - Creating accounts, setting default, importing/exporting NEP-2, listing balances, claim GAS.

### Crypto Helpers (C# `Neo.Cryptography`, `Neo.Wallets` utilities)
- Deterministic ECDSA (RFC6979) for P-256.
- NEP-2 key derivation/wrapping/unwrapping (`ScryptParameters`, AES-256-ECB).
- Script hash conversions (`UInt160`, `Address`, Base58Check).
- Multisig contract creation (`Contract.CreateMultiSigContract`), standard account script.
- BLS/EDDSA support (if relevant for plugins).
- Sign/verify utilities aligning with `Neo.Crypto.ECC.ECKeyPair`.

## Current Rust Gaps

- `neo-wallet`:
  - `Account` struct only stores script hash and keys; no contract scripts, signer scopes, watch-only flags.
  - `Keystore` only handles raw key storage; no NEP-6 JSON schema, no NEP-2 encryption/decryption.
  - `Wallet` lacks transaction assembly, witness building, contract parameter context, or default account handling.
  - No support for multi-sig accounts, contract deployment helpers, or signer scopes.
- `neo-crypto`:
  - Provides core P-256 operations, AES, scrypt, but lacks deterministic signatures, NEP-2 wrappers, Base58Check utilities, script hash conversion helpers.
  - No direct support for C# `Contract.CreateSignatureRedeemScript`, multi-sig script generation, or witness serialization.

## Implementation Plan

1. **Schema & Serialization**
   - Define Rust structs for NEP-6 wallet (`WalletFile`, `AccountEntry`, `Contract`, `ScryptParameters`) mirroring C#.
   - Implement JSON (de)serialization using `serde`.
   - Add import/export of NEP-2 encrypted keys with configurable scrypt parameters.
   - Match optional fields (`extra`, `contract.parameters`, `contract.deployed`) and omit-null semantics exactly as in C# JSON.

2. **Account & Contract Support**
   - Expand `Account` model to hold script hash, contract script, parameter list, default/watch-only flags.
   - Provide functions for `Contract::new_standard`, `Contract::new_multisig`, witness rules, signer scopes.
   - Add helpers for base58 address encoding/decoding and script hash conversions.

3. **Crypto Enhancements**
   - Implement deterministic ECDSA signing (RFC6979) for secp256r1.
   - Add NEP-2 wrap/unwrap using scrypt + AES-256-ECB.
   - Provide Base58Check encoding/decoding functions, UInt160/UInt256 types with conversions.
   - Supply script building helpers (`ContractScriptBuilder`) for multi-sig and signature redemption script.

4. **Signer & Witness**
   - Model `Signer` with scopes and restrictions; integrate with `neo-contract` and `neo-runtime`.
   - Provide witness creation functions to include invocation/verification scripts.
   - Implement `ContractParametersContext` equivalent to manage multi-signatures.

5. **Wallet Operations**
   - Support account creation (new key pair), import from WIF, import NEP-2, export NEP-2.
   - Manage default account, list balances (requires runtime integration), claim GAS, send assets.
   - Provide CLI commands mirroring `neo-cli` operations.

6. **Storage Backends**
   - Implement JSON file storage with atomic write/backup semantics.
   - Optional: add SQLite backend for compatibility.

7. **Testing & Interop**
   - Add test vectors for NEP-2 wrap/unwrap and deterministic signatures (use C# generated data).
   - Validate NEP-6 serialization/deserialization parity with C#.
  - Build integration tests covering signer scopes and multi-sig interactions with runtime.

## Deliverables & Milestones

1. Crypto foundation (deterministic signatures, NEP-2 helpers, Base58/UInt160).
2. NEP-6 schema implementation with JSON import/export and key management.
3. Account/contract enhancements (multi-sig, witness rules, signer scopes).
4. Wallet operations & CLI integration (create/import/export via private key, WIF, NEP-2; watch-only management).
5. Integration with runtime for balance retrieval and transaction signing.

### Current Coverage

- ✅ Import/export via WIF and NEP-2 with configurable scrypt parameters (`neo-wallet`, `neo-node`, `neo-cli`).
- ✅ Watch-only account management and NEP-6 round-tripping.
- ✅ Signer scope model available (`CalledByEntry`, `Global`, custom contracts/groups placeholders) and persisted through NEP-6 `extra` metadata (query/update via `/wallet/accounts/detail` + `/wallet/update/signer`).
- ⏳ Multi-sig contracts, signer scopes, and witness rules remain outstanding.

### Phase 1 Detailed Notes

- **NEP-6 Schema**
  - JSON structure identical to `Neo.Wallets.NEP6.NEP6Wallet`:
    ```jsonc
    {
      "name": "example",
      "version": "1.0",
      "scrypt": { "n": 16384, "r": 8, "p": 8 },
      "accounts": [
        {
          "address": "N...",
          "label": "primary",
          "is_default": true,
          "lock": false,
          "key": "6PY...",
          "contract": {
            "script": "...",
            "parameters": [{ "name": "signature", "type": "Signature" }],
            "deployed": false
          },
          "extra": null
        }
      ],
      "extra": null
    }
    ```
  - `address` derived from `UInt160::to_address(AddressVersion)`.
  - `key` field present only when private key encrypted (NEP-2). Watch-only accounts omit it.

- **Interop Expectations**
  - Import/export must roundtrip using `neo_csharp/tests/Neo.UnitTests/Wallets/NEP6/UT_NEP6Account.cs`.
  - Scrypt parameters default to `(16384, 8, 8)` but we allow overriding for tests (low-cost).
  - Validate that decrypting `key` reconstructs script hash and contract script (signature redeem by default).

- **Signer Scope Primitives**
  - Introduce `SignerScope` enum and `Signer` struct with serialization matching `Neo.Network.P2P.Payloads.Signer`.
  - Reserve `WitnessRule` support for subsequent phases; document placeholders in code.

- **Testing**
  - Provide fixtures under `neo-wallet/tests/` referencing JSON blobs derived from C# repo when possible.
  - Ensure watch-only accounts remain readable in absence of `key`.

## Next Steps

- Draft parity checklist for contract manifest/runtime (if not already done) and for RPC.
  - Align `neo-contract` improvements (events, trust lists) with wallet features.
- Establish shared test vectors from C# node for NEP-2/NEP-6 functions.
- Coordinate wallet roadmap with consensus/runtime plans to ensure signing/execution workflows align.
