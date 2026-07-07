//! Runtime bridge for synchronous RPC relay handlers.
//!
//! Relay handlers are synchronous while blockchain service APIs are async. This
//! module owns that boundary so relay orchestration can stay focused on Neo
//! transaction and block submission semantics.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use tokio::runtime::{Handle, Runtime};
use tokio::task::block_in_place;

/// Drives an async service round-trip to completion from a synchronous
/// RPC handler. Uses the ambient multi-thread runtime when one exists
/// (the jsonrpsee server path), and a throwaway runtime otherwise (direct
/// handler invocation in tests).
pub(super) fn block_on_service<F, T>(future: F) -> Result<T, RpcException>
where
    F: std::future::Future<Output = T>,
{
    if let Ok(handle) = Handle::try_current() {
        Ok(block_in_place(|| handle.block_on(future)))
    } else {
        let runtime = Runtime::new().map_err(|err| internal_error(err.to_string()))?;
        Ok(runtime.block_on(future))
    }
}
