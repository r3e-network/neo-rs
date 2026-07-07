//! Relay-result projection for node and wallet RPC submission paths.
//!
//! C# `GetRelayResult` attaches the `VerifyResult` name as error data for every
//! non-success relay outcome. Keeping the mapping here keeps the relay facade
//! focused on service submission while preserving RPC response parity.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use neo_blockchain::RelayResult;
use neo_payloads::VerifyResult;
use serde_json::{Value, json};

pub(in crate::server) fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
    match result.result {
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
        // C# `RpcServer.Node` maps NotYetValid to `RpcError.ExpiredTransaction`
        // (NOT the default VerificationFailed). Both Expired and NotYetValid were
        // reported as ExpiredTransaction before the split, and the code is kept
        // stable for clients (RpcServer.Node.cs:102-106).
        VerifyResult::NotYetValid => Err(RpcException::from(
            RpcError::expired_transaction().with_data("NotYetValid"),
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
        )),
    }
}
