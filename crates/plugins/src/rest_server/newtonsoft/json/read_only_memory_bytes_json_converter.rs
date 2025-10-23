// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ReadOnlyMemoryBytesJsonConverter`.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::Value;

pub struct ReadOnlyMemoryBytesJsonConverter;

impl ReadOnlyMemoryBytesJsonConverter {
    pub fn to_json(bytes: &[u8]) -> Value {
        Value::String(BASE64.encode(bytes))
    }

    pub fn from_json(token: &Value) -> Result<Vec<u8>, String> {
        let value = token
            .as_str()
            .ok_or_else(|| "ReadOnlyMemory bytes must be a base64 string".to_string())?;

        BASE64
            .decode(value.as_bytes())
            .map_err(|err| format!("Invalid base64: {err}"))
    }
}
