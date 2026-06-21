use super::super::utility::{
    NepTransferFieldRefs, insert_nep_transfer_fields, parse_nep_transfer_fields,
    parse_transfer_lists, transfer_lists_to_json,
};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256, strip_hex_prefix};
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP11 transfers for an address matching C# `RpcNep11Transfers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11Transfers {
    /// User script hash.
    pub user_script_hash: UInt160,
    /// Sent transfers.
    pub sent: Vec<RpcNep11Transfer>,
    /// Received transfers.
    pub received: Vec<RpcNep11Transfer>,
}

impl RpcNep11Transfers {
    /// Converts to JSON.
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        transfer_lists_to_json(
            &self.sent,
            &self.received,
            &self.user_script_hash,
            protocol_settings,
            RpcNep11Transfer::to_json,
        )
    }

    /// Creates from JSON.
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let (sent, received, user_script_hash) =
            parse_transfer_lists(json, protocol_settings, RpcNep11Transfer::from_json)?;

        Ok(Self {
            user_script_hash,
            sent,
            received,
        })
    }
}

/// Individual NEP11 transfer entry matching C# `RpcNep11Transfer`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11Transfer {
    /// Token id as raw bytes.
    pub token_id: Vec<u8>,
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Asset hash.
    pub asset_hash: UInt160,
    /// Transfer address script hash.
    pub user_script_hash: Option<UInt160>,
    /// Transfer amount.
    pub amount: BigInt,
    /// Block index.
    pub block_index: u32,
    /// Transfer notify index.
    pub transfer_notify_index: u16,
    /// Transaction hash.
    pub tx_hash: UInt256,
}

impl RpcNep11Transfer {
    /// Converts to JSON.
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "tokenid".to_string(),
            JToken::String(hex::encode(&self.token_id)),
        );
        insert_nep_transfer_fields(
            &mut json,
            NepTransferFieldRefs {
                timestamp_ms: self.timestamp_ms,
                asset_hash: self.asset_hash,
                user_script_hash: self.user_script_hash,
                amount: &self.amount,
                block_index: self.block_index,
                transfer_notify_index: self.transfer_notify_index,
                tx_hash: self.tx_hash,
            },
            protocol_settings,
        );
        json
    }

    /// Creates from JSON.
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let token_id_str = json
            .get("tokenid")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'tokenid' field"))?;
        let token_id = hex::decode(strip_hex_prefix(&token_id_str))
            .map_err(|_| CoreError::other(format!("Invalid tokenid: {token_id_str}")))?;

        let fields = parse_nep_transfer_fields(json, protocol_settings)?;

        Ok(Self {
            token_id,
            timestamp_ms: fields.timestamp_ms,
            asset_hash: fields.asset_hash,
            user_script_hash: fields.user_script_hash,
            amount: fields.amount,
            block_index: fields.block_index,
            transfer_notify_index: fields.transfer_notify_index,
            tx_hash: fields.tx_hash,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_nep11_transfers.rs"]
mod tests;
