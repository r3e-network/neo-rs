// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.UInt256JsonConverter`.

use crate::rest_server::exceptions::uint256_format_exception::UInt256FormatException;
use neo_core::UInt256;
use serde_json::Value;
use std::str::FromStr;

pub struct UInt256JsonConverter;

impl UInt256JsonConverter {
    pub fn to_json(value: &UInt256) -> Value {
        Value::String(value.to_string())
    }

    pub fn from_json(token: &Value) -> Result<UInt256, UInt256FormatException> {
        let value = token
            .as_str()
            .ok_or_else(|| UInt256FormatException::with_message("value must be a string"))?;

        UInt256::from_str(value)
            .map_err(|_| UInt256FormatException::with_message(format!("'{value}' is invalid.")))
    }
}
