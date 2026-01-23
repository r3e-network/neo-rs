use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_core::cryptography::{ECCurve, ECPoint};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::contract_parameter::ContractParameter;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;
use neo_core::{WitnessRule, WitnessScope};
use neo_json::{JArray, JObject, JToken, MAX_SAFE_INTEGER};
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
                    let value = numeric_from_token(token)?;
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
                        let max_safe = MAX_SAFE_INTEGER as f64;
                        if value < -max_safe || value > max_safe {
                            return Err(invalid_params(format!(
                                "Numeric value out of safe range for {}: {}",
                                stringify!($ty),
                                value
                            )));
                        }
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
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < 0.0 || rounded > u32::MAX as f64 {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                Ok(BlockHashOrIndex::from_index(rounded as u32))
            }
            JToken::String(text) => BlockHashOrIndex::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid block hash or index: {}", text))),
            _ => Err(invalid_params("Expected block hash or index string")),
        }
    }
}

impl RpcConvertible for ContractNameOrHashOrId {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < i32::MIN as f64 || rounded > i32::MAX as f64 {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                Ok(ContractNameOrHashOrId::from_id(rounded as i32))
            }
            JToken::String(text) => ContractNameOrHashOrId::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid contract identifier: {}", text))),
            _ => Err(invalid_params("Expected contract identifier string")),
        }
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

            let signer_token = obj.get("signer").unwrap_or(token);
            let signer = parse_signer(signer_token, ctx)?;
            signers.push(signer);

            if let Some(witness_token) = obj.get("witness") {
                if !matches!(witness_token, JToken::Null) {
                    let witness = parse_witness(witness_token)?;
                    witnesses.push(witness);
                }
            } else if obj.contains_property("invocation") || obj.contains_property("verification") {
                let witness = parse_witness(token)?;
                witnesses.push(witness);
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
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(hash) = result {
            return Ok(Address::new(hash, address_version));
        }
    }

    WalletHelper::to_script_hash(text, address_version)
        .map(|hash| Address::new(hash, address_version))
        .map_err(|_| invalid_params(format!("Invalid address: {}", text)))
}

fn parse_uint160(text: &str) -> Result<UInt160, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
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
                    // Neo N3 uses secp256r1 (NIST P-256) curve for public keys
                    ECPoint::new(ECCurve::Secp256r1, bytes)
                        .map_err(|e| invalid_params(format!("Invalid ECPoint: {}", e)))
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

fn numeric_from_token(token: &JToken) -> Result<f64, RpcException> {
    match token {
        JToken::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(0.0);
            }
            trimmed
                .parse::<f64>()
                .map_err(|_| invalid_params("Expected numeric value"))
        }
        _ => token
            .as_number()
            .ok_or_else(|| invalid_params("Expected numeric value")),
    }
}

fn invalid_params<T: Into<String>>(message: T) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use neo_core::network::p2p::payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::smart_contract::ContractParameterType;
    use neo_core::wallets::helper::Helper as WalletHelper;
    use neo_core::{UInt160, UInt256};
    use neo_json::{JArray, JObject, JToken};

    fn ctx() -> ConversionContext {
        ConversionContext::new(ProtocolSettings::default().address_version)
    }

    fn assert_invalid_params(err: RpcException) {
        assert_eq!(err.code(), RpcError::invalid_params().code());
    }

    fn signer_entry(account: &str, scopes: &str, extra: Option<(&str, JToken)>) -> JToken {
        let mut signer = JObject::new();
        signer.insert("account".to_string(), JToken::String(account.to_string()));
        signer.insert("scopes".to_string(), JToken::String(scopes.to_string()));
        if let Some((key, value)) = extra {
            signer.insert(key.to_string(), value);
        }

        let mut entry = JObject::new();
        entry.insert("signer".to_string(), JToken::Object(signer));
        JToken::Object(entry)
    }

    #[test]
    fn numeric_conversion_rejects_fractional_for_integers() {
        let token = JToken::Number(1.5);
        let err = ParameterConverter::convert::<u32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_fractional_edges() {
        let token = JToken::Number(0.9999999999999);
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::Number(-0.0000000000001);
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_out_of_range() {
        let token = JToken::Number(256.0);
        let err = ParameterConverter::convert::<u8>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_negative_for_unsigned() {
        let token = JToken::Number(-1.0);
        assert_invalid_params(ParameterConverter::convert::<u8>(&token, &ctx()).unwrap_err());
        assert_invalid_params(ParameterConverter::convert::<u16>(&token, &ctx()).unwrap_err());
        assert_invalid_params(ParameterConverter::convert::<u32>(&token, &ctx()).unwrap_err());
        assert_invalid_params(ParameterConverter::convert::<u64>(&token, &ctx()).unwrap_err());

        let token = JToken::String("-1".to_string());
        assert_invalid_params(ParameterConverter::convert::<u32>(&token, &ctx()).unwrap_err());
    }

    #[test]
    fn numeric_conversion_rejects_nan_and_infinity() {
        let token = JToken::Number(f64::NAN);
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::Number(f64::INFINITY);
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_accepts_string_and_whitespace() {
        let token = JToken::String("42".to_string());
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 42);

        let token = JToken::String(" 42.0 ".to_string());
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 42);
    }

    #[test]
    fn numeric_conversion_accepts_empty_string_as_zero() {
        let token = JToken::String(String::new());
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 0);
    }

    #[test]
    fn numeric_conversion_rejects_null() {
        let token = JToken::Null;
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_hex_string() {
        let token = JToken::String("0xFF".to_string());
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_unsafe_integer_range() {
        let token = JToken::String(format!("{}", MAX_SAFE_INTEGER + 1));
        let err = ParameterConverter::convert::<i64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::String(format!("{}", MAX_SAFE_INTEGER + 1));
        let err = ParameterConverter::convert::<u64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_unsafe_integer_numeric() {
        let token = JToken::Number((MAX_SAFE_INTEGER as f64) + 1.0);
        let err = ParameterConverter::convert::<i64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::Number((MAX_SAFE_INTEGER as f64) + 1.0);
        let err = ParameterConverter::convert::<u64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_accepts_scientific_notation() {
        let token = JToken::String("1e6".to_string());
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 1_000_000);

        let token = JToken::String("1.5e2".to_string());
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 150);
    }

    #[test]
    fn numeric_conversion_accepts_boolean_tokens() {
        let token = JToken::Boolean(true);
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 1);

        let token = JToken::Boolean(false);
        let value = ParameterConverter::convert::<i32>(&token, &ctx()).expect("numeric");
        assert_eq!(value, 0);
    }

    #[test]
    fn boolean_conversion_accepts_numeric_tokens() {
        let token = JToken::Number(1.0);
        let value = ParameterConverter::convert::<bool>(&token, &ctx()).expect("bool");
        assert!(value);

        let token = JToken::Number(0.0);
        let value = ParameterConverter::convert::<bool>(&token, &ctx()).expect("bool");
        assert!(!value);
    }

    #[test]
    fn numeric_conversion_rejects_unsafe_long_string() {
        let token = JToken::String(i64::MIN.to_string());
        let err = ParameterConverter::convert::<i64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_large_double_values() {
        let token = JToken::Number(f64::MAX);
        let err = ParameterConverter::convert::<i64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::Number(f64::MIN);
        let err = ParameterConverter::convert::<i64>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn numeric_conversion_rejects_unicode_digits() {
        let token = JToken::String("１２３４".to_string());
        let err = ParameterConverter::convert::<i32>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn bytes_conversion_rejects_invalid_base64() {
        let token = JToken::String("not-base64".to_string());
        let err = ParameterConverter::convert::<Vec<u8>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn bytes_conversion_rejects_unicode_input() {
        let token = JToken::String("😊".to_string());
        let err = ParameterConverter::convert::<Vec<u8>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn bytes_conversion_accepts_base64() {
        let token = JToken::String("AQID".to_string());
        let bytes = ParameterConverter::convert::<Vec<u8>>(&token, &ctx()).expect("bytes");
        assert_eq!(bytes, vec![1, 2, 3]);
    }

    #[test]
    fn address_conversion_accepts_uint160_string() {
        let token = JToken::String(UInt160::zero().to_string());
        let address = ParameterConverter::convert::<Address>(&token, &ctx()).expect("address");
        assert_eq!(address.script_hash(), &UInt160::zero());
    }

    #[test]
    fn address_conversion_accepts_base58() {
        let version = ctx().address_version;
        let base58 = WalletHelper::to_address(&UInt160::zero(), version);
        let token = JToken::String(base58);
        let address = ParameterConverter::convert::<Address>(&token, &ctx()).expect("address");
        assert_eq!(address.script_hash(), &UInt160::zero());
    }

    #[test]
    fn address_conversion_rejects_invalid_address() {
        let token = JToken::String("invalid-address".to_string());
        let err = ParameterConverter::convert::<Address>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn address_conversion_rejects_whitespace_wrapped_address() {
        let version = ctx().address_version;
        let base58 = WalletHelper::to_address(&UInt160::zero(), version);
        let token = JToken::String(format!(" {base58} "));
        let err = ParameterConverter::convert::<Address>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn address_array_rejects_null_entry() {
        let mut array = JArray::new();
        array.add(None);
        let token = JToken::Array(array);
        let err = ParameterConverter::convert::<Vec<Address>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn address_array_rejects_invalid_address() {
        let mut array = JArray::new();
        array.add(Some(JToken::String("invalid-address".to_string())));
        let token = JToken::Array(array);
        let err = ParameterConverter::convert::<Vec<Address>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn address_array_rejects_non_array_token() {
        let token = JToken::Object(JObject::new());
        let err = ParameterConverter::convert::<Vec<Address>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn address_array_accepts_base58() {
        let version = ctx().address_version;
        let base58 = WalletHelper::to_address(&UInt160::zero(), version);
        let token = JToken::Array(JArray::from(vec![JToken::String(base58)]));
        let addresses =
            ParameterConverter::convert::<Vec<Address>>(&token, &ctx()).expect("addresses");
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0].script_hash(), &UInt160::zero());
    }

    #[test]
    fn block_hash_or_index_rejects_invalid_value() {
        let token = JToken::String("not-a-block".to_string());
        let err = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn block_hash_or_index_accepts_numeric() {
        let token = JToken::Number(1.0);
        let value = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).expect("hash");
        assert!(value.is_index());
        assert_eq!(value.as_index().expect("index"), 1);
    }

    #[test]
    fn block_hash_or_index_accepts_numeric_string() {
        let token = JToken::String("1".to_string());
        let value = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).expect("hash");
        assert!(value.is_index());
        assert_eq!(value.as_index().expect("index"), 1);
    }

    #[test]
    fn block_hash_or_index_accepts_hash_string() {
        let hash_text = "0x761a9bb72ca2a63984db0cc43f943a2a25e464f62d1a91114c2b6fbbfd24b51d";
        let token = JToken::String(hash_text.to_string());
        let value = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).expect("hash");
        assert!(!value.is_index());
        assert_eq!(
            value.as_hash().expect("hash"),
            UInt256::parse(hash_text).expect("parse hash")
        );

        let token = JToken::String(hash_text.trim_start_matches("0x").to_string());
        let value = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).expect("hash");
        assert_eq!(
            value.as_hash().expect("hash"),
            UInt256::parse(hash_text).expect("parse hash")
        );
    }

    #[test]
    fn block_hash_or_index_rejects_negative() {
        let token = JToken::Number(-1.0);
        let err = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);

        let token = JToken::String("-1".to_string());
        let err = ParameterConverter::convert::<BlockHashOrIndex>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn contract_identifier_rejects_empty_string() {
        let token = JToken::String(String::new());
        let err =
            ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn contract_identifier_accepts_numeric() {
        let token = JToken::Number(7.0);
        let value =
            ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("id");
        assert!(value.is_id());
        assert_eq!(value.as_id().expect("id"), 7);
    }

    #[test]
    fn contract_identifier_accepts_numeric_string() {
        let token = JToken::String("1".to_string());
        let value =
            ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("id");
        assert!(value.is_id());
        assert_eq!(value.as_id().expect("id"), 1);
    }

    #[test]
    fn contract_identifier_accepts_hash_string() {
        let hash_text = "0x1234567890abcdef1234567890abcdef12345678";
        let token = JToken::String(hash_text.to_string());
        let value =
            ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("hash");
        assert!(value.is_hash());
        assert_eq!(
            value.as_hash().expect("hash"),
            UInt160::parse(hash_text).expect("parse hash")
        );
    }

    #[test]
    fn contract_identifier_treats_short_hash_as_name() {
        let token = JToken::String("0xabc".to_string());
        let value =
            ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("name");
        assert!(value.is_name());
        let err = value.as_hash().expect_err("not a hash");
        assert_eq!(err.code(), RpcError::invalid_params().code());
    }

    #[test]
    fn uuid_conversion_rejects_invalid_string() {
        let token = JToken::String("not-a-uuid".to_string());
        let err = ParameterConverter::convert::<Uuid>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn uuid_conversion_accepts_valid_string() {
        let value = Uuid::new_v4();
        let token = JToken::String(value.to_string());
        let parsed = ParameterConverter::convert::<Uuid>(&token, &ctx()).expect("uuid");
        assert_eq!(parsed, value);
    }

    #[test]
    fn signers_reject_invalid_scope_combination() {
        let account = UInt160::zero().to_string();
        let entry = signer_entry(&account, "Global|CustomContracts", None);
        let token = JToken::Array(JArray::from(vec![entry]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_allowed_group() {
        let mut groups = JArray::new();
        groups.add(Some(JToken::String("zz".to_string())));
        let entry = signer_entry(
            &UInt160::zero().to_string(),
            "CustomGroups",
            Some(("allowedgroups", JToken::Array(groups))),
        );
        let token = JToken::Array(JArray::from(vec![entry]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_allowed_groups_type() {
        let entry = signer_entry(
            &UInt160::zero().to_string(),
            "CustomGroups",
            Some(("allowedgroups", JToken::String("invalid".to_string()))),
        );
        let token = JToken::Array(JArray::from(vec![entry]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_allowed_contract() {
        let mut contracts = JArray::new();
        contracts.add(Some(JToken::String("invalid".to_string())));
        let entry = signer_entry(
            &UInt160::zero().to_string(),
            "CustomContracts",
            Some(("allowedcontracts", JToken::Array(contracts))),
        );
        let token = JToken::Array(JArray::from(vec![entry]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_rules() {
        let mut signer = JObject::new();
        signer.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        signer.insert(
            "scopes".to_string(),
            JToken::String("WitnessRules".to_string()),
        );
        signer.insert("rules".to_string(), JToken::String("invalid".to_string()));
        let mut entry = JObject::new();
        entry.insert("signer".to_string(), JToken::Object(signer));
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_null_entry() {
        let token = JToken::Array(JArray::from(vec![JToken::Null]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_too_many_entries() {
        let entry = signer_entry(&UInt160::zero().to_string(), "CalledByEntry", None);
        let entries = vec![entry; MAX_TRANSACTION_ATTRIBUTES + 1];
        let token = JToken::Array(JArray::from(entries));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_missing_account_field() {
        let mut entry = JObject::new();
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_missing_scopes_field() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_scope_value() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("InvalidScopeValue".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_allowed_contracts_type() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("CustomContracts".to_string()),
        );
        entry.insert(
            "allowedcontracts".to_string(),
            JToken::String("invalid".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_accept_flat_entry() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let parsed =
            ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).expect("signers");
        assert_eq!(parsed.signers().len(), 1);
        assert!(parsed.witnesses().is_empty());
    }

    #[test]
    fn signers_accept_base58_account() {
        let version = ctx().address_version;
        let base58 = WalletHelper::to_address(&UInt160::zero(), version);
        let mut entry = JObject::new();
        entry.insert("account".to_string(), JToken::String(base58));
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let parsed =
            ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).expect("signers");
        assert_eq!(parsed.signers().len(), 1);
        assert_eq!(parsed.signers()[0].account, UInt160::zero());
    }

    #[test]
    fn signers_accept_flat_entry_with_witness() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        entry.insert(
            "invocation".to_string(),
            JToken::String("SGVsbG8K".to_string()),
        );
        entry.insert(
            "verification".to_string(),
            JToken::String("V29ybGQK".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let parsed =
            ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).expect("signers");
        assert_eq!(parsed.signers().len(), 1);
        assert_eq!(parsed.witnesses().len(), 1);
        let witness = &parsed.witnesses()[0];
        assert_eq!(
            witness.invocation_script,
            BASE64_STANDARD
                .decode("SGVsbG8K")
                .expect("decode invocation")
        );
        assert_eq!(
            witness.verification_script,
            BASE64_STANDARD
                .decode("V29ybGQK")
                .expect("decode verification")
        );
    }

    #[test]
    fn signers_reject_invalid_witness_invocation() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        entry.insert(
            "invocation".to_string(),
            JToken::String("not-base64".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn signers_reject_invalid_witness_verification() {
        let mut entry = JObject::new();
        entry.insert(
            "account".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        entry.insert(
            "scopes".to_string(),
            JToken::String("CalledByEntry".to_string()),
        );
        entry.insert(
            "verification".to_string(),
            JToken::String("not-base64".to_string()),
        );
        let token = JToken::Array(JArray::from(vec![JToken::Object(entry)]));
        let err = ParameterConverter::convert::<SignersAndWitnesses>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }

    #[test]
    fn contract_parameters_accept_valid_array() {
        let mut obj = JObject::new();
        obj.insert("value".to_string(), JToken::String("test".to_string()));
        obj.insert("type".to_string(), JToken::String("String".to_string()));
        let token = JToken::Array(JArray::from(vec![JToken::Object(obj)]));

        let params =
            ParameterConverter::convert::<Vec<ContractParameter>>(&token, &ctx()).expect("params");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, ContractParameterType::String);
    }

    #[test]
    fn contract_parameters_reject_null_entry() {
        let token = JToken::Array(JArray::from(vec![JToken::Null]));
        let err =
            ParameterConverter::convert::<Vec<ContractParameter>>(&token, &ctx()).unwrap_err();
        assert_invalid_params(err);
    }
}
