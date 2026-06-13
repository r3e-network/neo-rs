use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_config::ProtocolSettings;
use neo_payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
use neo_primitives::ContractParameterType;
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::{JArray, JObject, JToken};

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
    let base58 = neo_wallets::wallet_helper::to_address(&UInt160::zero(), version);
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
    let base58 = neo_wallets::wallet_helper::to_address(&UInt160::zero(), version);
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
    let base58 = neo_wallets::wallet_helper::to_address(&UInt160::zero(), version);
    let token = JToken::Array(JArray::from(vec![JToken::String(base58)]));
    let addresses = ParameterConverter::convert::<Vec<Address>>(&token, &ctx()).expect("addresses");
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
    let err = ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).unwrap_err();
    assert_invalid_params(err);
}

#[test]
fn contract_identifier_accepts_numeric() {
    let token = JToken::Number(7.0);
    let value = ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("id");
    assert!(value.is_id());
    assert_eq!(value.as_id().expect("id"), 7);
}

#[test]
fn contract_identifier_accepts_numeric_string() {
    let token = JToken::String("1".to_string());
    let value = ParameterConverter::convert::<ContractNameOrHashOrId>(&token, &ctx()).expect("id");
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
    let base58 = neo_wallets::wallet_helper::to_address(&UInt160::zero(), version);
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
    let err = ParameterConverter::convert::<Vec<ContractParameter>>(&token, &ctx()).unwrap_err();
    assert_invalid_params(err);
}
