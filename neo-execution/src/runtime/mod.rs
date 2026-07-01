//! # neo-execution::runtime
//!
//! Runtime flags, execution context state, and VM-facing support types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `bls12381_interop`: BLS12-381 interop handlers.
//! - `diagnostic`: RPC diagnostic endpoints and health reporting helpers.
//! - `engine_provider`: execution engine provider trait.
//! - `env_flags`: execution environment flag records.
//! - `execution_context_state`: execution context state records.
//! - `helper`: shared helper functions.
//! - `interoperable`: VM interoperability trait helpers.
//! - `notify_event_args`: contract notification event records.

/// `InteropInterface` wrapper for BLS12-381 curve points (CryptoLib).
pub mod bls12381_interop;
pub mod diagnostic;
pub mod engine_provider;
/// Environment flag helpers used by execution diagnostics and optional profiling.
pub mod env_flags;
pub mod execution_context_state;
pub mod helper;
pub mod interoperable;
pub mod notify_event_args;
