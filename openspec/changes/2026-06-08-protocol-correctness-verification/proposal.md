# Protocol Correctness Verification (post reth-style refactor)

## Why

The 2026-06-08 reth-style service architecture refactor moved every
long-running component (blockchain, network, runtime, consensus, mempool,
state-service) off the Akka-style `neo-actors` framework and onto
`async` + `tokio` + `mpsc` + `broadcast` services. It also deleted the
monolithic `neo-core` crate and split it into focused crates
(`neo-error`, `neo-time`, `neo-ledger-types`, `neo-chain`,
`neo-payloads`, `neo-block`, `neo-blockchain`, `neo-system`, ...).

The refactor intentionally **postponed** several protocol surfaces
(`NetworkMessage` envelope, `RemoteNode` handshake state-machine,
`TaskManager` completion flow, mempool, state-service, the
`neo-rpc --features server` build) until later stages of the
kill-neo-core migration. The CI test surface, the `cargo test
--workspace --lib` results, and the `neo-rpc --features server`
compile state need to be re-baselined so we know exactly what is
verified and what is still a stub.

This change documents the actual state — what the workspace currently
proves about Neo N3 v3.9.2 protocol correctness, what the gaps are, and
which follow-up OpenSpec changes are needed to close them.

## What Changes

- Run the full test surface (`cargo test --workspace --lib`,
  `cargo test -p neo-tests`, `cargo test -p neo-network --tests`,
  `cargo test -p neo-rpc`, `cargo test -p neo-consensus --lib`) and
  capture the per-binary pass/fail/ignored counts.
- Confirm the legacy `neo-core` actor modules
  (`neo-core/src/network/p2p/`) are gone and the reth-style
  `neo-network` service is the canonical home.
- Re-verify the wire-format enums (`MessageCommand`, `MessageFlags`,
  `InventoryType`, `VerifyResult`) round-trip byte-for-byte.
- Re-verify the existing C#-reference vectors:
  - MPT genesis state root (`neo-crypto` `test_genesis_state_root_matches_reference`),
  - BLS12-381 compatibility vector (`bls12381_compatibility_vector`),
  - secp256k1 high-s signature acceptance (`secp256k1_verify_accepts_high_s_like_csharp`),
  - call-flags bit-values (`call_flags_bit_values_match_csharp`).
- Re-verify the dBFT message state machine end-to-end
  (`neo-consensus`, 101 lib tests, 1 ignored).
- Document, **without fixing**, the seven currently-failing
  unit tests and the broken `neo-rpc --features server` build
  so the OpenSpec backlog can be re-prioritised.

## Impact

**Codebase**: No code changes. This is a measurement + reporting
change. The verification report at
`openspec/changes/2026-06-08-protocol-correctness-verification/verification-report.md`
is the only durable artifact.

**APIs**: No API changes.

**Dependencies**: No dependency changes.

**Testing**: No new tests; this change just measures the existing
ones and records the results.

**Documentation**: Adds the verification report. Updates the
`protocol-compliance-audit` spec with the new "as of 2026-06-08"
status summary.

## Capabilities

### New Capabilities

- `protocol-correctness-verification`: a snapshot of which Neo N3
  v3.9.2 protocol surfaces are byte-for-byte compatible with C#,
  which are partial, and which are still stubs. Re-runnable: the
  `tasks.md` is the script.

### Modified Capabilities

- `protocol-compliance-audit`: the existing
  `openspec/specs/protocol-compliance-audit/spec.md` requirements
  are updated to reflect the 2026-06-08 reth-style layout. The
  byte-for-byte compatibility requirement stays the same; the
  implementation coverage notes are refreshed.

## Non-goals

- This change **does not fix** any of the failing tests or the
  `neo-rpc --features server` build. Those are tracked as separate
  follow-up OpenSpec changes (see the "Recommendations" section of
  the verification report).
- This change **does not add** a real mainnet-block reproducer
  harness. The single `tests/fixtures/mainnet_block_1000.hex`
  fixture is all-zero and is not consumed by any test; that gap is
  recorded but not closed here.
- This change **does not** wire `neo-state-service` or `neo-mempool`
  back in. Both crates are still WIP placeholders per
  `neo-state-service/src/lib.rs` and `neo-mempool/src/lib.rs`.
