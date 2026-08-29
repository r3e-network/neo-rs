//! Comprehensive JString (JToken::String variant) compatibility tests.

use neo_json::JToken;

#[test]
fn test_string_construction_and_equality() {
    assert_eq!(JToken::from("neo"), JToken::String("neo".to_string()));
    assert_eq!(JToken::from("neo".to_string()), JToken::String("neo".to_string()));
    assert_ne!(JToken::String("neo".to_string()), JToken::String("n3".to_string()));
    assert_ne!(JToken::String(String::new()), JToken::Null);
}

#[test]
fn test_string_accessors() {
    let token = JToken::String("hello".to_string());

    assert_eq!(token.as_string(), Some("hello".to_string()));
    assert_eq!(token.to_string_value(), "hello");
    assert_eq!(token.try_as_string().unwrap(), "hello");
    assert!(token.as_boolean());
    assert_eq!(token.as_number(), None);
}

#[test]
fn test_empty_string_is_falsy() {
    let token = JToken::String(String::new());
    assert!(!token.as_boolean());
    assert_eq!(token.to_string_value(), "");
}

#[test]
fn test_non_string_accessors_fail() {
    assert_eq!(JToken::Null.as_string(), None);
    assert!(JToken::Null.try_as_string().is_err());
    assert!(JToken::Boolean(true).try_as_string().is_err());
    assert!(JToken::Number(1.5).try_as_string().is_err());
}

#[test]
fn test_number_string_parses_as_number() {
    let token = JToken::String("42".to_string());
    assert_eq!(token.as_number(), Some(42.0));
    // Strict accessors only accept genuine Number tokens; strings are not coerced.
    assert!(token.try_as_int32().is_err());
    assert!(token.try_as_number().is_err());
}

#[test]
fn test_non_numeric_string_is_not_a_number() {
    assert_eq!(JToken::String("abc".to_string()).as_number(), None);
    assert_eq!(JToken::String(String::new()).as_number(), None);
}

#[test]
fn test_parse_string_escapes() {
    let token = JToken::parse(r#""a\"b\\c\/d\n\t\r""#, 32).unwrap();
    assert_eq!(
        token,
        JToken::String("a\"b\\c/d\n\t\r".to_string()),
        "standard JSON escape sequences"
    );
}

#[test]
fn test_parse_unicode_escapes() {
    let token = JToken::parse(r#""Néo\u4e2d""#, 32).unwrap();
    assert_eq!(token, JToken::String("Néo中".to_string()));
}

#[test]
fn test_parse_invalid_string_fails() {
    // Unterminated string literal.
    assert!(JToken::parse(r#""unterminated"#, 32).is_err());
    // Control character inside string.
    assert!(JToken::parse("\"line\nbreak\"", 32).is_err());
    // Invalid escape sequence.
    assert!(JToken::parse(r#""bad\escape""#, 32).is_err());
}

#[test]
fn test_string_serialization_round_trip() {
    let value = "quote\" backslash\\ newline\n tab\t unicode中";
    let token = JToken::String(value.to_string());

    let serialized = token.to_string_formatted(false).unwrap();
    let reparsed = JToken::parse(&serialized, 32).unwrap();
    assert_eq!(reparsed, token);
}

#[test]
fn test_string_within_containers() {
    let array = JToken::from_array(vec![
        Some(JToken::String("first".to_string())),
        Some(JToken::String(String::new())),
        None,
    ]);

    assert_eq!(
        array.get_index(0).unwrap(),
        Some(&JToken::String("first".to_string()))
    );
    assert_eq!(array.get_index(1).unwrap(), Some(&JToken::String(String::new())));
    assert_eq!(array.get_index(2).unwrap(), None);
}

#[test]
fn test_string_property_lookup() {
    let mut object = neo_json::OrderedDictionary::new();
    object.insert("name".to_string(), Some(JToken::String("neo".to_string())));
    let token = JToken::from_object(object);

    assert_eq!(
        token.get_property("name").unwrap(),
        Some(&JToken::String("neo".to_string()))
    );
    assert_eq!(token.get_property("missing").unwrap(), None);
}
