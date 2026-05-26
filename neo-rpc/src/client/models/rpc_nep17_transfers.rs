// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep17_transfers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::utility::{
    object_array, optional_script_hash_or_address_lossy, parse_object_array_lossy,
    required_address_script_hash, required_bigint_string, required_script_hash_or_address,
    required_u16_number, required_u32_number, required_u64_number, required_uint256,
};
use neo_core::config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JObject, JToken};
use neo_primitives::{UInt160, UInt256};
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
        let mut json = JObject::new();

        json.insert(
            "sent".to_string(),
            object_array(&self.sent, |transfer| transfer.to_json(protocol_settings)),
        );
        json.insert(
            "received".to_string(),
            object_array(&self.received, |transfer| {
                transfer.to_json(protocol_settings)
            }),
        );

        json.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &self.user_script_hash,
                protocol_settings.address_version,
            )),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let sent = parse_object_array_lossy(json, "sent", |obj| {
            RpcNep17Transfer::from_json(obj, protocol_settings)
        });

        let received = parse_object_array_lossy(json, "received", |obj| {
            RpcNep17Transfer::from_json(obj, protocol_settings)
        });

        let user_script_hash = required_address_script_hash(json, "address", protocol_settings)?;

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
    pub fn to_json(&self, _protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
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
                    _protocol_settings.address_version,
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

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        let timestamp_ms = required_u64_number(json, "timestamp")?;
        let asset_hash =
            required_script_hash_or_address(json, "assethash", _protocol_settings, "asset hash")?;
        let user_script_hash =
            optional_script_hash_or_address_lossy(json, "transferaddress", _protocol_settings);
        let amount = required_bigint_string(json, "amount", "amount")?;
        let block_index = required_u32_number(json, "blockindex")?;
        let transfer_notify_index = required_u16_number(json, "transfernotifyindex")?;
        let tx_hash = required_uint256(json, "txhash")?;

        Ok(Self {
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
    use super::super::test_fixtures::rpc_case_result;
    use super::*;
    use neo_config::ProtocolSettings;

    #[test]
    fn transfer_roundtrip() {
        let entry = RpcNep17Transfer {
            timestamp_ms: 1234,
            asset_hash: UInt160::zero(),
            user_script_hash: Some(UInt160::zero()),
            amount: BigInt::from(7),
            block_index: 9,
            transfer_notify_index: 1,
            tx_hash: UInt256::zero(),
        };
        let json = entry.to_json(&ProtocolSettings::default_settings());
        let parsed =
            RpcNep17Transfer::from_json(&json, &ProtocolSettings::default_settings()).unwrap();
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
        let entry = RpcNep17Transfer {
            timestamp_ms: 1,
            asset_hash: UInt160::zero(),
            user_script_hash: None,
            amount: BigInt::from(11),
            block_index: 2,
            transfer_notify_index: 0,
            tx_hash: UInt256::zero(),
        };
        let transfers = RpcNep17Transfers {
            user_script_hash: UInt160::zero(),
            sent: vec![entry.clone()],
            received: vec![entry.clone()],
        };
        let json = transfers.to_json(&ProtocolSettings::default_settings());
        let parsed =
            RpcNep17Transfers::from_json(&json, &ProtocolSettings::default_settings()).unwrap();

        assert_eq!(parsed.user_script_hash, transfers.user_script_hash);
        assert_eq!(parsed.sent.len(), 1);
        assert_eq!(parsed.received.len(), 1);
        assert_eq!(parsed.sent[0].amount, entry.amount);
        assert_eq!(parsed.received[0].user_script_hash, entry.user_script_hash);
    }

    #[test]
    fn nep17_transfers_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getnep17transfersasync") else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcNep17Transfers::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }

    #[test]
    fn nep17_transfers_null_address_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getnep17transfersasync_with_null_transferaddress")
        else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcNep17Transfers::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
