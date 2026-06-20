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
        let mut json = entry.to_json(&settings);
        json.insert(
            "tokenid".to_string(),
            JToken::String("0X010203".to_string()),
        );
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

    #[test]
    fn transfer_to_json_keeps_token_id_before_shared_fields() {
        let entry = RpcNep11Transfer {
            token_id: vec![1, 2, 3],
            timestamp_ms: 1234,
            asset_hash: UInt160::zero(),
            user_script_hash: None,
            amount: BigInt::from(7),
            block_index: 9,
            transfer_notify_index: 1,
            tx_hash: UInt256::zero(),
        };

        assert_eq!(
            entry
                .to_json(&ProtocolSettings::default_settings())
                .to_string(),
            format!(
                r#"{{"tokenid":"010203","timestamp":1234,"assethash":"{}","transferaddress":null,"amount":"7","blockindex":9,"transfernotifyindex":1,"txhash":"{}"}}"#,
                UInt160::zero(),
                UInt256::zero()
            )
        );
    }
}
