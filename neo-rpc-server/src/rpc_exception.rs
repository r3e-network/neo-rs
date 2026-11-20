//! Conversions between the shared [`RpcException`] envelope and the RPC
//! server's [`RpcError`].
//!
//! Both types now store the same `code`/`message`/`data` envelope, so the
//! conversions are zero-cost moves rather than field-by-field copies.

pub use neo_primitives::RpcException;

use super::rpc_error::RpcError;

impl From<RpcError> for RpcException {
    fn from(error: RpcError) -> Self {
        error.into_envelope()
    }
}

impl From<RpcException> for RpcError {
    fn from(err: RpcException) -> Self {
        Self::from_envelope(err)
    }
}
