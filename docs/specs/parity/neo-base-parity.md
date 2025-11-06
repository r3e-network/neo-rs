# Neo Base Parity Checklist (Rust vs. C#)

## Scope

`neo-base` is the lowest layer shared by every other crate. The C# reference implementation spreads the equivalent functionality across `Neo.UInt160`, `Neo.UInt256`, `Neo.Helper`, and the binary reader/writer helpers under `Neo.IO`. This document tracks the strict fidelity we need before higher-level modules (wallet, consensus, runtime) can be considered feature complete.

## Type Parity Targets

### UInt160 / UInt256
- Fixed-size big endian byte arrays (`UInt160`, `UInt256`) with:
  - Default zero value and `Display`/`Debug` hex formatting identical to C# (`0x` prefix, uppercase).
  - `TryFrom<&[u8]>`, `From<[u8; N]>`, `as_slice`, `into_inner`.
  - Constant-time equality and ordering by big endian lexicographic order.
- Arithmetic is intentionally _out of scope_ (the C# types only expose comparison + bit operations).
- `NeoEncode`/`NeoDecode` via raw bytes (no varint prefix) mirroring `Neo.IO.BinaryWriter/Reader`.
- Serde string encoding using hex (C# serialises via `ToString()`).

### Hash Helpers
- Promote `Hash160`/`Hash256` wrappers to newtype aliases of `UInt160`/`UInt256`.
- Provide constructors for **double SHA256** and **hash160** to align with `Neo.Cryptography.Helper`.
- Ensure `Hash160::from_script` matches `Neo.SmartContract.Helper.ToScriptHash`.

### Address Encoding
- `AddressVersion` newtype (`u8`) so the wallet/runtime can carry protocol settings.
- `UInt160::to_address(AddressVersion)` returning Base58Check string with the version byte prepended (C# `Helper.ToAddress`).
- `UInt160::from_address(&str, AddressVersion)` validating version + checksum.
- Helper to convert between `UInt160` and `neo-crypto::ecc256::PublicKey::script_hash()` (to keep wallet wiring simple).

### Binary Codec
- Shared endian helpers to read/write fixed arrays without heap allocation.
- Guard against short reads with `DecodeError::InvalidLength` to match C# exceptions.

## Test Vectors

1. **UInt160 formatting**
   - `UInt160::from([0x52; 20])` ⇒ `"0x5252…52"` matching `UInt160.Parse`.
2. **Address roundtrip**
   - Script hash `0x8618383e5b58c50c66bc8a8e8e43725dc41c153c` with version `0x35` (from `neo_csharp/scripts/run-localnet-nodes.sh`) ⇒ address `"NRPf2BLaP595UFybH1nwrExJSt5ZGbKnjd"`.
3. **Hash conversions**
   - Signature redeem script from `neo_csharp/tests/Neo.UnitTests/Wallets/UT_Wallet.cs` must yield identical hash160.
4. **Serde roundtrip**
   - JSON encoding of `UInt256` must match the string produced by `UInt256.ToString()` (uppercase hex with `0x` prefix).

## Deliverables for Phase 1

- `UInt160`, `UInt256`, `AddressVersion` structs with codec + serde implementations.
- Address helpers (`to_address`, `from_address`) plus Base58Check dependency reuse.
- Unit tests covering the vectors above and no-std friendly constructors.
- Documentation comments pointing back to this spec and referencing the corresponding C# types.

## Follow-Up Work

- Integrate Merkle proof helpers once wallet/runtime require them.
- Consider exposing `be_bytes` accessors compatible with upcoming MPT / Patricia trie code.
- Evaluate deriving traits (`NeoEncode`, `NeoDecode`) via macros once parity stabilises.
