// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_method_token.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{CallFlags, MethodToken, UInt160};
use neo_json::JObject;

/// RPC method token helper matching C# RpcMethodToken
pub struct RpcMethodToken {
    /// The method token
    pub method_token: MethodToken,
}

impl RpcMethodToken {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let hash = json
            .get("hash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(s).ok())
            .ok_or("Missing or invalid 'hash' field")?;

        let method = json
            .get("method")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'method' field")?
            .to_string();

        let parameters_count = json
            .get("paramcount")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'paramcount' field")? as u16;

        let has_return_value = json
            .get("hasreturnvalue")
            .and_then(|v| v.as_boolean())
            .ok_or("Missing or invalid 'hasreturnvalue' field")?;

        let call_flags_str = json
            .get("callflags")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'callflags' field")?;

        let call_flags = CallFlags::from_str(call_flags_str)
            .map_err(|_| format!("Invalid call flags: {}", call_flags_str))?;

        Ok(Self {
            method_token: MethodToken {
                hash,
                method,
                parameters_count,
                has_return_value,
                call_flags,
            },
        })
    }
}
