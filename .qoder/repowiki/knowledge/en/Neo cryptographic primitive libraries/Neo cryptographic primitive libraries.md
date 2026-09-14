---
kind: external_dependency
name: Neo cryptographic primitive libraries
slug: neo-crypto-libraries
category: external_dependency
category_hints:
    - vendor_identity
scope:
    - '**'
---

### Crypto libraries
- Role: Implement Neo N3's elliptic curve signatures (secp256k1 for NEO/GAS accounts, ed25519-dalek for committee/BFT messages, k256/p256 for ECDSA variants) and BLS12-381 aggregation used by consensus/signatures.
- Durable usage model: Pinned versions in the workspace; `secp256k1` uses `recovery` and `global-context` features required by Neo's signature recovery path. These are leaf dependencies consumed by `neo-crypto` and surfaced to higher layers via the crypto module.