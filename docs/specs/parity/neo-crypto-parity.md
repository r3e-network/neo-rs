# Neo Crypto Parity Checklist (Rust vs. C#)

## Summary

The Rust `neo-crypto` crate currently exposes core primitives (secp256r1 key generation/sign/verify, AES-256-ECB, scrypt). The C# implementation (`Neo.Cryptography`, `Neo.Cryptography.ECC`, `Neo.Cryptography.MPTTrie`, `Neo.Cryptography.BLS12_381`) offers a richer set of utilities for deterministic signatures, NEP-2 key wrapping, Base58Check, Merkle proof helpers, and optional cryptographic extensions (BLS/EDDSA). This document lists the missing functionality and outlines parity work.

## Components to Map

1. **ECC & Signatures (`Neo.Cryptography.ECC`)**
   - ECPoint/ECFieldElement, deterministic ECDSA (RFC6979) for P-256.
   - Signature encoding/decoding, canonicalization.
   - Keypair utilities (WIF import/export).

2. **NEP-2 / NEP-6 Helpers (`Neo.Wallets`)**
   - Scrypt parameter customization (N, r, p).
   - NEP-2 encryption/decryption (Base58Check, AES-256-ECB).
   - Key derivation for NEP-6 accounts.

3. **Hashing & Encoding (`Neo.Cryptography`)**
   - SHA256, RIPEMD160, double SHA256 wrappers.
   - Murmur, Blake2b (used for bloom filters, contract IDs).
   - Base58Check encoding/decoding utilities.
   - Script hash conversion (`UInt160`, `UInt256` conversions).

4. **Merkle / MPT**
   - Merkle Tree, Merkle proof verification helper.
   - MPT trie utilities for state root (already partly managed by `Neo.Cryptography.MPTTrie`).

5. **BLS / Additional Algorithms**
   - If BLS12_381 support is desired (used by plugins), map wrappers.

## Current Rust Gaps

- Deterministic ECDSA signing (RFC6979) absent; `Secp256r1Sign` likely random nonces.
- No NEP-2/NEP-6 key wrapping helpers (Base58Check, AES).
- No Base58Check encoding/decoding, WIF handling.
- Missing hash wrappers for double SHA256, Murmur, Blake2b.
- No Merkle tree utilities (though `neo-base` has simple Merkle root builder, needs proof support).
- No script hash conversions to/from addresses.
- No support for MPT operations or state root calculations.

## Implementation Plan

1. **Deterministic Signatures**
   - Implement RFC6979 nonce generation for secp256r1, matching C# deterministic signing.
   - Provide APIs for signing with and without deterministic option, verifying canonical form.
   - Enforce low-S normalisation (C# `ECDsaSignature.IsLowS`) so signatures are canonical.
   - Ensure signature serialisation uses fixed 64-byte `(r || s)` layout (`Neo.Cryptography.ECC.ECDsaSignature`).

2. **Hash & Encoding Utilities**
   - Add SHA256, double SHA256, RIPEMD160, Murmur128/256, Blake2b hash functions with fixed output wrappers (`Hash160`, `Hash256`).
   - Implement Base58Check encoding/decoding.
   - Provide conversions between byte arrays, `UInt160`, `UInt256`, Base58 addresses.

3. **NEP-2 / NEP-6 Helpers**
   - Implement NEP-2 key wrap/unwrap (scrypt -> AES-256-ECB -> Base58Check).
   - Expose `ScryptParams` struct matching C# defaults (N=16384, r=8, p=8).
   - Support WIF import/export.

4. **Merkle / State Utilities**
   - Provide Merkle proof verifier helpers (leaf inclusion).
   - Optional: port MPT trie logic or integrate with `neo-runtime` state root plan.

5. **BLS/Optional Algorithms**
   - Evaluate whether BLS12_381 support is required; if yes, map to Rust crate (e.g., `blst`).

6. **Testing**
   - Add test vectors for NEP-2, deterministic signatures, Base58Check using C# outputs.
   - Ensure hash outputs match C# reference implementation.

### Phase 1 Detailed Notes

- **Deterministic Signing API**
  - Mirror `Neo.Cryptography.Crypto.Sign` overloads: `sign(message, private_key)` defaulting to deterministic mode.
  - Provide explicit `sign_with_random_nonce` for edge cases (kept crate-internal for now).
  - Use `neo_base::hash::sha256` when callers pass arbitrary byte payloads to follow C# behaviour (message hashed internally).

- **NEP-2 Encode/Decode**
  - Input: 32-byte private key, passphrase bytes, address version (default `ProtocolSettings.Default.AddressVersion`), scrypt parameters `(N, r, p)`.
  - Steps:
    1. Derive 64-byte key via scrypt (`passphrase`, salt = first 4 bytes of `hash160` of the corresponding public key + two zero bytes, params `(N, r, p)`).
    2. XOR private key with derived half (`derived[0..32]`), encrypt using AES-256-ECB with key `derived[32..64]`.
    3. Assemble payload: `0x01 0x42 <flag>` (set compressed + hasher bits like C#) followed by salt and cipher text.
    4. Output Base58Check string.
  - Decode reverses the process and validates checksum + derived address.

- **Base58Check Helpers**
  - Expose `to_base58_check(version_byte, payload)` and `from_base58_check` returning version + payload like `Helper.ToAddress` and `ToScriptHash`.
  - Keep helpers `no_std` by relying on `bs58` + `alloc`.

- **Test Fixtures**
  - Adopt vectors from `neo_csharp/tests/Neo.UnitTests/Wallets/UT_Wallet.cs` (`nep2Key`, pass `"pwd"`, scrypt `N=2, r=1, p=1`) and from `scripts/run-localnet-nodes.sh`.
  - Confirm deterministic signing against constants in `neo_csharp/tests/Neo.UnitTests/Cryptography/UT_Crypto.cs`.

## Deliverables

1. Deterministic signature API with tests.
2. NEP-2 wrap/unwrap and Base58Check utilities.
3. Hash/Merkle helper functions.
4. Address/script hash conversion helpers integrated with wallet/runtime.

## Next Steps

- Align crypto roadmap with wallet module (NEP-6, signer scopes) and runtime (script hash conversions).
- Establish shared test fixtures derived from C# code to guarantee parity.
