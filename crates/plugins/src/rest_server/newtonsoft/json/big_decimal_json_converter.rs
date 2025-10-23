// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.BigDecimalJsonConverter`.

use neo_core::big_decimal::BigDecimal;
use num_bigint::BigInt;
use serde_json::{json, Value};

/// JSON helper for serialising and deserialising [`BigDecimal`] instances.
pub struct BigDecimalJsonConverter;

impl BigDecimalJsonConverter {
    /// Serialises a [`BigDecimal`] into the `{ "value": "...", "decimals": ... }` structure.
    pub fn to_json(value: &BigDecimal) -> Value {
        json!({
            "value": value.value().to_string(),
            "decimals": value.decimals(),
        })
    }

    /// Deserialises a [`BigDecimal`] from the JSON representation produced by [`to_json`].
    pub fn from_json(token: &Value) -> Result<BigDecimal, String> {
        match token {
            Value::Object(map) => {
                let value_token = Self::get_case_insensitive(map, "value")
                    .ok_or_else(|| "missing value property for BigDecimal".to_string())?;
                let decimals_token = Self::get_case_insensitive(map, "decimals")
                    .ok_or_else(|| "missing decimals property for BigDecimal".to_string())?;

                let value = Self::parse_big_int(value_token)?;
                let decimals = decimals_token
                    .as_u64()
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or_else(|| "BigDecimal decimals must be between 0 and 255".to_string())?;

                Ok(BigDecimal::new(value, decimals))
            }
            Value::Number(_) => Err("Numeric BigDecimal values must be encoded as objects".to_string()),
            _ => Err("Unsupported BigDecimal JSON representation".to_string()),
        }
    }

    fn get_case_insensitive<'a>(
        map: &'a serde_json::Map<String, Value>,
        name: &str,
    ) -> Option<&'a Value> {
        let lower = name.to_lowercase();
        map.iter()
            .find(|(key, _)| key.to_lowercase() == lower)
            .map(|(_, value)| value)
    }

    fn parse_big_int(token: &Value) -> Result<BigInt, String> {
        match token {
            Value::String(text) => BigInt::parse_bytes(text.as_bytes(), 10)
                .ok_or_else(|| "Invalid BigInteger string".to_string()),
            Value::Number(number) => BigInt::parse_bytes(number.to_string().as_bytes(), 10)
                .ok_or_else(|| "Invalid BigInteger number".to_string()),
            _ => Err("BigInteger value must be a string or number".to_string()),
        }
    }

}
