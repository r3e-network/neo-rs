//! Typed request parsing for node RPC relay handlers.
//!
//! Node relay methods accept Base64-encoded wire payloads. Decoding and
//! deserializing those payloads here keeps the handler focused on relay
//! submission and relay-result mapping.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_base64_param_with_decode_message, invalid_params};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::{block::Block, transaction::Transaction};
use serde_json::Value;

pub(super) struct RawTransactionRequest {
    pub(super) transaction: Transaction,
}

impl RawTransactionRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "sendrawtransaction",
            "Invalid transaction payload",
        )?;
        let transaction = Transaction::from_bytes(&raw)
            .map_err(|err| invalid_params(format!("Invalid transaction: {err}")))?;
        Ok(Self { transaction })
    }
}

pub(super) struct SubmitBlockRequest {
    pub(super) block: Block,
}

impl SubmitBlockRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "submitblock",
            "Invalid block payload",
        )?;
        let mut reader = MemoryReader::new(&raw);
        let block = <Block as Serializable>::deserialize(&mut reader)
            .map_err(|err| invalid_params(format!("Invalid block: {err}")))?;
        Ok(Self { block })
    }
}
