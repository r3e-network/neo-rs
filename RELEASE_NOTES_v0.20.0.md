# Neo-RS v0.20.0 Release Notes

**Release date:** 2026-10-29
**Protocol compatibility:** Neo N3 v3.10.2 (NeoVM v3.10.1)
**MSRV:** Rust 1.99
**License:** MIT

v0.20.0 publishes the work on `main` since v0.19.1 and moves the C# protocol
target from v3.10.1 to v3.10.2.

## Protocol

C# Neo v3.10.2 (23 October 2026) is a security patch. It does not add a
hardfork, an opcode, or a new wire format. Every `IsMultiSigContract` path now
decodes each public key on secp256r1 before the script counts as a standard
multisig contract. A key that does not decode is not a standard witness:
state-independent verification leaves it to the VM, and state-dependent
verification rejects it.

`Helper::parse_multi_sig_contract` is the only classification path in neo-rs,
and it already decoded keys. This release locks that rule to the C# tests:
undecodable prefixes `0x00` and `0x04`, a secp256k1-only point, and a
transaction whose witness is a multisig-shaped script with an all-zero key.
Details are in `docs/protocol/NEO_3.10.2.md`.

The unsigned block header still has one writer, and its byte length is
`UnsignedHeader::WIRE_SIZE`.

## Toolchain

Local builds, CI, and the Docker image use Rust 1.99.1. MSRV equals that pin.
Workspace lints match the CI warning gate.

## Also included from main since v0.19.1

- Application-log comparison that fails closed, with a per-height journal.
- Local state roots stop after a validator-signed root diverges.
- Pruned MPT node cache no longer corrupts a trie that shares the global cache.
- Replay and bulk state-root jobs take explicit database paths.

## Verification for this release commit

```text
smart_contract_helper_tests (v3.10.2 cases): 4 passed
header_wire_parity:                            2 passed
neo-vm arena_pool:                             5 passed, 3 ignored
```

## Known limitations

This remains a testnet and private-network release, not a mainnet-certified
node. A single uninterrupted genesis-to-tip state-root comparison is still
open. See `RELEASE_NOTES_v0.19.1.md` and `MAINNET-STATUS.md`.
