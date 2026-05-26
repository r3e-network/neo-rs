// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_block.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_config::ProtocolSettings;
use neo_core::Block;
use neo_json::JObject;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// RPC block information matching C# `RpcBlock`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcBlock {
    /// The block data
    pub block: Block,

    /// Number of confirmations
    pub confirmations: u32,

    /// Hash of the next block
    pub next_block_hash: Option<UInt256>,
}

impl RpcBlock {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = super::super::utility::block_to_json(&self.block, protocol_settings);
        json.insert(
            "confirmations".to_string(),
            neo_json::JToken::Number(f64::from(self.confirmations)),
        );

        if let Some(hash) = self.next_block_hash {
            json.insert(
                "nextblockhash".to_string(),
                neo_json::JToken::String(hash.to_string()),
            );
        } else {
            json.insert("nextblockhash".to_string(), neo_json::JToken::Null);
        }

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let block = super::super::utility::block_from_json(json, protocol_settings)?;

        let confirmations = json
            .get("confirmations")
            .and_then(neo_json::JToken::as_number)
            .ok_or("Missing or invalid 'confirmations' field")? as u32;

        let next_block_hash = json
            .get("nextblockhash")
            .and_then(neo_json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok());

        Ok(Self {
            block,
            confirmations,
            next_block_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;

    #[test]
    fn block_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getblockasync") else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcBlock::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
