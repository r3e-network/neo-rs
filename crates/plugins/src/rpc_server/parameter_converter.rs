use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_core::cryptography::crypto_utils::ECPoint;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::contract_parameter::ContractParameter;
use neo_core::uint160::UInt160;
use neo_core::{WitnessRule, WitnessScope};
use neo_json::{JArray, JObject, JToken};
use std::str::FromStr;
use uuid::Uuid;

use super::model::{Address, BlockHashOrIndex, ContractNameOrHashOrId, SignersAndWitnesses};
use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;

/// Context supplied when converting RPC parameters.
#[derive(Debug, Clone, Copy)]
pub struct ConversionContext {
    pub address_version: u8,
}

impl ConversionContext {
    pub fn new(address_version: u8) -> Self {
        Self { address_version }
    }
}

/// Trait implemented by types that can be constructed from a JSON-RPC token.
pub trait RpcConvertible: Sized {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException>;
}

pub struct ParameterConverter;

impl ParameterConverter {
    pub fn convert<T: RpcConvertible>(
        token: &JToken,
        ctx: &ConversionContext,
    ) -> Result<T, RpcException> {
        T::from_token(token, ctx)
    }

    pub fn convert_optional<T: RpcConvertible>(
        token: Option<&JToken>,
        ctx: &ConversionContext,
    ) -> Result<Option<T>, RpcException> {
        match token {
            Some(value) => T::from_token(value, ctx).map(Some),
            None => Ok(None),
        }
    }
}

impl RpcConvertible for String {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        expect_string(token, "Expected string value")
    }
}

impl RpcConvertible for bool {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        Ok(token.as_boolean())
    }
}

macro_rules! impl_numeric_convertible {
    ($($ty:ty),+) => {
        $(
            impl RpcConvertible for $ty {
                fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
                    let value = token
                        .as_number()
                        .ok_or_else(|| invalid_params("Expected numeric value"))?;
                    if value.is_nan() || value.is_infinite() {
                        return Err(invalid_params(format!(
                            "Invalid numeric value: {}",
                            token.to_string_value()
                        )));
                    }

                    let min = <$ty>::MIN as f64;
                    let max = <$ty>::MAX as f64;
                    if value < min || value > max {
                        return Err(invalid_params(format!("Numeric value out of range for {}: {}", stringify!($ty), value)));
                    }

                    if !matches!(stringify!($ty), "f32" | "f64") {
                        let rounded = value.round();
                        if (value - rounded).abs() > f64::EPSILON {
                            return Err(invalid_params(format!("Non-integer value for {}: {}", stringify!($ty), value)));
                        }
                        return Ok(rounded as $ty);
                    }

                    Ok(value as $ty)
                }
            }
        )+
    };
}

impl_numeric_convertible!(i8, u8, i16, u16, i32, u32, i64, u64, f64);

impl RpcConvertible for Vec<u8> {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected Base64 string")?;
        BASE64_STANDARD
            .decode(text.trim())
            .map_err(|_| invalid_params("Invalid Base64-encoded bytes"))
    }
}

impl RpcConvertible for Address {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected address string")?;
        parse_address(&text, ctx.address_version)
    }
}

impl RpcConvertible for Vec<Address> {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut result = Vec::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item
                .as_ref()
                .ok_or_else(|| invalid_params(format!("Null address entry at index {}", index)))?;
            result.push(<Address as RpcConvertible>::from_token(token, ctx)?);
        }
        Ok(result)
    }
}

impl RpcConvertible for BlockHashOrIndex {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected block hash or index string")?;
        BlockHashOrIndex::try_parse(&text)
            .ok_or_else(|| invalid_params(format!("Invalid block hash or index: {}", text)))
    }
}

impl RpcConvertible for ContractNameOrHashOrId {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected contract identifier string")?;
        ContractNameOrHashOrId::try_parse(&text)
            .ok_or_else(|| invalid_params(format!("Invalid contract identifier: {}", text)))
    }
}

impl RpcConvertible for Uuid {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected UUID string")?;
        Uuid::from_str(text.trim()).map_err(|_| invalid_params(format!("Invalid UUID: {}", text)))
    }
}

impl RpcConvertible for SignersAndWitnesses {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        if array.count() > MAX_TRANSACTION_ATTRIBUTES {
            return Err(invalid_params("Max allowed signers exceeded"));
        }

        let mut signers = Vec::with_capacity(array.count());
        let mut witnesses = Vec::new();

        for (index, entry) in array.children().iter().enumerate() {
            let token = entry.as_ref().ok_or_else(|| {
                invalid_params(format!("Invalid signer entry at index {}", index))
            })?;
            let obj = expect_object(token)?;

            let signer_token = obj.get("signer").ok_or_else(|| {
                invalid_params(format!("Missing signer object at index {}", index))
            })?;
            let signer = parse_signer(signer_token, ctx)?;
            signers.push(signer);

            if let Some(witness_token) = obj.get("witness") {
                if !matches!(witness_token, JToken::Null) {
                    let witness = parse_witness(witness_token)?;
                    witnesses.push(witness);
                }
            }
        }

        Ok(SignersAndWitnesses::new(signers, witnesses))
    }
}

impl RpcConvertible for Vec<ContractParameter> {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut parameters = Vec::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item.as_ref().ok_or_else(|| {
                invalid_params(format!("Invalid contract parameter at index {}", index))
            })?;
            let value = jtoken_to_serde(token);
            let parameter = ContractParameter::from_json(&value).map_err(|e| {
                invalid_params(format!(
                    "Invalid contract parameter at index {}: {}",
                    index, e
                ))
            })?;
            parameters.push(parameter);
        }
        Ok(parameters)
    }
}

/// Converts a `JToken` into a `serde_json::Value` for downstream APIs that expect serde JSON.
fn jtoken_to_serde(token: &JToken) -> serde_json::Value {
    match token {
        JToken::Null => serde_json::Value::Null,
        JToken::Boolean(b) => serde_json::Value::Bool(*b),
        JToken::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        JToken::String(s) => serde_json::Value::String(s.clone()),
        JToken::Array(arr) => serde_json::Value::Array(
            arr.children()
                .iter()
                .map(|item| {
                    item.as_ref()
                        .map(jtoken_to_serde)
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
        ),
        JToken::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (key, value) in obj.iter() {
                map.insert(
                    key.clone(),
                    value
                        .as_ref()
                        .map(jtoken_to_serde)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            serde_json::Value::Object(map)
        }
    }
}
fn parse_address(text: &str, address_version: u8) -> Result<Address, RpcException> {
    let trimmed = text.trim();
    let mut result = None;
    if UInt160::try_parse(trimmed, &mut result) {
        if let Some(hash) = result {
            return Ok(Address::new(hash, address_version));
        }
    }

    match UInt160::from_address(trimmed) {
        Ok(hash) => Ok(Address::new(hash, address_version)),
        Err(_) => Err(invalid_params(format!("Invalid address: {}", trimmed))),
    }
}

fn parse_uint160(text: &str) -> Result<UInt160, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text.trim(), &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }
    Err(invalid_params(format!("Invalid UInt160 value: {}", text)))
}

fn parse_signer(token: &JToken, ctx: &ConversionContext) -> Result<Signer, RpcException> {
    let obj = expect_object(token)?;

    let account_token = obj
        .get("account")
        .ok_or_else(|| invalid_params("Signer is missing 'account' field"))?;
    let account_text = expect_string(account_token, "Signer 'account' must be a string")?;
    let account = *parse_address(&account_text, ctx.address_version)?.script_hash();

    let scopes_token = obj
        .get("scopes")
        .ok_or_else(|| invalid_params("Signer is missing 'scopes' field"))?;
    let scopes_text = expect_string(scopes_token, "Signer 'scopes' must be a string")?;
    let scopes = parse_witness_scope(&scopes_text)?;

    let mut signer = Signer {
        account,
        scopes,
        allowed_contracts: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };

    if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
        if let Some(contracts_token) = obj.get("allowedcontracts") {
            let array = expect_array(contracts_token)?;
            signer.allowed_contracts = array
                .children()
                .iter()
                .map(|item| {
                    let contract = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null contract entry"))?;
                    let text = expect_string(contract, "Allowed contract entries must be strings")?;
                    parse_uint160(&text)
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
        if let Some(groups_token) = obj.get("allowedgroups") {
            let array = expect_array(groups_token)?;
            signer.allowed_groups = array
                .children()
                .iter()
                .map(|item| {
                    let group = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null group entry"))?;
                    let text = expect_string(group, "Allowed group entries must be strings")?;
                    let bytes = hex::decode(text.trim_start_matches("0x"))
                        .map_err(|_| invalid_params("Invalid ECPoint"))?;
                    Ok(ECPoint::new(bytes))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    if scopes.contains(WitnessScope::WITNESS_RULES) {
        if let Some(rules_token) = obj.get("rules") {
            let array = expect_array(rules_token)?;
            signer.rules = array
                .children()
                .iter()
                .map(|item| {
                    let value = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null witness rule"))?;
                    let json = jtoken_to_serde(value);
                    WitnessRule::from_json(&json)
                        .map_err(|e| invalid_params(format!("Invalid witness rule: {}", e)))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    Ok(signer)
}

fn parse_witness(token: &JToken) -> Result<Witness, RpcException> {
    let obj = expect_object(token)?;
    let invocation = obj
        .get("invocation")
        .map(|t| {
            let text = expect_string(t, "Invocation script must be a string")?;
            BASE64_STANDARD
                .decode(text.trim())
                .map_err(|_| invalid_params("Invalid invocation script"))
        })
        .transpose()? // Option<Result> -> Result<Option>
        .unwrap_or_default();

    let verification = obj
        .get("verification")
        .map(|t| {
            let text = expect_string(t, "Verification script must be a string")?;
            BASE64_STANDARD
                .decode(text.trim())
                .map_err(|_| invalid_params("Invalid verification script"))
        })
        .transpose()? // Option<Result> -> Result<Option>
        .unwrap_or_default();

    Ok(Witness::new_with_scripts(invocation, verification))
}

fn parse_witness_scope(text: &str) -> Result<WitnessScope, RpcException> {
    let cleaned = text.replace(' ', "");
    if cleaned.is_empty() {
        return Ok(WitnessScope::NONE);
    }

    let mut value: u8 = 0;
    for part in cleaned.split(['|', ',']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let flag = match part {
            "None" => 0x00,
            "CalledByEntry" => WitnessScope::CALLED_BY_ENTRY.bits(),
            "CustomContracts" => WitnessScope::CUSTOM_CONTRACTS.bits(),
            "CustomGroups" => WitnessScope::CUSTOM_GROUPS.bits(),
            "WitnessRules" => WitnessScope::WITNESS_RULES.bits(),
            "Global" => WitnessScope::GLOBAL.bits(),
            other => return Err(invalid_params(format!("Unknown witness scope: {}", other))),
        };

        if flag == WitnessScope::GLOBAL.bits() && value != 0 {
            return Err(invalid_params(
                "Global scope cannot be combined with other scopes",
            ));
        }
        value |= flag;
    }

    WitnessScope::from_byte(value)
        .ok_or_else(|| invalid_params(format!("Invalid witness scope combination: {}", text)))
}

fn expect_array(token: &JToken) -> Result<&JArray, RpcException> {
    match token {
        JToken::Array(array) => Ok(array),
        _ => Err(invalid_params("Expected JSON array")),
    }
}

fn expect_object(token: &JToken) -> Result<&JObject, RpcException> {
    match token {
        JToken::Object(obj) => Ok(obj),
        _ => Err(invalid_params("Expected JSON object")),
    }
}

fn expect_string(token: &JToken, context: impl Into<String>) -> Result<String, RpcException> {
    token
        .as_string()
        .ok_or_else(|| invalid_params(context.into()))
}

fn invalid_params<T: Into<String>>(message: T) -> RpcException {
    RpcException::new(RpcError::invalid_params().with_data(message.into()))
}
