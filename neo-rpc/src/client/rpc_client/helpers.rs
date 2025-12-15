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
        _ => Err(ClientRpcError::new(
            -32603,
            format!("{context}: expected string token"),
        )),
    }
}

pub(super) fn token_as_number(token: JToken, context: &str) -> Result<f64, ClientRpcError> {
    match token {
        JToken::Number(value) => Ok(value),
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
