//! # neo-rpc::server::rpc_error_factory
//!
//! Helpers for constructing canonical RPC errors.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_error_factory`: canonical JSON-RPC error constructors.

// Rust port of Neo.Plugins.RpcServer.RpcErrorFactory providing helper
// constructors for specialised `RpcError` instances.

use super::rpc_error::RpcError;

pub fn invalid_contract_verification(data: impl Into<String>) -> RpcError {
    RpcError::invalid_contract_verification().with_data(data)
}
