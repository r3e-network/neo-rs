use super::*;
use neo_serialization::json::{JArray, JObject, JToken};

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

#[test]
fn object_field_helpers_preserve_field_errors() {
    let err = parse_object_field(JToken::Null, "ctx", "field", "missing field", |_| Ok(()))
        .expect_err("non-object should fail");
    assert_eq!(err.message(), "ctx: expected object token");

    let err = parse_object_field(
        JToken::Object(JObject::new()),
        "ctx",
        "field",
        "missing field",
        |_| Ok(()),
    )
    .expect_err("missing field should fail");
    assert_eq!(err.message(), "missing field");
}

#[test]
fn scalar_field_parsers_preserve_rpc_errors() {
    let fee = parse_i64_number_or_string_token(
        &JToken::String("42".into()),
        "networkfee",
        "Invalid networkfee token type",
    )
    .expect("string fee");
    assert_eq!(fee, 42);

    let err = parse_i64_number_or_string_token(
        &JToken::Boolean(true),
        "networkfee",
        "Invalid networkfee token type",
    )
    .expect_err("boolean fee should fail");
    assert_eq!(err.message(), "Invalid networkfee token type");

    let hash = UInt256::zero().to_string();
    let parsed = parse_uint256_string_token(
        &JToken::String(hash),
        "Missing hash in submitblock",
        "Invalid block hash",
    )
    .expect("hash");
    assert_eq!(parsed, UInt256::zero());

    let err = parse_uint256_string_token(
        &JToken::Number(1.0),
        "Missing hash in submitblock",
        "Invalid block hash",
    )
    .expect_err("non-string hash should fail");
    assert_eq!(err.message(), "Missing hash in submitblock");
}

#[test]
fn object_array_result_preserves_supplied_error_messages() {
    let err = parse_object_array_result(
        &JToken::Null,
        "not array",
        "null entry",
        "not object",
        |_| Ok(()),
    )
    .expect_err("non-array should fail");
    assert_eq!(err.message(), "not array");

    let err = parse_object_array_result(
        &JToken::Array(JArray::from(vec![None])),
        "not array",
        "null entry",
        "not object",
        |_| Ok(()),
    )
    .expect_err("null entry should fail");
    assert_eq!(err.message(), "null entry");

    let err = parse_object_array_result(
        &JToken::Array(JArray::from(vec![JToken::String("x".into())])),
        "not array",
        "null entry",
        "not object",
        |_| Ok(()),
    )
    .expect_err("non-object entry should fail");
    assert_eq!(err.message(), "not object");
}
