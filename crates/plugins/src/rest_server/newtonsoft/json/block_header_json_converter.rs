// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.BlockHeaderJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::header::Header as BlockHeader;
use serde_json::Value;

pub struct BlockHeaderJsonConverter;

impl BlockHeaderJsonConverter {
    pub fn to_json(header: &BlockHeader) -> Value {
        RestServerUtility::block_header_to_j_token(header)
    }
}
