// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nef_file.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::RpcMethodToken;
use neo_core::NefFile;
use neo_json::{JArray, JObject};

/// RPC NEF file helper matching C# RpcNefFile
pub struct RpcNefFile {
    /// The NEF file
    pub nef_file: NefFile,
}

impl RpcNefFile {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let compiler = json.get("compiler")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'compiler' field")?
            .to_string();
            
        let source = json.get("source")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'source' field")?
            .to_string();
            
        let tokens = json.get("tokens")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| RpcMethodToken::from_json(obj).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
            
        let script = json.get("script")
            .and_then(|v| v.as_string())
            .and_then(|s| base64::decode(s).ok())
            .ok_or("Missing or invalid 'script' field")?;
            
        let checksum = json.get("checksum")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'checksum' field")?
            as u32;
            
        Ok(Self {
            nef_file: NefFile {
                compiler,
                source,
                tokens: tokens.into_iter().map(|t| t.method_token).collect(),
                script,
                checksum,
            },
        })
    }
}