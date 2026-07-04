# neo-rs Audit Refactor — Network ChannelFull Semantics

## What was done

Completed a low-risk incremental refactor from the existing service/app-layer audit: `neo-network` now distinguishes local command-channel backpressure from actual service shutdown in `NetworkHandle::try_broadcast_transaction`.

## Key changes

- Added `NetworkError::ChannelFull` with message `local node command channel is full`.
- Mapped `NetworkError::ChannelFull` to `neo_runtime::ServiceError::ServiceUnavailable(...)`.
- Updated `try_broadcast_transaction` so:
  - `TrySendError::Full(_)` returns `NetworkError::ChannelFull`.
  - `TrySendError::Closed(_)` returns `NetworkError::LocalShuttingDown`.
- Added a focused test covering both full-queue and closed-channel paths.

## Files changed

- `neo-network/src/errors/error.rs`
- `neo-network/src/service/handle.rs`
- `neo-network/src/tests/service/handle.rs`

## Verification

QA independently verified:

- `cargo test --manifest-path "/Users/jinghuiliao/git/neo-rs/Cargo.toml" -p neo-network try_broadcast_transaction` — passed.
- `cargo check --manifest-path "/Users/jinghuiliao/git/neo-rs/Cargo.toml" -p neo-network --tests` — passed.
- `cargo test --manifest-path "/Users/jinghuiliao/git/neo-rs/Cargo.toml" -p neo-network` — passed, 107 tests passed, 0 failed.

## Follow-up notes

- No source bug or test bug remained after QA; routing decision: `NoOne`.
- Deliberately not included in this round: blockchain explicit shutdown path, live `NeoValidateStage` wiring, RPC split, or broader architecture changes.
- Earlier teammate failures were network/turn-limit related; the final implementation and QA outputs were independently verified before delivery.
