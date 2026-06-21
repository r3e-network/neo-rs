use super::super::ClientRpcError;
use super::super::models::RpcPlugin;
use neo_primitives::UInt256;
use neo_serialization::json::{JObject, JToken};

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

pub(super) fn parse_object_field<T>(
    token: JToken,
    context: &str,
    field: &str,
    missing_error: &str,
    parse_field: impl FnOnce(&JToken) -> Result<T, ClientRpcError>,
) -> Result<T, ClientRpcError> {
    let obj = token_as_object(token, context)?;
    let field_token = obj
        .get(field)
        .ok_or_else(|| ClientRpcError::new(-32603, missing_error))?;
    parse_field(field_token)
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

pub(super) fn parse_i64_number_or_string_token(
    token: &JToken,
    value_name: &str,
    invalid_type_error: &str,
) -> Result<i64, ClientRpcError> {
    match token {
        JToken::Number(value) => Ok(*value as i64),
        JToken::String(value) => value.parse::<i64>().map_err(|_| {
            ClientRpcError::new(-32603, format!("Invalid {value_name} value: {value}"))
        }),
        _ => Err(ClientRpcError::new(-32603, invalid_type_error)),
    }
}

pub(super) fn parse_i64_object_field(
    token: JToken,
    context: &str,
    field: &str,
    missing_error: &str,
    value_name: &str,
    invalid_type_error: &str,
) -> Result<i64, ClientRpcError> {
    parse_object_field(token, context, field, missing_error, |field_token| {
        parse_i64_number_or_string_token(field_token, value_name, invalid_type_error)
    })
}

pub(super) fn parse_uint256_string_token(
    token: &JToken,
    missing_or_type_error: &str,
    invalid_hash_prefix: &str,
) -> Result<UInt256, ClientRpcError> {
    let hash = token
        .as_string()
        .ok_or_else(|| ClientRpcError::new(-32603, missing_or_type_error))?;
    UInt256::parse(&hash)
        .map_err(|err| ClientRpcError::new(-32603, format!("{invalid_hash_prefix}: {err}")))
}

pub(super) fn parse_uint256_object_field(
    token: JToken,
    context: &str,
    field: &str,
    missing_or_type_error: &str,
    invalid_hash_prefix: &str,
) -> Result<UInt256, ClientRpcError> {
    parse_object_field(
        token,
        context,
        field,
        missing_or_type_error,
        |field_token| {
            parse_uint256_string_token(field_token, missing_or_type_error, invalid_hash_prefix)
        },
    )
}

pub(super) fn parse_object_array_result<T>(
    result: &JToken,
    non_array_error: &str,
    null_entry_error: &str,
    non_object_error: &str,
    mut parse_object: impl FnMut(&JObject) -> neo_error::CoreResult<T>,
) -> Result<Vec<T>, ClientRpcError> {
    let array = result
        .as_array()
        .ok_or_else(|| ClientRpcError::new(-32603, non_array_error))?;

    array
        .iter()
        .map(|item| {
            let token = item
                .as_ref()
                .ok_or_else(|| ClientRpcError::new(-32603, null_entry_error))?;
            let obj = token
                .as_object()
                .ok_or_else(|| ClientRpcError::new(-32603, non_object_error))?;
            parse_object(obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
        })
        .collect()
}

pub(super) fn parse_plugins(result: &JToken) -> Result<Vec<RpcPlugin>, ClientRpcError> {
    parse_object_array_result(
        result,
        "listplugins returned non-array",
        "plugin entry was null",
        "plugin entry was not an object",
        |obj| {
            RpcPlugin::from_json(obj)
                .map_err(|err| neo_error::CoreError::other(format!("invalid plugin entry: {err}")))
        },
    )
}

#[cfg(test)]
#[path = "../../tests/client/rpc_client/helpers.rs"]
mod tests;
