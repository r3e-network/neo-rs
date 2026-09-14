---
kind: error_handling
name: Per-Crate Enumerated Error Types with thiserror, Result Aliases, and VM Fault-State Propagation
category: error_handling
scope:
    - '**'
source_files:
    - neo-core/src/error.rs
    - neo-core/src/unhandled_exception_policy.rs
    - neo-core/src/rpc/exception.rs
    - neo-vm/src/execution_engine/exception.rs
    - neo-consensus/src/error.rs
    - neo-p2p/src/error.rs
    - neo-storage/src/error.rs
    - neo-rpc/src/error.rs
---

## Overview

The neo-rs monorepo uses a consistent, per-crate enumerated error model built on `thiserror`. Each major crate (`neo-core`, `neo-consensus`, `neo-p2p`, `neo-storage`, `neo-rpc`, `neo-hsm`, `neo-tee`, `neo-io`, `neo-json`) defines its own domain-specific `Error` enum in a dedicated `src/error.rs` file, exposes typed result aliases (`CoreResult`, `ConsensusResult`, `P2PResult`, `StorageResult`, `RpcResult`, …), and implements `From` conversions from lower-level errors into the local error type. Errors are propagated via `Result<T, E>` — panics are reserved for unrecoverable internal state (e.g. missing invocation context) rather than protocol or user errors.

## Key Files and Packages

- `neo-core/src/error.rs` — `CoreError` enum with ~20 variants covering validation, I/O, serialization, cryptography, gas limits, buffer overflow, timeout, not-found/already-exists, execution, compression; provides `is_retryable()`, `is_user_error()`, `category()` helpers and a `ToNativeError` extension trait that maps any `Result<T, E: ToString>` into `CoreResult<T>` via `native_contract`.
- `neo-core/src/unhandled_exception_policy.rs` — thin re-export of `UnhandledExceptionPolicy` from `neo-primitives`, centralizing policy for uncaught VM exceptions.
- `neo-core/src/rpc/exception.rs` — re-exports `RpcException` from `neo-primitives` for RPC-layer exception types.
- `neo-vm/src/execution_engine/exception.rs` — VM try/catch/finally/throw implementation; an unhandled exception sets `VMState::FAULT` and returns `VmError::UnhandledException(StackItem)` rather than panicking.
- `neo-consensus/src/error.rs` — `ConsensusError` covering view mismatches, invalid proposals, signature verification failures, hash mismatches, timeouts, wrong block/view, duplicate validators, channel/send/persistence errors, bincode errors, insufficient signatures.
- `neo-p2p/src/error.rs` — `P2PError` covering connection failures, peer disconnects, invalid messages, protocol violations (with peer address), timeouts, IO/network errors; includes `is_timeout()` helper.
- `neo-storage/src/error.rs` — `StorageError` covering key-not-found, read-only, serialization, backend, invalid operation, commit-failed, generic IO; derives `Clone, PartialEq, Eq` for testability.
- `neo-rpc/src/error.rs` — `RpcError` covering request failures, invalid responses, method-not-found, invalid params, internal errors, plus `#[from]` conversions for `serde_json::Error`, `std::io::Error`, `FromUtf8Error`, `reqwest::Error`, and `neo_core::CoreError`.
- `neo-hsm/src/error.rs`, `neo-tee/src/error.rs`, `neo-io/src/lib.rs`, `neo-json/src/error.rs` — similarly structured per-crate error enums.

## Architecture and Conventions

1. **Per-crate error enums with `thiserror`**: Every crate defines a single `Error` enum annotated with `#[derive(Error, Debug)]` (and often `Clone, PartialEq, Eq`). Variants carry structured fields (e.g. `HashMismatch { expected, got }`, `InsufficientGas { required, available }`) so callers can inspect specifics without string parsing.

2. **Typed result aliases**: Each crate exports a `Result<T, E = <CrateError>>` alias and a named `XxxResult<T>` alias (`CoreResult`, `ConsensusResult`, `P2PResult`, `StorageResult`, `RpcResult`, …). This is the standard return type throughout the codebase.

3. **Structured constructors**: Each error enum exposes `fn xxx<S: Into<String>>(...) -> Self` constructors (e.g. `CoreError::invalid_format`, `P2PError::protocol_violation`, `StorageError::key_not_found`) used by callers instead of constructing variants directly.

4. **Cross-crate error propagation via `From`**: Lower-level errors are converted into the owning crate's error type using `impl From<LowerError> for UpperError`. Examples include `From<std::io::Error>` → `Io` variants, `From<PrimitiveError>` → `CoreError`, `From<neo_storage::StorageError>` → `CoreError::InvalidOperation`, `From<crate::neo_vm::VmError>` → `CoreError::InvalidOperation`, `From<bincode::Error>` → `ConsensusError::BincodeError`, and `From<reqwest::Error>` → `RpcError::Http`. A macro `impl_error_from!` in `neo-core` batches standard-library conversions.

5. **Retryability / user-vs-system classification**: `CoreError` exposes `is_retryable()` (IO, Network, Timeout, System) and `is_user_error()` / `is_system_error()` (Invalid, InvalidOperation, ValidationFailed, TypeConversion, InsufficientGas, Base58Decode, Cryptographic). This lets callers implement retry/backoff policies uniformly.

6. **Category tagging for logging/metrics**: `CoreError::category()` maps each variant to a stable string category (`validation`, `io`, `serialization`, `operation`, `system`, `resource`, `cryptography`, `buffer`, `configuration`, `timeout`, `conversion`, `execution`, `compression`) used for structured logging and metrics aggregation.

7. **VM exception model is distinct from Rust errors**: The Neo VM uses a stack-based exception model implemented in `neo-vm/src/execution_engine/exception.rs`. `execute_try` pushes a try context onto a per-context try-stack; `execute_throw` walks the invocation stack looking for matching catch/finally handlers; if no handler is found, it sets `VMState::FAULT` and returns `VmError::UnhandledException(exception_stack_item)`. This keeps smart-contract exceptions within the VM boundary and does not panic the host process.

8. **RPC exception re-export**: `neo-core/src/rpc/exception.rs` re-exports `RpcException` from `neo-primitives`, keeping the RPC layer agnostic of the exact exception payload while still exposing a typed error surface.

9. **Fallback stringly-typed variants**: Some crates keep a `Other(String)` or `ChannelError(String)` variant as a catch-all for errors that cannot be fully modeled (e.g. arbitrary channel send errors, unknown P2P issues). These are used sparingly alongside well-typed variants.

## Conventions and Constraints Observed

- **No panics for protocol/user errors**: Panics appear only when internal invariants are violated (e.g. "No current context" when the invocation stack is empty during exception handling). Protocol violations, bad input, network failures, and storage errors all return `Err(...)`.
- **Errors are Clone + PartialEq where useful**: `CoreError`, `P2PError`, `StorageError` derive `Clone`; `StorageError` additionally derives `PartialEq, Eq` so tests can assert equality of error values.
- **Errors are Display + Debug via `thiserror`**: All error enums use `#[error("...")]` attributes so `to_string()` produces human-readable messages suitable for logs and RPC responses.
- **Domain-specific helpers**: Each error type adds small query methods (`is_retryable`, `is_user_error`, `is_system_error`, `is_timeout`) rather than relying on string matching.
- **Layered conversion up the stack**: Lower layers (`neo-io`, `neo-primitives`, `neo-storage`, `neo-vm`) expose their own errors; higher layers (`neo-core`) convert them into `CoreError`, and the RPC layer converts `CoreError` into `RpcError` via `#[from]`.
- **VM faults do not propagate as Rust panics**: An unhandled VM exception becomes a `VmError::UnhandledException` returned from the execution engine, which the caller must handle explicitly — the VM never unwinds the Rust call stack via `panic!`.
- **Uncaught exception policy is centralized**: `unhandled_exception_policy.rs` re-exports `UnhandledExceptionPolicy` from `neo-primitives`, indicating that the policy for what happens when a contract throws without a catch is configurable at the primitives layer and consumed by both core and VM code.