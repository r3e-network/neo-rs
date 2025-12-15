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

/// RPC block information matching C# RpcBlock
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
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = super::super::utility::block_to_json(&self.block, protocol_settings);
        json.insert(
            "confirmations".to_string(),
            neo_json::JToken::Number(self.confirmations as f64),
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let block = super::super::utility::block_from_json(json, protocol_settings)?;

        let confirmations = json
            .get("confirmations")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'confirmations' field")? as u32;

        let next_block_hash = json
            .get("nextblockhash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(&s).ok());

        Ok(Self {
            block,
            confirmations,
            next_block_hash,
        })
    }
}
