pub use neo_core::rpc::RpcException;

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
