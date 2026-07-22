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
//! - `env_flags`: execution environment flag records.
//! - `execution_context_state`: execution context state records.
//! - `helper`: shared helper functions.

/// `InteropInterface` wrapper for BLS12-381 curve points (CryptoLib).
pub mod bls12381_interop;
pub mod diagnostic;
/// Environment flag helpers used by execution diagnostics and optional profiling.
pub mod env_flags;
pub mod execution_context_state;
pub mod helper;
