use super::super::ClientRpcError;
use super::super::models::{RpcApplicationLog, RpcTransaction};
use super::RpcClient;
use super::helpers::{
    parse_i64_object_field, parse_uint256_object_field, token_as_object, token_as_string,
};
use crate::serialization;
use neo_io::Serializable;
use neo_payloads::Transaction;
use neo_payloads::block::Block;
use neo_primitives::UInt256;
use neo_serialization::json::JToken;

fn serialize_to_base64<T: Serializable>(value: &T) -> Result<String, ClientRpcError> {
    serialization::serializable_to_base64(value)
        .map_err(|err| ClientRpcError::new(-32603, format!("serialization failed: {err}")))
}

impl RpcClient {
    /// Retrieves a transaction by hash.
    pub async fn get_transaction(&self, hash: &str) -> Result<RpcTransaction, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getrawtransaction",
                vec![JToken::String(hash.to_string()), JToken::Boolean(true)],
            )
            .await?;
        let obj = token_as_object(result, "getrawtransaction")?;
        RpcTransaction::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Retrieves the application log for a block or transaction hash.
    /// Matches C# `GetApplicationLogAsync`
    pub async fn get_application_log(
        &self,
        hash: &str,
    ) -> Result<RpcApplicationLog, ClientRpcError> {
        let result = self
            .rpc_send_async("getapplicationlog", vec![JToken::String(hash.to_string())])
            .await?;
        let obj = token_as_object(result, "getapplicationlog")?;
        RpcApplicationLog::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Retrieves the application log for a block or transaction hash with trigger filtering.
    /// Matches C# `GetApplicationLogAsync` with trigger parameter
    pub async fn get_application_log_with_trigger(
        &self,
        hash: &str,
        trigger: &str,
    ) -> Result<RpcApplicationLog, ClientRpcError> {
        let params = vec![
            JToken::String(hash.to_string()),
            JToken::String(trigger.to_string()),
        ];
        let result = self.rpc_send_async("getapplicationlog", params).await?;
        let obj = token_as_object(result, "getapplicationlog")?;
        RpcApplicationLog::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Retrieves a transaction by hash as raw hex.
    /// Matches C# `GetRawTransactionHexAsync`
    pub async fn get_raw_transaction_hex(&self, hash: &str) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_async("getrawtransaction", vec![JToken::String(hash.to_string())])
            .await?;
        token_as_string(result, "getrawtransaction")
    }

    /// Calculates the network fee for a transaction.
    /// Matches C# `CalculateNetworkFeeAsync`
    pub async fn calculate_network_fee(&self, tx: &Transaction) -> Result<i64, ClientRpcError> {
        let base64 = serialize_to_base64(tx)?;
        let result = self
            .rpc_send_async("calculatenetworkfee", vec![JToken::String(base64)])
            .await?;
        parse_i64_object_field(
            result,
            "calculatenetworkfee",
            "networkfee",
            "Missing networkfee in calculatenetworkfee result",
            "networkfee",
            "Invalid networkfee token type",
        )
    }

    /// Broadcasts a raw transaction.
    /// Returns the transaction hash on success (C# parity).
    pub async fn send_raw_transaction(&self, tx: &Transaction) -> Result<UInt256, ClientRpcError> {
        let base64 = serialize_to_base64(tx)?;
        let result = self
            .rpc_send_async("sendrawtransaction", vec![JToken::String(base64)])
            .await?;
        parse_uint256_object_field(
            result,
            "sendrawtransaction",
            "hash",
            "Missing hash in sendrawtransaction",
            "Invalid tx hash",
        )
    }

    /// Broadcasts a block.
    /// Returns the block hash on success (C# parity).
    pub async fn submit_block(&self, block: &Block) -> Result<UInt256, ClientRpcError> {
        let base64 = serialize_to_base64(block)?;
        let result = self
            .rpc_send_async("submitblock", vec![JToken::String(base64)])
            .await?;
        parse_uint256_object_field(
            result,
            "submitblock",
            "hash",
            "Missing hash in submitblock",
            "Invalid block hash",
        )
    }
}
