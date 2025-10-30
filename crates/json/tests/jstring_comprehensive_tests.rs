use neo_json::JString;

#[test]
fn test_constructor_and_accessors() {
    let value = "hello world";
    let jstring = JString::from(value);
    assert_eq!(jstring.value, value);
    assert_eq!(jstring.as_string(), value.to_string());
    assert!(jstring.as_boolean());
}

#[test]
fn test_as_number_parsing() {
    let numeric = JString::from("123.5");
    assert_eq!(numeric.as_number(), 123.5);

    let not_numeric = JString::from("neo");
    assert!(not_numeric.as_number().is_nan());

    let empty = JString::from("");
    assert_eq!(empty.as_number(), 0.0);
}

#[test]
fn test_display_and_write() {
    let value = "line1\nline2\t";
    let jstring = JString::from(value);

    // Display returns unescaped content
    assert_eq!(format!("{}", jstring), value);

    // Writer returns JSON-escaped content
    let mut buffer = Vec::new();
    jstring.write(&mut buffer).expect("write should succeed");
    assert_eq!(String::from_utf8(buffer).unwrap(), "\"line1\\nline2\\t\"");
}
