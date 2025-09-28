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

use neo_core::{BlockHeader, ProtocolSettings, UInt256};
use neo_json::JObject;
use serde::{Deserialize, Serialize};

/// RPC block header information matching C# RpcBlockHeader
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        // TODO: Implement when BlockHeader is available
        Err("BlockHeader deserialization not yet implemented".to_string())
    }
}
