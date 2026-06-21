use super::super::utility::{
    NepTransferFieldRefs, insert_nep_transfer_fields, parse_nep_transfer_fields,
    parse_transfer_lists, transfer_lists_to_json,
};
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::JObject;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP17 transfers for an address matching C# `RpcNep17Transfers`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfers {
    /// User script hash
    pub user_script_hash: UInt160,

    /// List of sent transfers
    pub sent: Vec<RpcNep17Transfer>,

    /// List of received transfers
    pub received: Vec<RpcNep17Transfer>,
}

impl RpcNep17Transfers {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        transfer_lists_to_json(
            &self.sent,
            &self.received,
            &self.user_script_hash,
            protocol_settings,
            RpcNep17Transfer::to_json,
        )
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let (sent, received, user_script_hash) =
            parse_transfer_lists(json, protocol_settings, RpcNep17Transfer::from_json)?;

        Ok(Self {
            user_script_hash,
            sent,
            received,
        })
    }
}

/// Individual NEP17 transfer entry matching C# `RpcNep17Transfer`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfer {
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,

    /// Asset hash
    pub asset_hash: UInt160,

    /// Transfer address script hash
    pub user_script_hash: Option<UInt160>,

    /// Transfer amount
    pub amount: BigInt,

    /// Block index
    pub block_index: u32,

    /// Transfer notify index
    pub transfer_notify_index: u16,

    /// Transaction hash
    pub tx_hash: UInt256,
}

impl RpcNep17Transfer {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
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

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let fields = parse_nep_transfer_fields(json, protocol_settings)?;

        Ok(Self {
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
#[path = "../../tests/client/models/rpc_nep17_transfers.rs"]
mod tests;
