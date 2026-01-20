// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client/helpers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::models::RpcPlugin;
use super::super::ClientRpcError;
use neo_json::{JObject, JToken};

pub(super) fn token_as_string(token: JToken, context: &str) -> Result<String, ClientRpcError> {
    match token {
        JToken::String(value) => Ok(value),
        JToken::Number(value) => Ok(value.to_string()),
        JToken::Boolean(value) => Ok(value.to_string()),
        _ => Err(ClientRpcError::new(
            -32603,
            format!("{context}: expected string token"),
        )),
    }
}

pub(super) fn token_as_number(token: JToken, context: &str) -> Result<f64, ClientRpcError> {
    match token {
        JToken::Number(value) => Ok(value),
        JToken::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(0.0);
            }
            Ok(trimmed.parse::<f64>().unwrap_or(f64::NAN))
        }
        JToken::Boolean(value) => Ok(if value { 1.0 } else { 0.0 }),
        _ => Err(ClientRpcError::new(
            -32603,
            format!("{context}: expected numeric token"),
        )),
    }
}

pub(super) fn token_as_object(token: JToken, context: &str) -> Result<JObject, ClientRpcError> {
    match token {
        JToken::Object(obj) => Ok(obj),
        _ => Err(ClientRpcError::new(
            -32603,
            format!("{context}: expected object token"),
        )),
    }
}

pub(super) fn token_as_boolean(token: JToken, context: &str) -> Result<bool, ClientRpcError> {
    match token {
        JToken::Boolean(value) => Ok(value),
        JToken::Number(value) => Ok(value != 0.0),
        JToken::String(value) => Ok(!value.is_empty()),
        JToken::Array(_) | JToken::Object(_) => Ok(true),
        _ => Err(ClientRpcError::new(
            -32603,
            format!("{context}: expected boolean token"),
        )),
    }
}

pub(super) fn parse_plugins(result: &JToken) -> Result<Vec<RpcPlugin>, ClientRpcError> {
    let array = result
        .as_array()
        .ok_or_else(|| ClientRpcError::new(-32603, "listplugins returned non-array"))?;

    array
        .iter()
        .map(|item| {
            let token = item
                .as_ref()
                .ok_or_else(|| ClientRpcError::new(-32603, "plugin entry was null"))?;
            let obj = token
                .as_object()
                .ok_or_else(|| ClientRpcError::new(-32603, "plugin entry was not an object"))?;
            RpcPlugin::from_json(obj)
                .map_err(|err| ClientRpcError::new(-32603, format!("invalid plugin entry: {err}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::{JArray, JObject, JToken};

    #[test]
    fn token_as_string_accepts_number_and_boolean() {
        let value = token_as_string(JToken::Number(12.0), "ctx").expect("number");
        assert_eq!(value, "12");
        let value = token_as_string(JToken::Boolean(true), "ctx").expect("bool");
        assert_eq!(value, "true");
    }

    #[test]
    fn token_as_number_accepts_string_and_boolean() {
        let value = token_as_number(JToken::String("7".into()), "ctx").expect("string");
        assert_eq!(value, 7.0);
        let value = token_as_number(JToken::Boolean(false), "ctx").expect("bool");
        assert_eq!(value, 0.0);
        let value = token_as_number(JToken::String("".into()), "ctx").expect("empty");
        assert_eq!(value, 0.0);
        let value = token_as_number(JToken::String("nope".into()), "ctx").expect("nan");
        assert!(value.is_nan());
    }

    #[test]
    fn token_as_boolean_accepts_string_number_and_container() {
        assert!(token_as_boolean(JToken::String("x".into()), "ctx").unwrap());
        assert!(!token_as_boolean(JToken::String("".into()), "ctx").unwrap());
        assert!(token_as_boolean(JToken::Number(1.0), "ctx").unwrap());
        assert!(!token_as_boolean(JToken::Number(0.0), "ctx").unwrap());
        assert!(token_as_boolean(JToken::Array(JArray::new()), "ctx").unwrap());
        assert!(token_as_boolean(JToken::Object(JObject::new()), "ctx").unwrap());
    }
}
