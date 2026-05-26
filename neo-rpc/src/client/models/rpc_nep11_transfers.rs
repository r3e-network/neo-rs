// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep11_transfers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::utility::{
    optional_script_hash_or_address_lossy, parse_transfer_lists, required_bigint_string,
    required_script_hash_or_address, required_u16_number, required_u32_number, required_u64_number,
    required_uint256, transfer_lists_to_json,
};
use neo_config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JObject, JToken};
use neo_primitives::{UInt160, UInt256};
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
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
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
        json.insert(
            "timestamp".to_string(),
            JToken::Number(self.timestamp_ms as f64),
        );
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
        );

        if let Some(ref user_script_hash) = self.user_script_hash {
            json.insert(
                "transferaddress".to_string(),
                JToken::String(WalletHelper::to_address(
                    user_script_hash,
                    protocol_settings.address_version,
                )),
            );
        } else {
            json.insert("transferaddress".to_string(), JToken::Null);
        }

        json.insert(
            "amount".to_string(),
            JToken::String(self.amount.to_string()),
        );
        json.insert(
            "blockindex".to_string(),
            JToken::Number(f64::from(self.block_index)),
        );
        json.insert(
            "transfernotifyindex".to_string(),
            JToken::Number(f64::from(self.transfer_notify_index)),
        );
        json.insert(
            "txhash".to_string(),
            JToken::String(self.tx_hash.to_string()),
        );
        json
    }

    /// Creates from JSON.
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let token_id_str = json
            .get("tokenid")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'tokenid' field")?;
        let token_id = hex::decode(token_id_str.trim_start_matches("0x"))
            .map_err(|_| format!("Invalid tokenid: {token_id_str}"))?;

        let timestamp_ms = required_u64_number(json, "timestamp")?;
        let asset_hash =
            required_script_hash_or_address(json, "assethash", protocol_settings, "asset hash")?;
        let user_script_hash =
            optional_script_hash_or_address_lossy(json, "transferaddress", protocol_settings);
        let amount = required_bigint_string(json, "amount", "amount")?;
        let block_index = required_u32_number(json, "blockindex")?;
        let transfer_notify_index = required_u16_number(json, "transfernotifyindex")?;
        let tx_hash = required_uint256(json, "txhash")?;

        Ok(Self {
            token_id,
            timestamp_ms,
            asset_hash,
            user_script_hash,
            amount,
            block_index,
            transfer_notify_index,
            tx_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_roundtrip() {
        let settings = ProtocolSettings::default_settings();
        let entry = RpcNep11Transfer {
            token_id: vec![1, 2, 3],
            timestamp_ms: 1234,
            asset_hash: UInt160::zero(),
            user_script_hash: Some(UInt160::zero()),
            amount: BigInt::from(7),
            block_index: 9,
            transfer_notify_index: 1,
            tx_hash: UInt256::zero(),
        };
        let json = entry.to_json(&settings);
        let parsed = RpcNep11Transfer::from_json(&json, &settings).unwrap();
        assert_eq!(parsed.token_id, entry.token_id);
        assert_eq!(parsed.timestamp_ms, entry.timestamp_ms);
        assert_eq!(parsed.asset_hash, entry.asset_hash);
        assert_eq!(parsed.user_script_hash, entry.user_script_hash);
        assert_eq!(parsed.amount, entry.amount);
        assert_eq!(parsed.block_index, entry.block_index);
        assert_eq!(parsed.transfer_notify_index, entry.transfer_notify_index);
        assert_eq!(parsed.tx_hash, entry.tx_hash);
    }

    #[test]
    fn transfers_roundtrip() {
        let settings = ProtocolSettings::default_settings();
        let entry = RpcNep11Transfer {
            token_id: vec![0xaa],
            timestamp_ms: 1,
            asset_hash: UInt160::zero(),
            user_script_hash: None,
            amount: BigInt::from(11),
            block_index: 2,
            transfer_notify_index: 0,
            tx_hash: UInt256::zero(),
        };
        let transfers = RpcNep11Transfers {
            user_script_hash: UInt160::zero(),
            sent: vec![entry.clone()],
            received: vec![entry.clone()],
        };
        let json = transfers.to_json(&settings);
        let parsed = RpcNep11Transfers::from_json(&json, &settings).unwrap();
        assert_eq!(parsed.user_script_hash, transfers.user_script_hash);
        assert_eq!(parsed.sent.len(), 1);
        assert_eq!(parsed.received.len(), 1);
        assert_eq!(parsed.sent[0].token_id, entry.token_id);
        assert_eq!(parsed.received[0].user_script_hash, entry.user_script_hash);
    }
}
