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
