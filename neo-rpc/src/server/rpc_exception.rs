pub use neo_core::rpc::RpcException;

use super::rpc_error::RpcError;

impl From<RpcError> for RpcException {
    fn from(error: RpcError) -> Self {
        RpcException::from_parts(
            error.code(),
            error.message().to_string(),
            error.data().map(|d| d.to_string()),
        )
    }
}

impl From<RpcException> for RpcError {
    fn from(err: RpcException) -> Self {
        RpcError::new(
            err.code(),
            err.message().to_string(),
            err.data().map(|d| d.to_string()),
        )
    }
}
