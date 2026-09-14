# Consensus signing key buffer hardening

## Overview

Local ECDSA signing briefly materializes a 32-byte private key for
`Secp256r1Crypto::sign`. That buffer must be `Zeroizing` so residual key bytes
are wiped when the stack frame ends (audit A22).

## API Reference

`ConsensusService::sign` (internal) copies `self.private_key` into
`Zeroizing<[u8; 32]>` before calling `Secp256r1Crypto::sign`.

## Design

`Zeroizing` wraps the fixed array so `Drop` zeroes memory even on early returns
after a successful copy. The underlying `private_key` field remains
`Zeroizing<Vec<u8>>`.

## Usage Examples

No public API change; consensus payload signing paths call `sign` unchanged.

## Test Coverage

Covered indirectly by consensus recovery/signing unit tests that exercise
`create_payload` / Commit signing.
