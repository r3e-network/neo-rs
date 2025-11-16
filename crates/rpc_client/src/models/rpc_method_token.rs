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

use neo_core::{
    smart_contract::{contract_state, CallFlags},
    UInt160,
};
use neo_json::JObject;
/// RPC method token helper matching C# RpcMethodToken
pub struct RpcMethodToken {
    /// The method token
    pub method_token: contract_state::MethodToken,
}

impl RpcMethodToken {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let hash = json
            .get("hash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(&s).ok())
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
            .map(|v| v.as_boolean())
            .ok_or("Missing or invalid 'hasreturnvalue' field")?;

        let call_flags_str = json
            .get("callflags")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'callflags' field")?;

        let call_flags = parse_call_flags(&call_flags_str)
            .ok_or_else(|| format!("Invalid call flags: {}", call_flags_str))?;

        Ok(Self {
            method_token: contract_state::MethodToken {
                hash,
                method,
                parameters_count,
                has_return_value,
                call_flags,
            },
        })
    }
}

fn parse_call_flags(value: &str) -> Option<CallFlags> {
    if let Ok(bits) = value.parse::<u8>() {
        return CallFlags::from_bits(bits);
    }

    let cleaned = value.replace('_', "");
    let mut result = CallFlags::empty();
    let mut matched = false;

    for part in cleaned
        .split(|c: char| c == '|' || c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        matched = true;
        let flag = match part.to_ascii_uppercase().as_str() {
            "NONE" => CallFlags::empty(),
            "READSTATES" => CallFlags::READ_STATES,
            "WRITESTATES" => CallFlags::WRITE_STATES,
            "ALLOWCALL" => CallFlags::ALLOW_CALL,
            "ALLOWNOTIFY" => CallFlags::ALLOW_NOTIFY,
            "STATES" => CallFlags::STATES,
            "READONLY" => CallFlags::READ_ONLY,
            "ALL" => CallFlags::ALL,
            other => return None,
        };
        result |= flag;
    }

    if matched {
        Some(result)
    } else {
        None
    }
}
