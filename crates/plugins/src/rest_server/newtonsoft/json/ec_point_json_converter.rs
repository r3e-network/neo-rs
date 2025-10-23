// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ECPointJsonConverter`.

use crate::rest_server::exceptions::uint256_format_exception::UInt256FormatException;
use neo_core::cryptography::crypto_utils::{ECCurve, ECPoint};
use serde_json::Value;

pub struct ECPointJsonConverter;

impl ECPointJsonConverter {
    pub fn to_json(value: &ECPoint) -> Value {
        Value::String(hex::encode_upper(value.as_bytes()))
    }

    pub fn from_json(token: &Value) -> Result<ECPoint, UInt256FormatException> {
        let value = token
            .as_str()
            .ok_or_else(|| UInt256FormatException::with_message("value must be a string"))?;

        let bytes = hex::decode(value).map_err(|_| {
            UInt256FormatException::with_message(format!("'{value}' is invalid."))
        })?;

        ECPoint::decode(&bytes, ECCurve::Secp256r1).map_err(|_| {
            UInt256FormatException::with_message(format!("'{value}' is invalid."))
        })
    }
}
