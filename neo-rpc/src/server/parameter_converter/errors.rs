//! Shared RPC parameter-conversion error constructors.

use super::super::rpc_error::RpcError;
use super::super::rpc_exception::RpcException;

pub(super) fn invalid_params<T: Into<String>>(message: T) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}
