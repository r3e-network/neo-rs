# Signer backends: extensible key management for neo-rs

Status: design + phased plan. Decision owner: maintainer. Date: 2026-06-15.

## Problem

The node already signs consensus messages with a software key or a PKCS#11 HSM
(`neo-hsm`). We want to support **more backends** — Ledger (and other interactive
hardware wallets), and any other PKCS#11 HSM — without duplicating crypto or
bolting unrelated concerns onto one crate.

## Decision summary

1. **Split signers by use case, not by "is it hardware".** There are two distinct
   signing contexts with different operational requirements:
   - **Consensus signing** (a dBFT validator): autonomous, high-frequency
     (multiple messages per block, sub-second), *no human in the loop*. Seam:
     [`neo_consensus::ConsensusSigner`].
   - **Wallet signing** (a user sending transactions): occasional, user-present,
     *explicit per-transaction confirmation is desirable*. Seam:
     [`neo_wallets::Wallet`] (`sign` / `sign_transaction`).
2. **Ledger / Trezor belong in the wallet layer, never in consensus.** They
   require a physical button press per signature. dBFT signs many messages per
   block automatically, so an interactive device cannot drive a validator. Putting
   Ledger behind `[consensus.hsm]` would be a category error.
3. **Other PKCS#11 HSMs already work today** via `neo-hsm`'s `GenericPkcs11`
   provider — they are *configuration*, not code. We only add ergonomic named
   profiles + docs.
4. **One verified crypto core.** Every backend reduces to the same primitive:
   *given the data to sign and a key identity, return a 64-byte secp256r1 `r‖s`*.
   The signature post-processing (DER→`r‖s`, low-s normalization) and the
   pubkey→`UInt160` script-hash identity live in **one** place (`neo-crypto`),
   reused by `neo-hsm`, `neo-ledger`, and the software keystore.

## Backend matrix

| Backend | Use case | Seam | Status / effort |
|---|---|---|---|
| Software key | consensus + wallet | both | done |
| PKCS#11 HSM (AWS/Azure/GCP) | consensus validator | `ConsensusSigner` | done (`neo-hsm`) |
| PKCS#11 HSM (YubiHSM2, nShield, Luna, SoftHSM2, Utimaco…) | consensus + wallet | `ConsensusSigner` | **config only** (generic provider) + named profiles for ergonomics |
| Nitro / TEE | consensus validator | `ConsensusSigner` | deferred (`neo-tee`) |
| **Ledger / Trezor** | **wallet (send txs)** | **`neo_wallets::Wallet`** | **new `neo-ledger` crate** |

## Shared core (Phase 1 — fully verifiable, do first)

Extract from `neo-hsm/src/pkcs11.rs` into `neo-crypto`:
- `secp256r1_canonical_signature(raw: &[u8], format: SigFormat) -> [u8; 64]`
  (DER decode for GCP-style, raw parse otherwise, then low-s normalization).
- the single-sig redeem-script → `UInt160` identity helper (already partly in
  `neo-vm`'s `RedeemScript`; expose a `script_hash_for_pubkey(&[u8;33])`).

Refactor `neo-hsm` to call the shared helper (no behaviour change; it currently
inlines this). This is the foundation every other backend reuses, and it is unit
testable with known test vectors — no hardware required.

## PKCS#11 ergonomics (Phase 1)

`neo-hsm` already covers any PKCS#11 device through `GenericPkcs11`. Add named
profiles for the common ones so operators don't hand-set `library_path`:
`YubiHsm2`, `NShield`, `SoftHsm2`, `Utimaco`. Each is a few lines in
`profile()` (default `.so` path + `SigFormat`). No new architecture.

```toml
[consensus.hsm]
provider  = "yubihsm2"     # was: provider="generic", library_path="…/libyubihsm_pkcs11.so"
key_label = "neo-validator"
```

## `neo-ledger` crate (Phase 2)

A feature-gated crate mirroring `neo-hsm`'s structure, implementing
`neo_wallets::Wallet` (NOT `ConsensusSigner`).

```
neo-ledger/
  src/lib.rs          // LedgerWallet: impl neo_wallets::Wallet
  src/transport.rs    // trait LedgerTransport { fn exchange(&self, apdu) -> Resp }
                      //   - HidTransport (feature "hid", ledger-transport-hid)
                      //   - MockTransport (tests / Speculos)
  src/apdu.rs         // Neo3 Ledger app command codec (GET_PUBLIC_KEY, SIGN)
  src/path.rs         // BIP-44 path encoding (Neo coin type 888')
```

Design rules:
- **Transport is a trait** so the device round-trip is mockable. Pure logic (APDU
  framing, path encoding, response parsing, sig post-processing via the shared
  `neo-crypto` helper) is **unit-tested against `MockTransport`** with no hardware.
- The real USB-HID transport sits behind a `hid` cargo feature.
- `sign()` sends the pre-image to the device; the device displays the tx and
  **waits for the user to confirm**; on success it returns a secp256r1 signature
  which we canonicalize with the shared helper.
- **Protocol caveat (honest):** the exact CLA/INS bytes + data layout must match
  the deployed Neo3 Ledger BOLOS app (`ledger-app-neo3`). They are isolated in
  `apdu.rs` so they are a one-file change, and must be validated against
  **Speculos** (Ledger's emulator) or a real device before production use. We do
  not claim a hardware-verified Ledger backend without that validation.

Wiring: `LedgerWallet` plugs into the wallet/transaction-signing path and the GUI
"Keys & Protection" screen alongside the software and HSM backends.

## Why not unify `ConsensusSigner` and `Wallet` into one trait now?

Tempting (both are "sign bytes → 64-byte sig"), but: `ConsensusSigner` is sync,
`Wallet::sign` is async; consensus is latency-critical and human-free, wallet is
interactive. They share the **crypto core** (Phase 1) but keep distinct seams.
Collapsing them would touch consensus signing for marginal benefit (YAGNI, and
do-not-disturb the consensus hot path). Revisit only if a third consumer appears.

## Phased plan

- **Phase 1 (verifiable, low risk):** shared `neo-crypto` canonicalization helper
  + refactor `neo-hsm` to use it + named PKCS#11 profiles + a key-management doc.
- **Phase 2 (architecture verifiable, protocol hardware-gated):** `neo-ledger`
  crate with mockable transport, unit-tested pure logic, `Wallet` impl, GUI wiring;
  APDU bytes flagged for Speculos validation.
- **Phase 3 (optional):** Trezor (same transport-trait pattern); WebAuthn/FIDO2
  passkey signer if browser/remote signing is ever wanted.
