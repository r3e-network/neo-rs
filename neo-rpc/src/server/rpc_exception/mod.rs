//! # neo-rpc::server::rpc_exception
//!
//! Exception-style RPC error wrappers used by handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_exception`: RPC exception wrapper and conversion helpers.

pub use neo_primitives::RpcException;

use super::rpc_error::RpcError;

impl From<RpcError> for RpcException {
    fn from(error: RpcError) -> Self {
        Self::from_parts(
            error.code(),
            error.message().to_string(),
            error.data().map(std::string::ToString::to_string),
        )
    }
}

impl From<RpcException> for RpcError {
    fn from(err: RpcException) -> Self {
        Self::new(
            err.code(),
            err.message().to_string(),
            err.data().map(std::string::ToString::to_string),
        )
    }
}
