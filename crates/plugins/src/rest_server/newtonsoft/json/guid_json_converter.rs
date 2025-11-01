// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.GuidJsonConverter`.

use serde_json::Value;
use uuid::Uuid;

pub struct GuidJsonConverter;

impl GuidJsonConverter {
    pub fn to_json(value: &Uuid) -> Value {
        Value::String(value.simple().to_string())
    }

    pub fn from_json(token: &Value) -> Result<Uuid, String> {
        let value = token
            .as_str()
            .ok_or_else(|| "Guid value must be a string".to_string())?;

        Uuid::parse_str(value).map_err(|err| err.to_string())
    }
}
