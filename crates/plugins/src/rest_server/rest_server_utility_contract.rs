// Copyright (C) 2015-2025 The Neo Project.
//
// Helper routines that translate the contract-related JSON structures used by
// the REST server into the strongly typed runtime equivalents. Mirrors the
// functionality provided by `RestServerUtility.Contract*` in the C# plugin.

use crate::rest_server::models::contract::invoke_params::InvokeParams;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::cryptography::crypto_utils::{ECPoint, ECCurve};
use neo_core::network::p2p::payloads::Signer;
use neo_core::smart_contract::contract_parameter::{
    ContractParameter, ContractParameterValue,
};
use neo_core::smart_contract::ContractParameterType;
use neo_core::{UInt160, UInt256, WitnessScope};
use num_bigint::BigInt;
use serde_json::{Map, Value};
use std::str::FromStr;
use neo_vm::stack_item::StackItem;
use std::collections::BTreeMap;

/// Contract utility functions matching the C# RestServerUtility helpers.
impl super::RestServerUtility {
    /// Creates an `InvokeParams` instance from a JSON token.
    pub fn contract_invoke_parameters_from_j_token(
        token: &Value,
    ) -> Result<InvokeParams, String> {
        let obj = token
            .as_object()
            .ok_or_else(|| "Invoke parameters JSON must be an object".to_string())?;

        let contract_params_value = get_case_insensitive(obj, "contractParameters")
            .ok_or_else(|| "Missing contractParameters array".to_string())?;
        let signers_value = get_case_insensitive(obj, "signers")
            .ok_or_else(|| "Missing signers array".to_string())?;

        let contract_parameters = contract_params_value
            .as_array()
            .ok_or_else(|| "contractParameters must be an array".to_string())?
            .iter()
            .map(Self::contract_parameter_from_j_token)
            .collect::<Result<Vec<_>, _>>()?;

        let signers = signers_value
            .as_array()
            .ok_or_else(|| "signers must be an array".to_string())?
            .iter()
            .map(Self::signer_from_j_token)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(InvokeParams::new(contract_parameters, signers))
    }

    /// Creates a signer from a JSON token.
    pub fn signer_from_j_token(token: &Value) -> Result<Signer, String> {
        let obj = token
            .as_object()
            .ok_or_else(|| "Signer JSON must be an object".to_string())?;

        let account = get_case_insensitive(obj, "account")
            .and_then(Value::as_str)
            .ok_or_else(|| "Signer.account must be a string".to_string())
            .and_then(|value| {
                UInt160::from_str(value)
                    .map_err(|err| format!("Invalid signer account: {err}"))
            })?;

        let scopes = get_case_insensitive(obj, "scopes")
            .and_then(Value::as_str)
            .ok_or_else(|| "Signer.scopes must be a string".to_string())
            .and_then(|value| {
                WitnessScope::from_str(value)
                    .map_err(|err| format!("Invalid witness scope: {err}"))
            })?;

        Ok(Signer::new(account, scopes))
    }

    /// Creates a contract parameter from a JSON token.
    pub fn contract_parameter_from_j_token(
        token: &Value,
    ) -> Result<ContractParameter, String> {
        let obj = token
            .as_object()
            .ok_or_else(|| "ContractParameter JSON must be an object".to_string())?;

        let type_str = get_case_insensitive(obj, "type")
            .and_then(Value::as_str)
            .ok_or_else(|| "ContractParameter.type must be provided".to_string())?;

        let parameter_type = parse_contract_parameter_type(type_str)?;
        let value_token = get_case_insensitive(obj, "value");

        let parameter = match parameter_type {
            ContractParameterType::Any => {
                ContractParameter::new(ContractParameterType::Any)
            }
            ContractParameterType::Boolean => {
                let value = value_token
                    .and_then(Value::as_bool)
                    .ok_or_else(|| "Boolean contract parameter requires a boolean value".to_string())?;
                ContractParameter::with_value(
                    ContractParameterType::Boolean,
                    ContractParameterValue::Boolean(value),
                )
            }
            ContractParameterType::Integer => {
                let value = value_token
                    .ok_or_else(|| "Integer contract parameter requires a value".to_string())?;
                let text = match value {
                    Value::String(text) => text.to_string(),
                    Value::Number(number) => number.to_string(),
                    _ => {
                        return Err(
                            "Integer contract parameter requires a numeric string value"
                                .to_string(),
                        )
                    }
                };
                let int = BigInt::from_str(&text)
                    .map_err(|err| format!("Invalid integer value: {err}"))?;
                ContractParameter::with_value(
                    ContractParameterType::Integer,
                    ContractParameterValue::Integer(int),
                )
            }
            ContractParameterType::ByteArray => {
                let bytes = parse_base64_value(value_token, "ByteArray contract parameter")?;
                ContractParameter::with_value(
                    ContractParameterType::ByteArray,
                    ContractParameterValue::ByteArray(bytes),
                )
            }
            ContractParameterType::Signature => {
                let bytes = parse_base64_value(value_token, "Signature contract parameter")?;
                ContractParameter::with_value(
                    ContractParameterType::Signature,
                    ContractParameterValue::Signature(bytes),
                )
            }
            ContractParameterType::String => {
                let text = value_token
                    .and_then(Value::as_str)
                    .ok_or_else(|| "String contract parameter requires a string value".to_string())?;
                ContractParameter::with_value(
                    ContractParameterType::String,
                    ContractParameterValue::String(text.to_string()),
                )
            }
            ContractParameterType::Hash160 => {
                let text = value_token
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Hash160 contract parameter requires a string value".to_string())?;
                let hash = UInt160::from_str(text)
                    .map_err(|err| format!("Invalid UInt160 value: {err}"))?;
                ContractParameter::with_value(
                    ContractParameterType::Hash160,
                    ContractParameterValue::Hash160(hash),
                )
            }
            ContractParameterType::Hash256 => {
                let text = value_token
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Hash256 contract parameter requires a string value".to_string())?;
                let hash = UInt256::from_str(text)
                    .map_err(|err| format!("Invalid UInt256 value: {err}"))?;
                ContractParameter::with_value(
                    ContractParameterType::Hash256,
                    ContractParameterValue::Hash256(hash),
                )
            }
            ContractParameterType::PublicKey => {
                let text = value_token
                    .and_then(Value::as_str)
                    .ok_or_else(|| "PublicKey contract parameter requires a string value".to_string())?;
                let point = parse_public_key(text)?;
                ContractParameter::with_value(
                    ContractParameterType::PublicKey,
                    ContractParameterValue::PublicKey(point),
                )
            }
            ContractParameterType::Array => {
                let array = value_token
                    .and_then(Value::as_array)
                    .ok_or_else(|| "Array contract parameter requires an array value".to_string())?;
                let values = array
                    .iter()
                    .map(Self::contract_parameter_from_j_token)
                    .collect::<Result<Vec<_>, _>>()?;
                ContractParameter::with_value(
                    ContractParameterType::Array,
                    ContractParameterValue::Array(values),
                )
            }
            ContractParameterType::Map => {
                let array = value_token
                    .and_then(Value::as_array)
                    .ok_or_else(|| "Map contract parameter requires an array value".to_string())?;
                let entries = array
                    .iter()
                    .map(parse_map_entry)
                    .collect::<Result<Vec<_>, _>>()?;
                ContractParameter::with_value(
                    ContractParameterType::Map,
                    ContractParameterValue::Map(entries),
                )
            }
            other => {
                return Err(format!(
                    "ContractParameterType({other:?}) is not supported in REST parsing"
                ));
            }
        };

        Ok(parameter)
    }

    /// Converts a contract parameter into a VM stack item mirroring the C# behaviour.
    pub fn contract_parameter_to_stack_item(
        parameter: &ContractParameter,
    ) -> Result<StackItem, String> {
        match &parameter.value {
            ContractParameterValue::Any | ContractParameterValue::Void => Ok(StackItem::null()),
            ContractParameterValue::Boolean(value) => Ok(StackItem::from_bool(*value)),
            ContractParameterValue::Integer(value) => {
                Ok(StackItem::from_int(value.clone()))
            }
            ContractParameterValue::Hash160(value) => {
                Ok(StackItem::from_byte_string(value.to_bytes()))
            }
            ContractParameterValue::Hash256(value) => {
                Ok(StackItem::from_byte_string(value.to_bytes()))
            }
            ContractParameterValue::ByteArray(bytes) => {
                Ok(StackItem::from_byte_string(bytes.clone()))
            }
            ContractParameterValue::Signature(bytes) => {
                Ok(StackItem::from_byte_string(bytes.clone()))
            }
            ContractParameterValue::PublicKey(point) => {
                Ok(StackItem::from_byte_string(point.encoded()))
            }
            ContractParameterValue::String(value) => {
                Ok(StackItem::from_byte_string(value.clone().into_bytes()))
            }
            ContractParameterValue::Array(items) => {
                let stack_items = items
                    .iter()
                    .map(Self::contract_parameter_to_stack_item)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(StackItem::from_array(stack_items))
            }
            ContractParameterValue::Map(entries) => {
                let mut map = BTreeMap::new();
                for (key, value) in entries {
                    let key_item = Self::contract_parameter_to_stack_item(key)?;
                    let value_item = Self::contract_parameter_to_stack_item(value)?;
                    map.insert(key_item, value_item);
                }
                Ok(StackItem::from_map(map))
            }
            ContractParameterValue::InteropInterface => Err(
                "InteropInterface contract parameter is not supported for invocation"
                    .to_string(),
            ),
        }
    }
}

fn get_case_insensitive<'a>(
    map: &'a Map<String, Value>,
    target: &str,
) -> Option<&'a Value> {
    map.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(target))
        .map(|(_, value)| value)
}

fn parse_contract_parameter_type(input: &str) -> Result<ContractParameterType, String> {
    match input {
        value if value.eq_ignore_ascii_case("Any") => Ok(ContractParameterType::Any),
        value if value.eq_ignore_ascii_case("Boolean") => Ok(ContractParameterType::Boolean),
        value if value.eq_ignore_ascii_case("Integer") => Ok(ContractParameterType::Integer),
        value if value.eq_ignore_ascii_case("ByteArray") => Ok(ContractParameterType::ByteArray),
        value if value.eq_ignore_ascii_case("String") => Ok(ContractParameterType::String),
        value if value.eq_ignore_ascii_case("Hash160") => Ok(ContractParameterType::Hash160),
        value if value.eq_ignore_ascii_case("Hash256") => Ok(ContractParameterType::Hash256),
        value if value.eq_ignore_ascii_case("PublicKey") => Ok(ContractParameterType::PublicKey),
        value if value.eq_ignore_ascii_case("Signature") => Ok(ContractParameterType::Signature),
        value if value.eq_ignore_ascii_case("Array") => Ok(ContractParameterType::Array),
        value if value.eq_ignore_ascii_case("Map") => Ok(ContractParameterType::Map),
        value if value.eq_ignore_ascii_case("InteropInterface") => {
            Ok(ContractParameterType::InteropInterface)
        }
        value if value.eq_ignore_ascii_case("Void") => Ok(ContractParameterType::Void),
        other => Err(format!("Unknown contract parameter type: {other}")),
    }
}

fn parse_base64_value(value: Option<&Value>, context: &str) -> Result<Vec<u8>, String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{context} requires a base64 string value"))?;
    BASE64
        .decode(text)
        .map_err(|err| format!("Failed to decode base64 value: {err}"))
}

fn parse_public_key(text: &str) -> Result<ECPoint, String> {
    let decode_hex = |value: &str| hex::decode(value).map_err(|err| err.to_string());
    let raw = if let Some(stripped) = text.strip_prefix("0x") {
        decode_hex(stripped)?
    } else if text.len() % 2 == 0 && text.chars().all(|c| c.is_ascii_hexdigit()) {
        decode_hex(text)?
    } else {
        BASE64
            .decode(text)
            .map_err(|err| format!("Invalid public key encoding: {err}"))?
    };

    ECPoint::decode(&raw, ECCurve::Secp256r1)
        .map_err(|err| format!("Invalid ECPoint: {err}"))
}

fn parse_map_entry(
    entry: &Value,
) -> Result<(ContractParameter, ContractParameter), String> {
    let obj = entry
        .as_object()
        .ok_or_else(|| "Map entries must be JSON objects".to_string())?;
    let key_value = get_case_insensitive(obj, "key")
        .ok_or_else(|| "Map entry missing key property".to_string())?;
    let value_value = get_case_insensitive(obj, "value")
        .ok_or_else(|| "Map entry missing value property".to_string())?;

    let key = super::RestServerUtility::contract_parameter_from_j_token(key_value)?;
    let value = super::RestServerUtility::contract_parameter_from_j_token(value_value)?;
    Ok((key, value))
}

#[cfg(test)]
mod tests {
    use crate::rest_server::RestServerUtility;
    use neo_core::smart_contract::contract_parameter::ContractParameterValue;
    use neo_core::WitnessScope;
    use num_bigint::BigInt;

    #[test]
    fn parses_invoke_params_from_json() {
        let json = serde_json::json!({
            "contractParameters": [
                { "type": "String", "value": "hello" },
                { "type": "Integer", "value": "42" }
            ],
            "signers": [
                {
                    "account": "0x0000000000000000000000000000000000000001",
                    "scopes": "CalledByEntry"
                }
            ]
        });

        let result =
            RestServerUtility::contract_invoke_parameters_from_j_token(&json).unwrap();
        assert_eq!(result.contract_parameters.len(), 2);
        assert_eq!(result.signers.len(), 1);
        assert_eq!(result.signers[0].scopes, WitnessScope::CALLED_BY_ENTRY);

        match &result.contract_parameters[0].value {
            ContractParameterValue::String(value) => assert_eq!(value, "hello"),
            other => panic!("Unexpected parameter value: {other:?}"),
        }

        match &result.contract_parameters[1].value {
            ContractParameterValue::Integer(value) => {
                assert_eq!(value, &BigInt::from(42))
            }
            other => panic!("Unexpected parameter value: {other:?}"),
        }
    }
}
