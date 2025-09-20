//! Comprehensive JsonSerializer Tests
//! Tests for JSON parsing and serialization functionality

use neo_json::JToken;

/// Test JsonTest_WrongJson functionality
#[test]
fn test_json_wrong_json() {
    // Test various malformed JSON strings
    let malformed_json_cases = vec![
        "{",                        // Unclosed object
        "}",                        // Unexpected closing brace
        "[",                        // Unclosed array
        "]",                        // Unexpected closing bracket
        "{'key': 'value'}",         // Single quotes (invalid JSON)
        "{key: 'value'}",           // Unquoted key
        "{\"key\": value}",         // Unquoted value
        "{\"key\": 'value'}",       // Mixed quotes
        "{\"key\": undefined}",     // Undefined value
        "{\"key\": }",              // Missing value
        "{\"key\" 'value'}",        // Missing colon
        "{\"key\": \"value\",}",    // Trailing comma in object
        "[1, 2, 3,]",               // Trailing comma in array
        "\"unterminated string",    // Unterminated string
        "{\"nested\": {\"key\": }", // Nested malformed object
        "null,",                    // Trailing comma after value
        "{\"\\u005\"}",             // Invalid Unicode escape
        "{\"key\": \"\\z\"}",       // Invalid escape sequence
    ];

    for malformed_json in malformed_json_cases {
        let result = JToken::parse_string(malformed_json, 64);
        assert!(
            result.is_err(),
            "Expected error for malformed JSON: {}",
            malformed_json
        );
    }
}

/// Test JsonTest_Array functionality
#[test]
fn test_json_array() {
    // Test valid array parsing
    let test_cases = vec![
        ("[]", 0),                       // Empty array
        ("[1]", 1),                      // Single element
        ("[1, 2, 3]", 3),                // Multiple elements
        ("[true, false]", 2),            // Boolean elements
        ("[\"hello\", \"world\"]", 2),   // String elements
        ("[null]", 1),                   // Null element
        ("[1, \"two\", true, null]", 4), // Mixed types
        ("[[1, 2], [3, 4]]", 2),         // Nested arrays
        ("[{\"key\": \"value\"}]", 1),   // Object in array
    ];

    for (json_str, expected_length) in test_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse valid JSON array: {}",
            json_str
        );

        if let Ok(Some(JToken::Array(arr))) = result {
            assert_eq!(
                arr.len(),
                expected_length,
                "Array length mismatch for: {}",
                json_str
            );
        } else {
            panic!("Expected array token for: {}", json_str);
        }
    }
}

/// Test JsonTest_Bool functionality
#[test]
fn test_json_bool() {
    // Test boolean parsing
    let test_cases = vec![("true", true), ("false", false)];

    for (json_str, expected_value) in test_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse valid JSON boolean: {}",
            json_str
        );

        if let Ok(Some(JToken::Boolean(value))) = result {
            assert_eq!(
                value, expected_value,
                "Boolean value mismatch for: {}",
                json_str
            );
        } else {
            panic!("Expected boolean token for: {}", json_str);
        }
    }

    // Test invalid boolean cases
    let invalid_cases = vec!["True", "False", "TRUE", "FALSE", "0", "1"];
    for invalid_json in invalid_cases {
        let result = JToken::parse_string(invalid_json, 64);
        // These should either parse as different types or fail
        if let Ok(Some(token)) = result {
            assert!(
                !matches!(token, JToken::Boolean(_)),
                "Should not parse as boolean: {}",
                invalid_json
            );
        }
    }
}

/// Test JsonTest_Numbers functionality
#[test]
fn test_json_numbers() {
    // Test number parsing
    let test_cases = vec![
        ("0", 0.0),
        ("1", 1.0),
        ("-1", -1.0),
        ("123", 123.0),
        ("-123", -123.0),
        ("1.5", 1.5),
        ("-1.5", -1.5),
        ("0.123456789", 0.123456789),
        ("123.456", 123.456),
        ("1e10", 1e10),
        ("1E10", 1E10),
        ("1e-10", 1e-10),
        ("1.23e-10", 1.23e-10),
        ("1.23E10", 1.23E10),
    ];

    for (json_str, expected_value) in test_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse valid JSON number: {}",
            json_str
        );

        if let Ok(Some(JToken::Number(value))) = result {
            assert!(
                (value - expected_value).abs() < f64::EPSILON,
                "Number value mismatch for: {} (got {}, expected {})",
                json_str,
                value,
                expected_value
            );
        } else {
            panic!("Expected number token for: {}", json_str);
        }
    }

    // Test invalid number cases
    let invalid_cases = vec!["01", "1.", ".1", "1.2.3", "1e", "1e+", "++1", "--1"];
    for invalid_json in invalid_cases {
        let result = JToken::parse_string(invalid_json, 64);
        // These should fail to parse
        assert!(
            result.is_err() || !matches!(result.as_ref().unwrap(), Some(JToken::Number(_))),
            "Should not parse as valid number: {}",
            invalid_json
        );
    }
}

/// Test JsonTest_String functionality
#[test]
fn test_json_string() {
    // Test string parsing
    let test_cases = vec![
        ("\"\"", ""),                                         // Empty string
        ("\"hello\"", "hello"),                               // Simple string
        ("\"Hello World\"", "Hello World"),                   // String with space
        ("\"Hello\\nWorld\"", "Hello\nWorld"),                // String with newline escape
        ("\"Hello\\tWorld\"", "Hello\tWorld"),                // String with tab escape
        ("\"Hello\\\\World\"", "Hello\\World"),               // String with backslash escape
        ("\"Hello\\\"World\"", "Hello\"World"),               // String with quote escape
        ("\"Hello\\/World\"", "Hello/World"),                 // String with forward slash escape
        ("\"Hello\\bWorld\"", "Hello\x08World"),              // String with backspace escape
        ("\"Hello\\fWorld\"", "Hello\x0CWorld"),              // String with form feed escape
        ("\"Hello\\rWorld\"", "Hello\rWorld"),                // String with carriage return escape
        ("\"\\u0048\\u0065\\u006C\\u006C\\u006F\"", "Hello"), // Unicode escape sequences
        ("\"😀😃😄\"", "😀😃😄"),                             // Emoji characters
        ("\"Multi\\nLine\\nText\"", "Multi\nLine\nText"),     // Multi-line text with escapes
    ];

    for (json_str, expected_value) in test_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse valid JSON string: {}",
            json_str
        );

        if let Ok(Some(JToken::String(value))) = result {
            assert_eq!(
                value, expected_value,
                "String value mismatch for: {}",
                json_str
            );
        } else {
            panic!("Expected string token for: {}", json_str);
        }
    }

    // Test invalid string cases
    let invalid_cases = vec![
        "hello",          // Unquoted string
        "\"unterminated", // Unterminated string
        "\"invalid\\z\"", // Invalid escape sequence
    ];

    for invalid_json in invalid_cases {
        let result = JToken::parse_string(invalid_json, 64);
        assert!(
            result.is_err(),
            "Should fail to parse invalid string: {}",
            invalid_json
        );
    }
}

/// Test JsonTest_Object functionality
#[test]
fn test_json_object() {
    // Test object parsing
    let test_cases = vec![
        ("{}", 0),                                           // Empty object
        ("{\"key\": \"value\"}", 1),                         // Single property
        ("{\"key1\": \"value1\", \"key2\": \"value2\"}", 2), // Multiple properties
        ("{\"number\": 42}", 1),                             // Number property
        ("{\"boolean\": true}", 1),                          // Boolean property
        ("{\"null_value\": null}", 1),                       // Null property
        ("{\"array\": [1, 2, 3]}", 1),                       // Array property
        ("{\"nested\": {\"inner\": \"value\"}}", 1),         // Nested object
        ("{\"mixed\": {\"types\": [1, \"two\", true]}}", 1), // Complex nested structure
    ];

    for (json_str, expected_property_count) in test_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse valid JSON object: {}",
            json_str
        );

        if let Ok(Some(JToken::Object(obj))) = result {
            assert_eq!(
                obj.len(),
                expected_property_count,
                "Object property count mismatch for: {}",
                json_str
            );
        } else {
            panic!("Expected object token for: {}", json_str);
        }
    }

    // Test complex object with all property types
    let complex_json = r#"{
        "string_prop": "hello",
        "number_prop": 42.5,
        "boolean_prop": true,
        "null_prop": null,
        "array_prop": [1, 2, "three"],
        "object_prop": {
            "nested_string": "world",
            "nested_number": 123
        }
    }"#;

    let result = JToken::parse_string(complex_json, 64);
    assert!(result.is_ok(), "Failed to parse complex JSON object");

    if let Ok(Some(JToken::Object(obj))) = result {
        assert_eq!(obj.len(), 6);
        assert!(obj.contains_key(&"string_prop".to_string()));
        assert!(obj.contains_key(&"number_prop".to_string()));
        assert!(obj.contains_key(&"boolean_prop".to_string()));
        assert!(obj.contains_key(&"null_prop".to_string()));
        assert!(obj.contains_key(&"array_prop".to_string()));
        assert!(obj.contains_key(&"object_prop".to_string()));
    } else {
        panic!("Expected object token for complex JSON");
    }
}

/// Test Deserialize_WrongJson functionality
#[test]
fn test_deserialize_wrong_json() {
    // Test additional malformed JSON cases for deserialization
    let malformed_cases = vec![
        "",                                     // Empty string
        "   ",                                  // Whitespace only
        "garbage",                              // Not JSON at all
        "123abc",                               // Invalid number format
        "{\"key\": \"value\" extra}",           // Extra content after valid JSON
        "[1, 2, 3] extra",                      // Extra content after array
        "{\"duplicate\": 1, \"duplicate\": 2}", // Duplicate keys (should be handled gracefully)
        "{\"\\uXXXX\": \"value\"}",             // Invalid unicode escape
        "{\"key\": \"\\u000\"}",                // Incomplete unicode escape
    ];

    for malformed_json in malformed_cases {
        let result = JToken::parse_string(malformed_json, 64);
        if malformed_json == "{\"duplicate\": 1, \"duplicate\": 2}" {
            // Duplicate keys might be handled by taking the last value
            // This is implementation-specific behavior
            if result.is_ok() {
                continue;
            }
        }
        if malformed_json == "" || malformed_json.trim().is_empty() {
            // Empty string or whitespace-only might be handled as None (null) by some parsers
            if result.is_ok() && matches!(result, Ok(None)) {
                continue;
            }
        }
        assert!(
            result.is_err(),
            "Expected error for malformed JSON: '{}'",
            malformed_json
        );
    }
}

/// Test Deserialize_EmptyObject functionality
#[test]
fn test_deserialize_empty_object() {
    // Test empty object deserialization
    let empty_object_cases = vec![
        "{}", " {} ",   // With whitespace
        "\n{}\n", // With newlines
        "\t{}\t", // With tabs
    ];

    for json_str in empty_object_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse empty object: '{}'",
            json_str
        );

        if let Ok(Some(JToken::Object(obj))) = result {
            assert!(obj.is_empty(), "Expected empty object for: '{}'", json_str);
            assert_eq!(obj.len(), 0, "Expected zero properties for: '{}'", json_str);
        } else {
            panic!("Expected object token for: '{}'", json_str);
        }
    }
}

/// Test Deserialize_EmptyArray functionality
#[test]
fn test_deserialize_empty_array() {
    // Test empty array deserialization
    let empty_array_cases = vec![
        "[]", " [] ",   // With whitespace
        "\n[]\n", // With newlines
        "\t[]\t", // With tabs
    ];

    for json_str in empty_array_cases {
        let result = JToken::parse_string(json_str, 64);
        assert!(
            result.is_ok(),
            "Failed to parse empty array: '{}'",
            json_str
        );

        if let Ok(Some(JToken::Array(arr))) = result {
            assert!(arr.is_empty(), "Expected empty array for: '{}'", json_str);
            assert_eq!(arr.len(), 0, "Expected zero elements for: '{}'", json_str);
        } else {
            panic!("Expected array token for: '{}'", json_str);
        }
    }
}

/// Test Deserialize_Map_Test functionality
#[test]
fn test_deserialize_map_test() {
    // Test map (object) deserialization with various key-value combinations
    let map_json = r#"{
        "string_key": "string_value",
        "number_key": 42,
        "boolean_key": true,
        "null_key": null,
        "array_key": [1, 2, 3],
        "object_key": {
            "nested_key": "nested_value"
        }
    }"#;

    let result = JToken::parse_string(map_json, 64);
    assert!(result.is_ok(), "Failed to parse map JSON");

    if let Ok(Some(JToken::Object(obj))) = result {
        // Check that all keys are present
        assert!(obj.contains_key(&"string_key".to_string()));
        assert!(obj.contains_key(&"number_key".to_string()));
        assert!(obj.contains_key(&"boolean_key".to_string()));
        assert!(obj.contains_key(&"null_key".to_string()));
        assert!(obj.contains_key(&"array_key".to_string()));
        assert!(obj.contains_key(&"object_key".to_string()));

        // Check specific values
        if let Some(Some(JToken::String(s))) = obj.get(&"string_key".to_string()) {
            assert_eq!(s, "string_value");
        } else {
            panic!("Expected string value for string_key");
        }

        if let Some(Some(JToken::Number(n))) = obj.get(&"number_key".to_string()) {
            assert_eq!(*n, 42.0);
        } else {
            panic!("Expected number value for number_key");
        }

        if let Some(Some(JToken::Boolean(b))) = obj.get(&"boolean_key".to_string()) {
            assert!(b);
        } else {
            panic!("Expected boolean value for boolean_key");
        }

        if let Some(None) = obj.get(&"null_key".to_string()) {
            // This is correct - null values are represented as None
        } else if let Some(Some(JToken::Null)) = obj.get(&"null_key".to_string()) {
            // This is also acceptable representation
        } else {
            panic!("Expected null value for null_key");
        }

        if let Some(Some(JToken::Array(arr))) = obj.get(&"array_key".to_string()) {
            assert_eq!(arr.len(), 3);
        } else {
            panic!("Expected array value for array_key");
        }

        if let Some(Some(JToken::Object(nested_obj))) = obj.get(&"object_key".to_string()) {
            assert!(nested_obj.contains_key(&"nested_key".to_string()));
        } else {
            panic!("Expected object value for object_key");
        }
    } else {
        panic!("Expected object token for map JSON");
    }
}

/// Test Deserialize_Array_Bool_Str_Num functionality
#[test]
fn test_deserialize_array_bool_str_num() {
    // Test array with mixed boolean, string, and number types
    let mixed_array_json = r#"[true, false, "hello", "world", 123, 456.789, 0, -1, ""]"#;

    let result = JToken::parse_string(mixed_array_json, 64);
    assert!(result.is_ok(), "Failed to parse mixed array JSON");

    if let Ok(Some(JToken::Array(arr))) = result {
        assert_eq!(arr.len(), 9, "Expected 9 elements in mixed array");

        // Check each element type and value
        if let Some(Some(JToken::Boolean(b))) = arr.get(0) {
            assert!(b, "Expected first element to be true");
        } else {
            panic!("Expected boolean at index 0");
        }

        if let Some(Some(JToken::Boolean(b))) = arr.get(1) {
            assert!(!b, "Expected second element to be false");
        } else {
            panic!("Expected boolean at index 1");
        }

        if let Some(Some(JToken::String(s))) = arr.get(2) {
            assert_eq!(s, "hello", "Expected 'hello' at index 2");
        } else {
            panic!("Expected string at index 2");
        }

        if let Some(Some(JToken::String(s))) = arr.get(3) {
            assert_eq!(s, "world", "Expected 'world' at index 3");
        } else {
            panic!("Expected string at index 3");
        }

        if let Some(Some(JToken::Number(n))) = arr.get(4) {
            assert_eq!(*n, 123.0, "Expected 123 at index 4");
        } else {
            panic!("Expected number at index 4");
        }

        if let Some(Some(JToken::Number(n))) = arr.get(5) {
            assert!(
                (n - 456.789).abs() < f64::EPSILON,
                "Expected 456.789 at index 5"
            );
        } else {
            panic!("Expected number at index 5");
        }

        if let Some(Some(JToken::Number(n))) = arr.get(6) {
            assert_eq!(*n, 0.0, "Expected 0 at index 6");
        } else {
            panic!("Expected number at index 6");
        }

        if let Some(Some(JToken::Number(n))) = arr.get(7) {
            assert_eq!(*n, -1.0, "Expected -1 at index 7");
        } else {
            panic!("Expected number at index 7");
        }

        if let Some(Some(JToken::String(s))) = arr.get(8) {
            assert_eq!(s, "", "Expected empty string at index 8");
        } else {
            panic!("Expected string at index 8");
        }
    } else {
        panic!("Expected array token for mixed array JSON");
    }
}

/// Test Deserialize_Array_OfArray functionality
#[test]
fn test_deserialize_array_of_array() {
    // Test nested array deserialization
    let nested_array_json = r#"[
        [1, 2, 3],
        ["a", "b", "c"],
        [true, false],
        [],
        [null],
        [[1, 2], [3, 4]],
        [{"key": "value"}]
    ]"#;

    let result = JToken::parse_string(nested_array_json, 64);
    assert!(result.is_ok(), "Failed to parse nested array JSON");

    if let Ok(Some(JToken::Array(outer_arr))) = result {
        assert_eq!(outer_arr.len(), 7, "Expected 7 sub-arrays");

        // Check first sub-array [1, 2, 3]
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(0) {
            assert_eq!(sub_arr.len(), 3);
            if let Some(Some(JToken::Number(n))) = sub_arr.get(0) {
                assert_eq!(*n, 1.0);
            }
        } else {
            panic!("Expected array at index 0");
        }

        // Check second sub-array ["a", "b", "c"]
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(1) {
            assert_eq!(sub_arr.len(), 3);
            if let Some(Some(JToken::String(s))) = sub_arr.get(0) {
                assert_eq!(s, "a");
            }
        } else {
            panic!("Expected array at index 1");
        }

        // Check third sub-array [true, false]
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(2) {
            assert_eq!(sub_arr.len(), 2);
            if let Some(Some(JToken::Boolean(b))) = sub_arr.get(0) {
                assert!(b);
            }
        } else {
            panic!("Expected array at index 2");
        }

        // Check fourth sub-array [] (empty)
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(3) {
            assert_eq!(sub_arr.len(), 0);
        } else {
            panic!("Expected empty array at index 3");
        }

        // Check fifth sub-array [null]
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(4) {
            assert_eq!(sub_arr.len(), 1);
            // The null element should be None or JToken::Null
            assert!(sub_arr.get(0).is_some());
        } else {
            panic!("Expected array at index 4");
        }

        // Check sixth sub-array [[1, 2], [3, 4]] (nested)
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(5) {
            assert_eq!(sub_arr.len(), 2);
            if let Some(Some(JToken::Array(nested_sub_arr))) = sub_arr.get(0) {
                assert_eq!(nested_sub_arr.len(), 2);
            }
        } else {
            panic!("Expected nested array at index 5");
        }

        // Check seventh sub-array [{"key": "value"}]
        if let Some(Some(JToken::Array(sub_arr))) = outer_arr.get(6) {
            assert_eq!(sub_arr.len(), 1);
            if let Some(Some(JToken::Object(obj))) = sub_arr.get(0) {
                assert!(obj.contains_key(&"key".to_string()));
            }
        } else {
            panic!("Expected array with object at index 6");
        }
    } else {
        panic!("Expected array token for nested array JSON");
    }
}
