//! Shared RPC error constructors.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;

/// Creates an RpcException for invalid parameters.
#[inline]
pub fn invalid_params(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}

/// Creates an RpcException for internal server errors.
#[inline]
pub fn internal_error(message: impl ToString) -> RpcException {
    RpcException::from(RpcError::internal_server_error().with_data(message.to_string()))
}
