# StdLib & CryptoLib Edge Case Investigation

**Date:** 2026-07-03
**Reference:** neo-rs vs C# v3.10.0

## Summary

| # | Issue | Status |
|---|-------|--------|
| 1 | atoi leniency | Already correct |
| 2 | memorySearch backward off-by-one | Already correct |
| 3 | base64Decode invalid-char handling | Already correct |
| 4 | recoverSecp256K1 recovery ids & 64-byte | **BUG FIXED** |
| 5 | CryptoLib Gorgon strict verify variants | Already correct |

## Detailed Analysis

### 1. atoi leniency — `std_lib/mod.rs:242` — ALREADY CORRECT

**Rust behavior** (`parse_dotnet_decimal` at line 385-400): Strips a single leading `+` or `-`, then requires all remaining chars to be decimal digits (via `is_ascii_digit()` before `BigInt::parse_bytes`).

**C# behavior**: `BigInteger.Parse(value, NumberStyles.AllowLeadingSign)` permits an optional leading `+`/`-` followed by decimal digits only. No `AllowLeadingWhite` or `AllowTrailingWhite` is set.

**Edge case verification**:
| Input | Rust result | C# result | Match? |
|-------|------------|-----------|--------|
| `"123"` | Ok(123) | 123 | ✓ |
| `"+123"` | Ok(123) | 123 | ✓ |
| `"-123"` | Ok(-123) | -123 | ✓ |
| `" 1"` | Error | Error (FormatException) | ✓ |
| `"1 "` | Error | Error | ✓ |
| `"--1"` | Error | Error (double sign) | ✓ |
| `"+-1"` | Error | Error | ✓ |
| `"+"` | Error | Error (empty digits) | ✓ |
| `"-"` | Error | Error | ✓ |
| `""` | Error | Error | ✓ |
| `"1.0"` | Error | Error | ✓ |
| `"00123"` | Ok(123) | 123 (leading zeros ok) | ✓ |
| `"0"` | Ok(0) | 0 | ✓ |
| `"-0"` | Ok(0) | 0 | ✓ |

**Verdict**: No divergence. The Rust implementation correctly limits to a single leading sign + decimal digits, matching C#'s `AllowLeadingSign` exactly.

---

### 2. memorySearch backward off-by-one — `std_lib/mod.rs:376` — ALREADY CORRECT

**C# reference**: `MemorySearch(mem, value, start, backward=true)` calls `mem.AsSpan(0, start).LastIndexOf(value)`. `AsSpan(0, start)` creates a span of length `start` covering indices `[0, start)` (exclusive end).

**Rust implementation** (`memory_search`, line 175-184): When `backward=true`, calls `last_index_of(&mem[..start], value)`. The Rust slice `&mem[..start]` is also exclusive at `start`, covering `[0, start)`.

**Edge case verification**:
| Case | Rust result | C# result | Match? |
|------|------------|-----------|--------|
| `"hello world", "o", start=11`, backward | 7 | 7 | ✓ |
| `"hello world", "o", start=5`, backward | 4 | 4 | ✓ |
| `"abc", "a", start=0`, backward, non-empty needle | -1 | -1 | ✓ |
| `"abc", "a", start=3`, backward | 0 | 0 | ✓ |
| `"abc", "a", start=4`, backward | 0 (search whole string) | 0 | ✓ |
| Empty needle, start=5, mem="abcde" | 5 (haystack.len()) | 5 (span.Length) | ✓ |
| Needle longer than mem[..start] | -1 | -1 | ✓ |

**Verdict**: No divergence. The `&mem[..start]` slice exactly matches C#'s `AsSpan(0, start)`. The `last_index_of` helper correctly implements .NET's `LastIndexOf` semantics (including empty needle returning `haystack.len()`).

---

### 3. base64Decode invalid-char handling — `encoding.rs:17` — ALREADY CORRECT

**C# behavior**: `Convert.FromBase64String` strips exactly four whitespace characters (space `0x20`, `\t`, `\n`, `\r`) from the input, then strictly decodes the remainder. Any other character, including other whitespace (vertical tab `0x0B`, form feed `0x0C`), causes a `FormatException`.

**Rust implementation** (`base64_decode_impl`, line 17-33):
1. Strips only `{' ', '\t', '\n', '\r'}` from the input
2. Calls `base64::decode_strict()` which uses `GeneralPurpose::new(alphabet::STANDARD, ...)` — the base64 crate rejects any character not in `[A-Za-z0-9+/=]`

**Verification**:
- Standard base64 chars: accept, decode ✓
- `=` padding: accept (required, canonical padding) ✓
- `@` non-alphabet: survives strip, rejected by strict decode ✓
- Vertical tab `\x0B`: survives strip, rejected by strict decode ✓  
- Space `0x20`: stripped before decode, not seen by strict decode ✓
- Non-multiple-of-4 length: rejected by strict decode ✓
- Non-canonical trailing bits: tolerated (`with_decode_allow_trailing_bits(true)`, matching .NET) ✓

Test already covers vertical tab rejection (`b"AQI\x0bD"` → error at line 54).

**Verdict**: No divergence. Only the 4 .NET-tolerated whitespace chars are stripped; all other invalid chars fault.

---

### 4. recoverSecp256K1 — `crypto_lib/mod.rs:165-171` — **BUG FIXED**

#### 4a. 64-byte signature acceptance (CONFIRMED BUG)

**C# behavior** (verified against neo-project v3.10.0):
- `Crypto.ECRecover(messageHash, signature)` first checks `if (signature.Length != 65) throw new ArgumentException(nameof(signature))`
- `RecoverSecp256K1` wraps in try/catch → returns `null` on exception
- A 64-byte signature → `ArgumentException` → `null`

**Rust behavior before fix**: `recover_secp256k1_method` at line 174 accepted both 65-byte and 64-byte signatures:
```rust
if signature.len() != 65 && signature.len() != 64 {
    return None;
}
```
A 64-byte signature that recovered → `Some(pubkey)` where C# returns `null`.

**Impact**: Any contract calling `recoverSecp256K1` with a 64-byte EIP-2098 signature would see a valid pubkey on Rust nodes vs null on C# nodes — a direct execution divergence. HF_Echidna is live on MainNet (block 7,300,000+), making this reachable today.

**Fix applied**: Changed `recover_secp256k1_method` to require exactly 65 bytes:
```rust
if signature.len() != 65 {
    return None;
}
```
The `Secp256k1Crypto::recover_public_key` utility (non-consensus) retains 64-byte support.

#### 4b. Recovery IDs 2-3 (NO BUG)

**C# behavior**: Uses BouncyCastle `ECDomainParameters.RecoverFromSignature(recId, ...)` which accepts `recId` in range `0..=3`.
**Rust behavior**: `secp256k1` crate's `RecoveryId::from_i32(i32::from(rec_id))` also accepts `0..=3`.

**Verdict**: Recovery IDs 2-3 are correctly accepted by both implementations. No divergence.

#### Test changes:
- Renamed `recover_secp256k1_accepts_64_byte_compact_signature_like_csharp` → `recover_secp256k1_rejects_64_byte_compact_signature_like_csharp`
- Changed assertion from `is_some()` to `is_none()` for 64-byte signature
- Added 64-byte rejection assertion to `recover_secp256k1_returns_none_on_bad_input`

---

### 5. CryptoLib Gorgon strict verify variants — `crypto_lib/mod.rs:94` — ALREADY CORRECT

**Rust implementation**: `verify_ecdsa_method` correctly implements V0/V1/V2 dispatch:

- **V2 (Gorgon, `gorgon=true`)**: Checks signature length (faults on non-64), decodes public key (faults on bad key), calls `verify_signature_with_hash` (propagates errors as faults). Matches C# `VerifyWithECDsaV2`.
- **V0/V1 (pre-Gorgon, `gorgon=false`)**: Decodes key FIRST (faults on bad key — C# `DecodePoint` throws non-ArgumentException), then checks signature length (returns false), then verifies (returns false on error). Matches C# `VerifyWithECDsaV0/V1`.

The three-version registration (V0 genesis, V1 Cockatrice, V2 Gorgon) is correctly defined in `metadata.rs`.

For `verifyWithEd25519`, V0 (Echidna→Gorgon, returns false on bad length) and V1 (Gorgon+, faults on bad length) are also correctly implemented.

**Schnorr verification**: C# v3.10.0 does not include Schnorr — this is a post-3.10.0 feature and out of scope.

**Verdict**: No divergence. The Rust code correctly implements Gorgon strict verification for both ECDSA and Ed25519 variants.

---

## Files Changed

| File | Change |
|------|--------|
| `neo-native-contracts/src/crypto_lib/mod.rs` | Fixed `recover_secp256k1_method` to accept only 65-byte signatures (was accepting 64-byte as well) |
| `neo-native-contracts/src/tests/crypto_lib/mod.rs` | Updated comment/assertions: 64-byte signature now expected to return None; added explicit 64-byte rejection test |

## Verification

```
$ cargo check -p neo-native-contracts  ✓ (pass)
$ cargo test -p neo-native-contracts -- recover_secp256k1  ✓ (pass)
```
