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

use neo_core::smart_contract::{CallFlags, method_token::MethodToken};
use neo_json::JObject;
use neo_primitives::UInt160;
/// RPC method token helper matching C# `RpcMethodToken`
pub struct RpcMethodToken {
    /// The method token
    pub method_token: MethodToken,
}

impl RpcMethodToken {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let hash = json
            .get("hash")
            .and_then(neo_json::JToken::as_string)
            .and_then(|s| UInt160::parse(&s).ok())
            .ok_or("Missing or invalid 'hash' field")?;

        let method = json
            .get("method")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'method' field")?;

        let parameters_count = parse_u16_field(json, "paramcount")?;

        let has_return_value = json
            .get("hasreturnvalue")
            .map(neo_json::JToken::as_boolean)
            .ok_or("Missing or invalid 'hasreturnvalue' field")?;

        let call_flags_token = json
            .get("callflags")
            .ok_or("Missing or invalid 'callflags' field")?;
        let call_flags = if let Some(text) = call_flags_token.as_string() {
            parse_call_flags(&text).ok_or_else(|| format!("Invalid call flags: {text}"))?
        } else if let Some(number) = call_flags_token.as_number() {
            CallFlags::from_bits(number as u8)
                .ok_or_else(|| format!("Invalid call flags bits: {}", number as u8))?
        } else {
            return Err("Invalid 'callflags' field".to_string());
        };

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

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "hash".to_string(),
            neo_json::JToken::String(self.method_token.hash.to_string()),
        );
        json.insert(
            "method".to_string(),
            neo_json::JToken::String(self.method_token.method.clone()),
        );
        json.insert(
            "paramcount".to_string(),
            neo_json::JToken::Number(f64::from(self.method_token.parameters_count)),
        );
        json.insert(
            "hasreturnvalue".to_string(),
            neo_json::JToken::Boolean(self.method_token.has_return_value),
        );
        json.insert(
            "callflags".to_string(),
            neo_json::JToken::String(call_flags_to_string(self.method_token.call_flags)),
        );
        json
    }
}

fn call_flags_to_string(flags: CallFlags) -> String {
    if flags.is_empty() {
        return "None".to_string();
    }
    if flags == CallFlags::READ_STATES {
        return "ReadStates".to_string();
    }
    if flags == CallFlags::WRITE_STATES {
        return "WriteStates".to_string();
    }
    if flags == CallFlags::ALLOW_CALL {
        return "AllowCall".to_string();
    }
    if flags == CallFlags::ALLOW_NOTIFY {
        return "AllowNotify".to_string();
    }
    if flags == CallFlags::STATES {
        return "States".to_string();
    }
    if flags == CallFlags::READ_ONLY {
        return "ReadOnly".to_string();
    }
    if flags == CallFlags::ALL {
        return "All".to_string();
    }

    let mut parts = Vec::new();
    if flags.contains(CallFlags::READ_STATES) {
        parts.push("ReadStates");
    }
    if flags.contains(CallFlags::WRITE_STATES) {
        parts.push("WriteStates");
    }
    if flags.contains(CallFlags::ALLOW_CALL) {
        parts.push("AllowCall");
    }
    if flags.contains(CallFlags::ALLOW_NOTIFY) {
        parts.push("AllowNotify");
    }
    parts.join(", ")
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
            _other => return None,
        };
        result |= flag;
    }

    if matched { Some(result) } else { None }
}

fn parse_u16_field(json: &JObject, field: &str) -> Result<u16, String> {
    let token = json
        .get(field)
        .ok_or_else(|| format!("Missing '{field}' field"))?;
    if let Some(number) = token.as_number() {
        Ok(number as u16)
    } else if let Some(text) = token.as_string() {
        text.parse::<u16>()
            .map_err(|_| format!("Invalid {field} value: {text}"))
    } else {
        Err(format!("Invalid '{field}' field"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JToken;

    #[test]
    fn parses_method_token() {
        let mut json = JObject::new();
        json.insert(
            "hash".to_string(),
            JToken::String("0000000000000000000000000000000000000000".to_string()),
        );
        json.insert(
            "method".to_string(),
            JToken::String("balanceOf".to_string()),
        );
        json.insert("paramcount".to_string(), JToken::Number(1f64));
        json.insert("hasreturnvalue".to_string(), JToken::Boolean(true));
        json.insert(
            "callflags".to_string(),
            JToken::String("ReadOnly".to_string()),
        );

        let parsed = RpcMethodToken::from_json(&json).unwrap();
        assert_eq!(parsed.method_token.method, "balanceOf");
        assert!(parsed.method_token.has_return_value);
        assert_eq!(parsed.method_token.parameters_count, 1);
        assert!(
            parsed
                .method_token
                .call_flags
                .contains(CallFlags::READ_ONLY)
        );
    }

    #[test]
    fn parses_numeric_flags_and_paramcount_strings() {
        let mut json = JObject::new();
        json.insert(
            "hash".to_string(),
            JToken::String("0000000000000000000000000000000000000000".to_string()),
        );
        json.insert("method".to_string(), JToken::String("transfer".to_string()));
        json.insert("paramcount".to_string(), JToken::String("2".to_string()));
        json.insert("hasreturnvalue".to_string(), JToken::Boolean(true));
        json.insert("callflags".to_string(), JToken::Number(3f64));

        let parsed = RpcMethodToken::from_json(&json).unwrap();
        assert_eq!(parsed.method_token.parameters_count, 2);
        assert!(
            parsed
                .method_token
                .call_flags
                .contains(CallFlags::READ_STATES)
        );
        assert!(
            parsed
                .method_token
                .call_flags
                .contains(CallFlags::WRITE_STATES)
        );
    }

    #[test]
    fn method_token_roundtrip_json() {
        let token = RpcMethodToken {
            method_token: MethodToken {
                hash: UInt160::zero(),
                method: "transfer".into(),
                parameters_count: 2,
                has_return_value: true,
                call_flags: CallFlags::ALL,
            },
        };
        let json = token.to_json();
        let parsed = RpcMethodToken::from_json(&json).expect("method token");
        assert_eq!(parsed.method_token.method, token.method_token.method);
        assert_eq!(parsed.method_token.call_flags, CallFlags::ALL);
    }

    #[test]
    fn method_token_to_json_uses_named_flags() {
        let token = RpcMethodToken {
            method_token: MethodToken {
                hash: UInt160::from([
                    0x0e, 0x1b, 0x9b, 0xfa, 0xa4, 0x4e, 0x60, 0x31, 0x1f, 0x6f, 0x3c, 0x96, 0xcf,
                    0xcd, 0x6d, 0x12, 0xc2, 0xfc, 0x3a, 0xdd,
                ]),
                method: "test".into(),
                parameters_count: 1,
                has_return_value: true,
                call_flags: CallFlags::ALL,
            },
        };

        let json = token.to_json();
        assert_eq!(
            json.get("callflags")
                .and_then(|value| value.as_string())
                .unwrap_or_default(),
            "All"
        );
    }
}
