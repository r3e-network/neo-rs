# neo-crypto

Cryptographic primitives tailored for Neo N3:

- `SecretKey` wrapper with zeroize-on-drop and constant-time equality.
- P-256 ECDSA signing/verification and keypair helpers (`from_private`, `generate`).
- AES-256-ECB helpers for NEP-2 style payloads.
- HMAC-SHA256 and scrypt key stretching utilities.

## Usage

```toml
[dependencies]
neo-crypto = { path = "../neo-crypto", default-features = false }

[features]
std = ["neo-crypto/std"]  # enables std-dependent error messages, default SHA
```

The crate is `no_std` (with `alloc`) by default so it can be used in enclaves
or embedded contexts.

## Testing

```bash
cargo test --manifest-path neo-crypto/Cargo.toml
```

The tests cover AES round-trips, ECC round-trips, and signature verification.
