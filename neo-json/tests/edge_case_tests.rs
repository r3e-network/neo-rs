use neo_json::{JToken, JsonError, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER};

#[test]
fn duplicate_properties_and_trailing_content_are_rejected() {
    let duplicate = JToken::parse(r#"{"a":1,"a":2}"#, 16).unwrap_err();
    assert!(duplicate.to_string().contains("Duplicate property name: a"));

    let trailing = JToken::parse("true false", 16).unwrap_err();
    assert!(matches!(trailing, JsonError::Serde(_)));
}

#[test]
fn nesting_limit_is_enforced_at_the_exact_boundary() {
    assert!(JToken::parse("0", 1).is_ok());
    assert!(JToken::parse("0", 0).is_err());
    assert!(JToken::parse("[0]", 2).is_ok());
    let error = JToken::parse("[0]", 1).unwrap_err();
    assert!(error.to_string().contains("Exceeded max depth"));
    assert!(JToken::parse("[[0]]", 3).is_ok());
    assert!(JToken::parse("[[0]]", 2).is_err());
}

#[test]
fn safe_integer_boundaries_round_trip_without_precision_loss() {
    for value in [MIN_SAFE_INTEGER, MAX_SAFE_INTEGER] {
        let token = JToken::parse(&value.to_string(), 16).expect("safe integer");
        assert_eq!(token.try_as_number().unwrap(), value as f64);
        assert_eq!(token.to_string(), value.to_string());
        assert_eq!(
            JToken::parse(&token.to_string(), 16).expect("round trip"),
            token
        );
    }
}

#[test]
fn int32_conversion_checks_type_integrality_and_range() {
    assert_eq!(
        JToken::Number(f64::from(i32::MIN)).try_as_int32().unwrap(),
        i32::MIN
    );
    assert_eq!(
        JToken::Number(f64::from(i32::MAX)).try_as_int32().unwrap(),
        i32::MAX
    );
    assert!(matches!(
        JToken::Number(f64::from(i32::MAX) + 1.0).try_as_int32(),
        Err(JsonError::Overflow(_))
    ));
    assert!(matches!(
        JToken::Number(1.5).try_as_int32(),
        Err(JsonError::InvalidCast(_))
    ));
    assert!(matches!(
        JToken::String("1".into()).try_as_int32(),
        Err(JsonError::InvalidCast(_))
    ));
}

#[test]
fn unicode_escapes_and_controls_round_trip() {
    let token = JToken::parse(r#""\uD83D\uDE00\n\t""#, 16).expect("unicode");
    assert_eq!(token.as_string().as_deref(), Some("😀\n\t"));
    let encoded = token.to_string();
    assert!(encoded.contains("\\n") && encoded.contains("\\t"));
    assert_eq!(JToken::parse(&encoded, 16).unwrap(), token);
    assert!(JToken::parse(r#""\uD83D""#, 16).is_err());
}

#[test]
fn explicit_null_and_absent_optional_entries_both_serialize_as_null() {
    let parsed = JToken::parse("[null]", 16).expect("null array");
    assert_eq!(parsed.get_index(0).unwrap(), Some(&JToken::Null));

    let optional = JToken::from_array(vec![None, Some(JToken::Null)]);
    assert_eq!(optional.to_string(), "[null,null]");
}

#[test]
fn object_property_order_is_stable_across_serialization() {
    let token = JToken::parse(r#"{"b":1,"a":2}"#, 16).expect("object");
    assert_eq!(token.to_string(), r#"{"b":1,"a":2}"#);
    assert_eq!(JToken::parse(&token.to_string(), 16).unwrap(), token);
}

#[test]
fn writer_and_byte_serialization_match_display_output() {
    let token = JToken::parse(r#"{"items":[1,true,null],"text":"neo"}"#, 16).unwrap();
    let bytes = token.to_byte_array(false).unwrap();
    let mut writer = Vec::new();
    token.write(&mut writer, false).unwrap();
    assert_eq!(writer, bytes);
    assert_eq!(String::from_utf8(bytes).unwrap(), token.to_string());

    let pretty = token.to_string_formatted(true).unwrap();
    assert!(pretty.contains('\n'));
    assert_eq!(JToken::parse(&pretty, 16).unwrap(), token);
}

#[test]
fn scalar_accessors_do_not_silently_accept_unrelated_container_types() {
    assert_eq!(JToken::Boolean(true).as_number(), Some(1.0));
    assert_eq!(JToken::String("2.5".into()).as_number(), Some(2.5));
    assert_eq!(JToken::String("not-a-number".into()).as_number(), None);
    assert!(matches!(
        JToken::Null.get_index(0),
        Err(JsonError::NotSupported(_))
    ));
    assert!(matches!(
        JToken::from_array(vec![]).get_property("key"),
        Err(JsonError::NotSupported(_))
    ));
}

#[test]
fn empty_json_path_returns_the_original_token() {
    let token = JToken::parse(r#"{"a":[1,2]}"#, 16).expect("object");
    let result = token.json_path("").expect("identity path");
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0), Some(&token));
}
