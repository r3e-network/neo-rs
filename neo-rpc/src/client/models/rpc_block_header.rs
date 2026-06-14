// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_block_header.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::utility::{RpcUtility, object_array, witness_to_json};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_io::Serializable;
use neo_payloads::BlockHeader;
use neo_primitives::UInt256;
use neo_serialization::json::{JObject, JToken};
use neo_wallets::wallet_helper as WalletHelper;
use serde::{Deserialize, Serialize};

/// RPC block header information matching C# `RpcBlockHeader`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcBlockHeader {
    /// The block header data
    pub header: BlockHeader,

    /// Number of confirmations
    pub confirmations: u32,

    /// Hash of the next block
    pub next_block_hash: Option<UInt256>,
}

impl RpcBlockHeader {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let version = json
            .get("version")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'version' field"))? as u32;

        let previous_hash = json
            .get("previousblockhash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'previousblockhash' field"))?;

        let merkle_root = json
            .get("merkleroot")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'merkleroot' field"))?;

        let timestamp = json
            .get("time")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'time' field"))? as u64;

        let nonce_str = json
            .get("nonce")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'nonce' field"))?;
        let nonce = u64::from_str_radix(nonce_str.trim_start_matches("0x"), 16)
            .map_err(|_| CoreError::other(format!("Invalid nonce value: {nonce_str}")))?;

        let index = json
            .get("index")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'index' field"))? as u32;

        let primary_index = json
            .get("primary")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'primary' field"))? as u8;

        let next_consensus_str = json
            .get("nextconsensus")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'nextconsensus' field"))?;
        let next_consensus = RpcUtility::get_script_hash(&next_consensus_str, protocol_settings)
            .map_err(|err| CoreError::other(format!("Invalid next consensus value: {err}")))?;

        let witnesses = json
            .get("witnesses")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CoreError::other("Missing 'witnesses' array"))?;
        let mut parsed_witnesses = Vec::with_capacity(witnesses.len());
        for entry in witnesses.iter() {
            let witness_token = entry.as_ref().ok_or_else(|| CoreError::other("Invalid witness entry: null value"))?;
            let witness_obj = witness_token
                .as_object()
                .ok_or_else(|| CoreError::other("Invalid witness entry: expected object"))?;
            parsed_witnesses.push(RpcUtility::witness_from_json(witness_obj)?);
        }

        let header = BlockHeader::new_with_witnesses(
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            parsed_witnesses,
        );

        let confirmations = json
            .get("confirmations")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'confirmations' field"))? as u32;

        let next_block_hash = json
            .get("nextblockhash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok());

        Ok(Self {
            header,
            confirmations,
            next_block_hash,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let header = &self.header;
        let mut json = JObject::new();
        json.insert(
            "hash".to_string(),
            JToken::String(header.hash().to_string()),
        );
        json.insert("size".to_string(), JToken::Number(header.size() as f64));
        json.insert(
            "version".to_string(),
            JToken::Number(f64::from(header.version())),
        );
        json.insert(
            "previousblockhash".to_string(),
            JToken::String(header.prev_hash().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            JToken::String(header.merkle_root().to_string()),
        );
        json.insert(
            "time".to_string(),
            JToken::Number(header.timestamp() as f64),
        );
        json.insert(
            "nonce".to_string(),
            JToken::String(format!("{:016X}", header.nonce())),
        );
        json.insert(
            "index".to_string(),
            JToken::Number(f64::from(header.index())),
        );
        json.insert(
            "primary".to_string(),
            JToken::Number(f64::from(header.primary_index())),
        );
        json.insert(
            "nextconsensus".to_string(),
            JToken::String(WalletHelper::to_address(
                header.next_consensus(),
                protocol_settings.address_version,
            )),
        );
        json.insert(
            "witnesses".to_string(),
            object_array(std::slice::from_ref(&header.witness), witness_to_json),
        );
        json.insert(
            "confirmations".to_string(),
            JToken::Number(f64::from(self.confirmations)),
        );
        if let Some(next_block_hash) = &self.next_block_hash {
            json.insert(
                "nextblockhash".to_string(),
                JToken::String(next_block_hash.to_string()),
            );
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use neo_serialization::json::{JArray, JToken};

    fn sample_witness_json(invocation: &[u8], verification: &[u8]) -> JObject {
        let mut obj = JObject::new();
        obj.insert(
            "invocation".to_string(),
            JToken::String(BASE64.encode(invocation)),
        );
        obj.insert(
            "verification".to_string(),
            JToken::String(BASE64.encode(verification)),
        );
        obj
    }

    #[test]
    fn parses_block_header_from_json() {
        let mut json = JObject::new();
        json.insert("version".to_string(), JToken::Number(0.0));
        json.insert(
            "previousblockhash".to_string(),
            JToken::String(UInt256::zero().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            JToken::String(UInt256::zero().to_string()),
        );
        json.insert("time".to_string(), JToken::Number(123.0));
        json.insert(
            "nonce".to_string(),
            JToken::String(format!("{:016x}", 42u64)),
        );
        json.insert("index".to_string(), JToken::Number(5.0));
        json.insert("primary".to_string(), JToken::Number(3.0));
        json.insert(
            "nextconsensus".to_string(),
            JToken::String(neo_primitives::UInt160::zero().to_string()),
        );

        let witness_json = sample_witness_json(&[1, 2, 3], &[4, 5, 6]);
        json.insert(
            "witnesses".to_string(),
            JToken::Array(JArray::from(vec![JToken::Object(witness_json)])),
        );

        json.insert("confirmations".to_string(), JToken::Number(8.0));
        json.insert(
            "nextblockhash".to_string(),
            JToken::String(UInt256::zero().to_string()),
        );

        let settings = ProtocolSettings::default();
        let rpc_header = RpcBlockHeader::from_json(&json, &settings).expect("should parse");

        assert_eq!(rpc_header.header.version(), 0);
        assert_eq!(rpc_header.header.timestamp(), 123);
        assert_eq!(rpc_header.header.nonce(), 42);
        assert_eq!(rpc_header.confirmations, 8);
        assert!(rpc_header.next_block_hash.is_some());
        // header carries exactly one witness by type (single `witness` field)
    }

    #[test]
    fn block_header_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getblockheaderasync") else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcBlockHeader::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
