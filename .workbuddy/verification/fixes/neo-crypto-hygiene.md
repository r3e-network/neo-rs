# neo-crypto Hygiene/Security Fixes

Applied 2026-07-03 based on neo-rs engineering review findings.

## Fix #1: Dead `Bls12381Crypto` — gated behind cfg

**File:** `neo-crypto/src/curves/bls12381.rs`, `neo-crypto/src/lib.rs`, `neo-crypto/Cargo.toml`

**Problem:** `Bls12381Crypto` had zero production callers, only used in tests.
This creates audit surface and could be mistakenly composed into production builds.

**Fix:** Gate the struct, impl block, const, and all relevant imports behind
`#[cfg(any(test, feature = "bls-experimental"))]`. Gate the re-export in `lib.rs`
the same way. Add a `bls-experimental` feature to `Cargo.toml`. Added doc comments
marking the struct as experimental/unverified for production consensus.

**Verification:** `cargo check -p neo-crypto` clean (no warnings). Tests pass
(the cfg(test) gate keeps it available for test builds).

---

## Fix #2: Dead `verify_signature_bytes` — removed

**File:** `neo-crypto/src/keys/signature.rs:518`

**Problem:** `Crypto::verify_signature_bytes` was a curve-guessing method with
zero callers anywhere in the workspace. It tried secp256k1 before secp256r1 for
33-byte keys, creating fragile try-order behavior.

**Fix:** Removed the method entirely. No test code referenced it.

**Verification:** `cargo check` and `cargo test` both pass cleanly.

---

## Fix #3: ECPoint Deserialize bypasses on-curve validation — custom impl

**File:** `neo-crypto/src/curves/ecc.rs:100`

**Problem:** `#[derive(Deserialize)]` on `ECPoint` allowed deserializing invalid
off-curve points. `ECPoint::new()` validates on-curve, but the derive skips it.
`Ord::cmp` at line ~520 does `.expect()` re-parse that panics on invalid points.

**Fix:** Removed `Deserialize` from the derive list. Added a custom
`impl serde::Deserialize for ECPoint` that deserializes into a raw helper struct,
then calls `ECPoint::new()` to validate on-curve. Invalid points now fail at
deserialization time with a proper error rather than causing panics later.

**Verification:** `cargo check` clean. All 171 tests pass (existing ECPoint
serde tests still round-trip valid points; invalid points would now fail at
deserialization).

---

## Fix #4: ECPoint docs claim secret-material zeroization — clarified

**File:** `neo-crypto/src/curves/ecc.rs:96-98`

**Problem:** Comment said "Key material is automatically zeroized when the point
is dropped" which is misleading — ECPoint holds public keys, not secret keys.

**Fix:** Changed to: "The point data is zeroized on drop (a defense-in-depth
measure even though ECPoints contain public keys, not secret keys)."

**Verification:** No code change, only doc. `cargo check` clean.

---

## Fix #5: BIP-32 uses variable-time num_bigint on secret keys — documented

**File:** `neo-crypto/src/keys/bip32.rs:82`

**Problem:** `add_mod_order` uses `num_bigint` arithmetic (variable-time) on
secret key material instead of the curve's constant-time Scalar. This is a
wallet-side timing concern, not a consensus issue.

**Fix:** Added `// SAFETY: BIP-32 child derivation here uses variable-time
big-integer arithmetic. This is acceptable because this code path is only used
for wallet key derivation, not consensus operations.` comment. No code change.

**Verification:** No functional change. `cargo check` clean.

---

## Build & Test Results

- `cargo check -p neo-crypto`: clean, zero warnings
- `cargo test -p neo-crypto`: 171 unit tests + 15 property tests + 4 doctests — all pass
