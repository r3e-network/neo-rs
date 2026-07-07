//! GAS-denominated RPC settings decoding.
//!
//! C# RpcServer settings accept `MaxGasInvoke` and `MaxFee` as either GAS units
//! or datoshi-scale integer values. This module keeps that compatibility parser
//! out of the settings record so the root module stays focused on configuration
//! shape and process-wide registry behavior.

use neo_error::CoreResult;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::Value;

use super::RpcServerConfig;

const GAS_UNIT_THRESHOLD: i64 = 1_000;

pub(super) fn deserialize_max_gas_invoke<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    parse_gas_value(value).map_err(de::Error::custom)
}

pub(super) fn deserialize_max_fee<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    parse_gas_value(value).map_err(de::Error::custom)
}

fn parse_gas_value(value: Value) -> CoreResult<i64> {
    match value {
        Value::Number(number) => parse_gas_number(&number),
        Value::String(text) => parse_gas_string(&text),
        Value::Null => Err(neo_error::CoreError::other("gas value cannot be null")),
        _ => Err(neo_error::CoreError::other(
            "gas value must be a number or string",
        )),
    }
}

fn parse_gas_number(number: &serde_json::Number) -> CoreResult<i64> {
    if let Some(int_value) = number.as_i64() {
        return apply_gas_threshold(int_value);
    }
    if let Some(uint_value) = number.as_u64() {
        let int_value = i64::try_from(uint_value)
            .map_err(|_| neo_error::CoreError::other("gas value exceeds i64"))?;
        return apply_gas_threshold(int_value);
    }
    let float_value = number
        .as_f64()
        .ok_or_else(|| neo_error::CoreError::other("gas value must be numeric"))?;
    convert_gas_units(float_value)
}

fn parse_gas_string(text: &str) -> CoreResult<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(neo_error::CoreError::other("gas value cannot be empty"));
    }
    if let Ok(int_value) = trimmed.parse::<i64>() {
        return apply_gas_threshold(int_value);
    }
    let float_value = trimmed
        .parse::<f64>()
        .map_err(|_| neo_error::CoreError::other("gas value must be numeric"))?;
    convert_gas_units(float_value)
}

fn apply_gas_threshold(value: i64) -> CoreResult<i64> {
    if value.abs() <= GAS_UNIT_THRESHOLD {
        value
            .checked_mul(RpcServerConfig::gas_datoshi_factor())
            .ok_or_else(|| neo_error::CoreError::other("gas value overflow"))
    } else {
        Ok(value)
    }
}

fn convert_gas_units(value: f64) -> CoreResult<i64> {
    if !value.is_finite() {
        return Err(neo_error::CoreError::other("gas value must be finite"));
    }
    let scaled = value * RpcServerConfig::gas_datoshi_factor() as f64;
    if scaled > i64::MAX as f64 || scaled < i64::MIN as f64 {
        return Err(neo_error::CoreError::other("gas value overflow"));
    }
    Ok(scaled.round() as i64)
}
