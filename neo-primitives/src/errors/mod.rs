//! # neo-primitives::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.
//! - `network_error`: network error records.
//! - `rpc_exception`: Exception-style RPC error wrappers used by handlers.

pub mod error;
pub mod network_error;
/// JSON-RPC exception codes and helpers shared by RPC-facing crates.
pub mod rpc_exception;
