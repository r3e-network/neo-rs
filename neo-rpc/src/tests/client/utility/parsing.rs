use super::*;
use crate::types::json::{
    object_array, parse_object_array_lossy, parse_optional_present_token_array_strict,
    parse_string_array_lossy, parse_uint256_array_lossy,
};
use neo_config::ProtocolSettings;
use neo_primitives::UInt160;
use neo_serialization::json::{JArray, JObject};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;

#[test]
fn optional_string_or_null_preserves_present_and_absent_values() {
    assert_eq!(
        optional_string_or_null(Some("value")).to_string(),
        r#""value""#
    );
    assert_eq!(optional_string_or_null(None::<&str>), JToken::Null);

    let mut json = JObject::new();
    insert_optional_string(&mut json, "label", Some("main"));
    insert_optional_string(&mut json, "missing", None::<&str>);
    assert_eq!(
        json.get("label").and_then(JToken::as_string).unwrap(),
        "main"
    );
    assert_eq!(json.get("missing"), Some(&JToken::Null));
}

#[test]
fn integer_token_parsers_accept_numbers_and_strings() {
    assert_eq!(parse_u32_token(&JToken::Number(10.0), "height"), Ok(10));
    assert_eq!(
        parse_u64_token(&JToken::String("42".to_string()), "id"),
        Ok(42)
    );
    assert_eq!(
        parse_i64_token(&JToken::String("-7".to_string()), "sysfee"),
        Ok(-7)
    );
}

#[test]
fn integer_token_parsers_preserve_legacy_errors() {
    assert_eq!(
        parse_u32_token(&JToken::String("bad".to_string()), "height")
            .expect_err("invalid unsigned integer")
            .to_string(),
        "Invalid unsigned integer for 'height': invalid digit found in string"
    );
    assert_eq!(
        parse_i64_token(&JToken::String("bad".to_string()), "sysfee")
            .expect_err("invalid signed integer")
            .to_string(),
        "Invalid signed integer for 'sysfee': invalid digit found in string"
    );
    assert_eq!(
        parse_u64_token(&JToken::Null, "id")
            .expect_err("non-number token")
            .to_string(),
        "Field 'id' must be a number"
    );
}

#[test]
fn integer_token_parsers_preserve_string_encoded_large_integers() {
    // Neo serializes sysfee/netfee as decimal strings. Values above 2^53
    // must round-trip losslessly through FromStr, not be funneled through
    // f64 (which would round 9007199254740993 down to ...992).
    assert_eq!(
        parse_i64_token(&JToken::String("9007199254740993".to_string()), "sysfee"),
        Ok(9_007_199_254_740_993)
    );
    assert_eq!(
        parse_u64_token(&JToken::String("18446744073709551615".to_string()), "value"),
        Ok(u64::MAX)
    );
}

#[test]
fn integer_token_parsers_reject_invalid_json_numbers() {
    // Out-of-range JSON number must error, not saturate.
    assert!(parse_u32_token(&JToken::Number(5_000_000_000.0), "x").is_err());
    // Negative into unsigned must error.
    assert!(parse_u32_token(&JToken::Number(-5.0), "x").is_err());
    // Fractional must error.
    assert!(parse_u32_token(&JToken::Number(1.5), "x").is_err());
    // In-range integral JSON number still parses.
    assert_eq!(
        parse_u32_token(&JToken::Number(4_294_967_295.0), "x"),
        Ok(u32::MAX)
    );
}

#[test]
fn hex_prefixed_parsers_accept_uppercase_prefix() {
    assert_eq!(
        parse_nonce_token(&JToken::String("0X2a".to_string())).unwrap(),
        42
    );
    assert_eq!(
        parse_oracle_response_code(&JToken::String("0X1f".to_string())).unwrap(),
        OracleResponseCode::ContentTypeNotSupported
    );
}

#[test]
fn object_array_lossy_keeps_only_successful_objects() {
    let mut valid = JObject::new();
    valid.insert("value".to_string(), JToken::String("ok".to_string()));

    let mut invalid = JObject::new();
    invalid.insert("value".to_string(), JToken::String("skip".to_string()));

    let mut entries = JArray::new();
    entries.add(Some(JToken::Object(valid)));
    entries.add(None);
    entries.add(Some(JToken::String("not an object".to_string())));
    entries.add(Some(JToken::Object(invalid)));

    let mut root = JObject::new();
    root.insert("items".to_string(), JToken::Array(entries));

    let parsed = parse_object_array_lossy(&root, "items", |obj| {
        let value = obj.get("value").and_then(JToken::as_string).unwrap();
        if value == "ok" {
            Ok(value)
        } else {
            Err("skip".to_string())
        }
    });

    assert_eq!(parsed, vec!["ok".to_string()]);
    let missing = parse_object_array_lossy(&root, "missing", |_| Ok::<_, JsonParseError>("unused"));
    assert!(missing.is_empty());
}

#[test]
fn script_hash_or_address_parsing_accepts_hex_and_address() {
    let settings = ProtocolSettings::default_settings();
    let hash = UInt160::zero();
    let address = WalletHelper::to_address(&hash, settings.address_version);

    assert_eq!(
        parse_script_hash_or_address(&hash.to_string(), &settings).unwrap(),
        hash
    );
    assert_eq!(
        parse_script_hash_or_address(&address, &settings).unwrap(),
        hash
    );
}

#[test]
fn optional_script_hash_or_address_is_lossy() {
    let settings = ProtocolSettings::default_settings();
    let mut json = JObject::new();
    json.insert(
        "transferaddress".to_string(),
        JToken::String("not a valid address".to_string()),
    );

    assert_eq!(
        optional_script_hash_or_address_lossy(&json, "transferaddress", &settings),
        None
    );
}

#[test]
fn required_address_script_hash_preserves_parent_address_semantics() {
    let settings = ProtocolSettings::default_settings();
    let hash = UInt160::zero();
    let address = WalletHelper::to_address(&hash, settings.address_version);

    let mut base58 = JObject::new();
    base58.insert("address".to_string(), JToken::String(address));
    assert_eq!(
        required_address_script_hash(&base58, "address", &settings).unwrap(),
        hash
    );

    let mut prefixed_hex = JObject::new();
    prefixed_hex.insert("address".to_string(), JToken::String(hash.to_string()));
    assert_eq!(
        required_address_script_hash(&prefixed_hex, "address", &settings).unwrap(),
        hash
    );

    let mut uppercase_prefixed_hex = JObject::new();
    uppercase_prefixed_hex.insert(
        "address".to_string(),
        JToken::String(format!("0X{}", strip_hex_prefix(&hash.to_string()))),
    );
    assert_eq!(
        required_address_script_hash(&uppercase_prefixed_hex, "address", &settings).unwrap(),
        hash
    );

    let mut bare_hex = JObject::new();
    bare_hex.insert(
        "address".to_string(),
        JToken::String(strip_hex_prefix(&hash.to_string()).to_string()),
    );
    assert!(required_address_script_hash(&bare_hex, "address", &settings).is_err());
}

#[test]
fn object_array_preserves_item_order() {
    let values = ["first", "second"];
    let token = object_array(&values, |value| {
        let mut object = JObject::new();
        object.insert("value".to_string(), JToken::String((*value).to_string()));
        object
    });

    assert_eq!(
        token.to_string(),
        r#"[{"value":"first"},{"value":"second"}]"#
    );
}

#[test]
fn object_array_from_iter_preserves_item_order() {
    let values = ["first", "second"];
    let token = object_array_from_iter(values.into_iter().map(|value| {
        let mut object = JObject::new();
        object.insert("value".to_string(), JToken::String(value.to_string()));
        object
    }));

    assert_eq!(
        token.to_string(),
        r#"[{"value":"first"},{"value":"second"}]"#
    );
}

#[test]
fn fallible_object_array_preserves_item_order() {
    let values = ["first", "second"];
    let token = fallible_object_array(&values, |value| {
        let mut object = JObject::new();
        object.insert("value".to_string(), JToken::String((*value).to_string()));
        Ok::<_, String>(object)
    })
    .expect("fallible object array");

    assert_eq!(
        token.to_string(),
        r#"[{"value":"first"},{"value":"second"}]"#
    );
}

#[test]
fn fallible_object_array_propagates_errors() {
    let values = ["ok", "bad"];

    let err = fallible_object_array(&values, |value| {
        if *value == "bad" {
            return Err("bad value".to_string());
        }
        let mut object = JObject::new();
        object.insert("value".to_string(), JToken::String((*value).to_string()));
        Ok(object)
    })
    .expect_err("fallible object array should propagate mapper errors");

    assert_eq!(err, "bad value");
}

#[test]
fn token_array_preserves_item_order() {
    let values = ["first", "second"];
    let token = token_array(&values, |value| JToken::String((*value).to_string()));

    assert_eq!(token.to_string(), r#"["first","second"]"#);
}

#[test]
fn cloned_token_array_preserves_item_order() {
    let values = [
        JToken::String("first".to_string()),
        JToken::String("second".to_string()),
    ];
    let token = cloned_token_array(&values);

    assert_eq!(token.to_string(), r#"["first","second"]"#);
}

#[test]
fn empty_array_builds_array_token() {
    assert_eq!(empty_array().to_string(), "[]");
}

#[test]
fn string_array_lossy_keeps_only_strings() {
    let mut entries = JArray::new();
    entries.add(Some(JToken::String("first".to_string())));
    entries.add(None);
    entries.add(Some(JToken::Number(1.0)));
    entries.add(Some(JToken::String("second".to_string())));

    let mut root = JObject::new();
    root.insert("items".to_string(), JToken::Array(entries));

    assert_eq!(
        parse_string_array_lossy(&root, "items"),
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(parse_string_array_lossy(&root, "missing").is_empty());
}

#[test]
fn uint256_array_lossy_keeps_only_valid_hash_strings() {
    let mut entries = JArray::new();
    entries.add(Some(JToken::String(UInt256::zero().to_string())));
    entries.add(Some(JToken::String("not a hash".to_string())));
    entries.add(None);

    let mut root = JObject::new();
    root.insert("hashes".to_string(), JToken::Array(entries));

    assert_eq!(
        parse_uint256_array_lossy(&root, "hashes"),
        vec![UInt256::zero()]
    );
}

#[test]
fn optional_present_token_array_strict_skips_empty_slots_and_errors_present_tokens() {
    use neo_error::CoreError;
    let mut entries = JArray::new();
    entries.add(Some(JToken::Number(1.0)));
    entries.add(None);
    entries.add(Some(JToken::Number(2.0)));

    let mut root = JObject::new();
    root.insert("items".to_string(), JToken::Array(entries));

    let parsed = parse_optional_present_token_array_strict(&root, "items", |token| {
        token
            .as_number()
            .map(|value| value as u8)
            .ok_or_else(|| CoreError::other("entry must be a number"))
    })
    .expect("strict present entries");
    assert_eq!(parsed, vec![1, 2]);
    assert!(
        parse_optional_present_token_array_strict(&root, "missing", |_| {
            Ok::<_, neo_error::CoreError>(0)
        })
        .expect("missing defaults")
        .is_empty()
    );
    let mut non_array = JObject::new();
    non_array.insert("items".to_string(), JToken::Boolean(true));
    assert!(
        parse_optional_present_token_array_strict(&non_array, "items", |_| {
            Ok::<_, neo_error::CoreError>(0)
        })
        .expect("non-array defaults")
        .is_empty()
    );

    let mut invalid = JObject::new();
    invalid.insert(
        "items".to_string(),
        JToken::Array(JArray::from(vec![JToken::String("bad".to_string())])),
    );
    assert_eq!(
        parse_optional_present_token_array_strict(&invalid, "items", |token| {
            token
                .as_number()
                .map(|value| value as u8)
                .ok_or_else(|| CoreError::other("entry must be a number"))
        })
        .expect_err("present invalid token errors")
        .to_string(),
        "entry must be a number"
    );
}

#[test]
fn optional_token_array_strict_errors_on_empty_or_invalid_slots() {
    use neo_error::CoreError;
    let mut entries = JArray::new();
    entries.add(Some(JToken::Number(1.0)));
    entries.add(Some(JToken::Number(2.0)));

    let mut root = JObject::new();
    root.insert("items".to_string(), JToken::Array(entries));

    let parsed =
        parse_optional_token_array_strict(&root, "items", "entry must be a number", |token| {
            token
                .as_number()
                .map(|value| value as u8)
                .ok_or_else(|| CoreError::other("entry must be a number"))
        })
        .expect("strict tokens");
    assert_eq!(parsed, vec![1, 2]);
    assert!(
        parse_optional_token_array_strict(&root, "missing", "entry must be a number", |_| {
            Ok::<_, neo_error::CoreError>(0)
        })
        .expect("missing defaults")
        .is_empty()
    );
    let mut non_array = JObject::new();
    non_array.insert("items".to_string(), JToken::Boolean(true));
    assert!(
        parse_optional_token_array_strict(&non_array, "items", "entry must be a number", |_| {
            Ok::<_, neo_error::CoreError>(0)
        })
        .expect("non-array defaults")
        .is_empty()
    );

    let mut missing_slot = JArray::new();
    missing_slot.add(None);
    let mut invalid = JObject::new();
    invalid.insert("items".to_string(), JToken::Array(missing_slot));
    assert_eq!(
        parse_optional_token_array_strict(&invalid, "items", "entry must be a number", |_| Ok::<
            _,
            neo_error::CoreError,
        >(
            0
        ))
        .expect_err("empty slot errors")
        .to_string(),
        "entry must be a number"
    );

    invalid.insert(
        "items".to_string(),
        JToken::Array(JArray::from(vec![JToken::String("bad".to_string())])),
    );
    assert_eq!(
        parse_optional_token_array_strict(&invalid, "items", "entry must be a number", |token| {
            token
                .as_number()
                .map(|value| value as u8)
                .ok_or_else(|| CoreError::other("entry parse failed"))
        },)
        .expect_err("present invalid token errors")
        .to_string(),
        "entry parse failed"
    );
}

#[test]
fn optional_string_array_strict_errors_on_empty_or_non_string_slots() {
    let mut entries = JArray::new();
    entries.add(Some(JToken::String("first".to_string())));
    entries.add(Some(JToken::String("second".to_string())));

    let mut root = JObject::new();
    root.insert("items".to_string(), JToken::Array(entries));

    assert_eq!(
        parse_optional_string_array_strict(&root, "items", "entry must be a string")
            .expect("strict strings"),
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        parse_optional_string_array_strict(&root, "missing", "entry must be a string")
            .expect("missing defaults")
            .is_empty()
    );
    let mut non_array = JObject::new();
    non_array.insert("items".to_string(), JToken::Boolean(true));
    assert!(
        parse_optional_string_array_strict(&non_array, "items", "entry must be a string")
            .expect("non-array defaults")
            .is_empty()
    );

    let mut missing_slot = JArray::new();
    missing_slot.add(None);
    let mut invalid = JObject::new();
    invalid.insert("items".to_string(), JToken::Array(missing_slot));
    assert_eq!(
        parse_optional_string_array_strict(&invalid, "items", "entry must be a string")
            .expect_err("empty slot errors")
            .to_string(),
        "entry must be a string"
    );

    invalid.insert(
        "items".to_string(),
        JToken::Array(JArray::from(vec![JToken::Number(1.0)])),
    );
    assert_eq!(
        parse_optional_string_array_strict(&invalid, "items", "entry must be a string")
            .expect_err("non-string errors")
            .to_string(),
        "entry must be a string"
    );
}
