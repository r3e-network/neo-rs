use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_block::VerifyResult;
use neo_blockchain::RelayResult;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::oneshot;

pub(super) fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
    match result.result {
        // C# GetRelayResult attaches WithData(reason.ToString()) to EVERY non-success
        // case, so both the error message suffix and the `data` field carry the
        // VerifyResult name. Mirror that for sendrawtransaction/submitblock parity.
        VerifyResult::Succeed => Ok(json!({"hash": result.hash.to_string()})),
        VerifyResult::AlreadyExists => Err(RpcException::from(
            RpcError::already_exists().with_data("AlreadyExists"),
        )),
        VerifyResult::AlreadyInPool => Err(RpcException::from(
            RpcError::already_in_pool().with_data("AlreadyInPool"),
        )),
        VerifyResult::OutOfMemory => Err(RpcException::from(
            RpcError::mempool_cap_reached().with_data("OutOfMemory"),
        )),
        VerifyResult::InvalidScript => Err(RpcException::from(
            RpcError::invalid_script().with_data("InvalidScript"),
        )),
        VerifyResult::InvalidAttribute => Err(RpcException::from(
            RpcError::invalid_attribute().with_data("InvalidAttribute"),
        )),
        VerifyResult::InvalidSignature => Err(RpcException::from(
            RpcError::invalid_signature().with_data("InvalidSignature"),
        )),
        VerifyResult::OverSize => Err(RpcException::from(
            RpcError::invalid_size().with_data("OverSize"),
        )),
        VerifyResult::Expired => Err(RpcException::from(
            RpcError::expired_transaction().with_data("Expired"),
        )),
        VerifyResult::InsufficientFunds => Err(RpcException::from(
            RpcError::insufficient_funds().with_data("InsufficientFunds"),
        )),
        VerifyResult::PolicyFail => Err(RpcException::from(
            RpcError::policy_failed().with_data("PolicyFail"),
        )),
        VerifyResult::UnableToVerify => Err(RpcException::from(
            RpcError::verification_failed().with_data("UnableToVerify"),
        )),
        VerifyResult::Invalid => Err(RpcException::from(
            RpcError::verification_failed().with_data("Invalid"),
        )),
        VerifyResult::HasConflicts => Err(RpcException::from(
            RpcError::verification_failed().with_data("HasConflicts"),
        )),
        VerifyResult::Unknown => Err(RpcException::from(
            RpcError::verification_failed().with_data("Unknown"),
        ))}
}

/// Replacement for the legacy `with_relay_responder` actor pattern.
///
/// The actor framework only existed to ferry a single `RelayResult` back to the
/// caller; in the reth-style service world that is a `tokio::sync::oneshot`
/// channel between the RPC handler and the (now async) relay service.
pub(super) async fn with_relay_responder<F, Fut>(
    _server: &RpcServer,
    send: F,
) -> Result<RelayResult, RpcException>
where
    F: FnOnce(oneshot::Sender<RelayResult>) -> Fut,
    Fut: std::future::Future<Output = Result<(), RpcException>>,
{
    let (responder, rx) = oneshot::channel();
    if let Err(err) = send(responder).await {
        return Err(err);
   }
    rx.await
        .map_err(|_| internal_error("relay result channel closed"))
}
