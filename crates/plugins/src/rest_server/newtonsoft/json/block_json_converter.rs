// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.BlockJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::block::Block;
use serde_json::Value;

pub struct BlockJsonConverter;

impl BlockJsonConverter {
    pub fn to_json(block: &Block) -> Value {
        RestServerUtility::block_to_j_token(block)
    }
}
