# Neo N3 v3.10.2 protocol delta

## Overview

neo-rs targets C# [Neo v3.10.2](https://github.com/neo-project/neo/releases/tag/v3.10.2)
(`9ca0203`, 2026-10-23). That release is a security patch on top of v3.10.1.
It does not add a hardfork, a native method, an opcode, or a P2P message.

NeoVM stays at `neo-project/neo-vm` tag `v3.10.1`. Parity scripts that read
`neo-project/neo` use tag `v3.10.2`. The opcode script keeps the VM tag.

## What changed

`Helper.IsMultiSigContract` had three public overloads. The overload that only
returned a `bool`, and the overload that returned `(m, n)` without the points,
skipped elliptic-curve decoding: `points?.Add(ECPoint.DecodePoint(...))` never
calls `DecodePoint` when `points` is null. A verification script with the
multisig shape and a public key that is not on secp256r1 was therefore
classified as a standard multisig contract by one path and rejected by another.

v3.10.2 decodes every 33-byte key before the script can be a multisig contract.
A failure to decode returns false. It does not throw. `IsSignatureContract` is
unchanged and still does not decode its key.

There is no wire-format change. Blocks, transactions, and P2P messages from
v3.10.1 stay valid. A malformed multisig witness is not a standard witness:
state-independent verification leaves it for the VM, and state-dependent
verification rejects it.

## API

`neo_core::smart_contract::helper::Helper` has one parser.
`is_multi_sig_contract` is `parse_multi_sig_contract(script).is_some()`.
The parser decodes each key with `ECPoint::from_bytes_with_curve(Secp256r1, key)`.
There is no structural-only overload.

## Usage

Call `Helper::is_multi_sig_contract` or `Helper::parse_multi_sig_contract`.
Both reject a script whose 33-byte pushes do not decode on secp256r1, including
prefix `0x00` and prefix `0x04`. The regression tests in
`neo-core/tests/smart_contract_helper_tests.rs` build those scripts with
`ScriptBuilder`.

## Design

One function is the classification used by the standard-witness fast path in
`Transaction::verify_state_independent` and by the fee shortcut in
`verify_state_dependent`. If that function accepted a key the VM later rejects,
the fast path and the VM would disagree about the same witness. Decoding inside
the only parser is what keeps them on the v3.10.2 rule.

## Tests

| C# test | Rust test |
|---|---|
| `TestIsMultiSigContract_InvalidPublicKey` (prefix 0 and 4) | `test_is_multi_sig_contract_rejects_undecodable_public_key` |
| `TestIsMultiSigContract_WrongCurve` | `test_is_multi_sig_contract_rejects_secp256k1_only_point` |
| `TestIsSignatureContract_WrongCurve` | `test_is_signature_contract_does_not_decode_public_key` |
| `Test_Verify_MalformedMultiSigWitness` | `test_malformed_multisig_witness_is_not_standard` |

`cargo test -p neo-core --test smart_contract_helper_tests` covers the four.
