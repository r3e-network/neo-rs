use super::*;

fn ser(item: &StackItem) -> String {
    let bytes = JsonSerializer::serialize_to_byte_array(item, 1 << 20).expect("serialize");
    String::from_utf8(bytes).expect("ascii/utf8 output")
}

fn de(json: &str) -> neo_error::CoreResult<StackItem> {
    // C# defaults: JToken.Parse depth 10, engine MaxStackSize 2048.
    JsonSerializer::deserialize(json.as_bytes(), 10, 2048)
}

#[test]
fn serialize_matches_csharp_stdlib_vectors() {
    // C# UT_StdLib.Json_Serialize.
    assert_eq!(ser(&StackItem::from_int(BigInt::from(5))), "5");
    assert_eq!(ser(&StackItem::from_bool(true)), "true");
    assert_eq!(
        ser(&StackItem::from_byte_string(b"test".to_vec())),
        "\"test\""
    );
    assert_eq!(ser(&StackItem::null()), "null");
    // Map{"key":"value"} (built via deserialize) round-trips compactly.
    assert_eq!(
        ser(&de(r#"{"key":"value"}"#).unwrap()),
        r#"{"key":"value"}"#
    );
}

#[test]
fn serialize_escapes_like_system_text_json() {
    // JavaScriptEncoder.Default: quote -> ", '<'/'>' -> </>,
    // all non-ASCII -> \uXXXX (uppercase), but short forms for \n \t \\.
    assert_eq!(
        ser(&StackItem::from_byte_string(b"a\"b".to_vec())),
        "\"a\\u0022b\""
    );
    assert_eq!(
        ser(&StackItem::from_byte_string("<x>".as_bytes().to_vec())),
        "\"\\u003Cx\\u003E\""
    );
    assert_eq!(
        ser(&StackItem::from_byte_string("中".as_bytes().to_vec())),
        "\"\\u4E2D\""
    );
    assert_eq!(
        ser(&StackItem::from_byte_string(b"\n\t\\".to_vec())),
        r#""\n\t\\""#
    );
}

#[test]
fn serialize_rejects_out_of_safe_range_integer() {
    // C# throws when the integer leaves the JS safe-integer range.
    let too_big = BigInt::from(JsonSerializer::MAX_SAFE_INTEGER) + 1;
    assert!(
        JsonSerializer::serialize_to_byte_array(&StackItem::from_int(too_big), 1 << 20)
            .is_err()
    );
}

#[test]
fn deserialize_matches_csharp_vectors() {
    // C# UT_StdLib.Json_Deserialize: "123" -> 123, "null" -> Null,
    // "***" -> fault, "123.45" -> fault ("no decimals"). Verified by
    // re-serializing (round-trip) so no StackItem accessor is needed.
    assert!(matches!(de("null").unwrap(), StackItem::Null));
    assert_eq!(ser(&de("123").unwrap()), "123");
    // UT_JsonSerializer.Numbers: integer-valued scientific float -> integer.
    assert_eq!(ser(&de("200.500000E+005").unwrap()), "20050000");
    assert!(de("123.45").is_err(), "fractional value is rejected");
    assert!(de("***").is_err(), "invalid JSON is rejected");
    // Structural round-trips (string / array / object key order).
    assert_eq!(ser(&de(r#""test""#).unwrap()), r#""test""#);
    assert_eq!(ser(&de("[1,true,null]").unwrap()), "[1,true,null]");
    assert_eq!(ser(&de(r#"{"b":1,"a":2}"#).unwrap()), r#"{"b":1,"a":2}"#);
}

#[test]
fn deserialize_enforces_depth_and_item_limits() {
    // Depth limit faults (C# JToken.Parse(json, maxDepth)).
    assert!(JsonSerializer::deserialize(b"[[[1]]]", 2, 2048).is_err());
    assert!(JsonSerializer::deserialize(b"[[[1]]]", 8, 2048).is_ok());

    // Item-count limit faults like C# maxStackSize: a wide-but-shallow array
    // would HALT without this guard but must FAULT to match C#.
    // `[1,1,1,1,1]` = 1 (array) + 5 (elements) = 6 items.
    assert!(JsonSerializer::deserialize(b"[1,1,1,1,1]", 10, 6).is_ok());
    assert!(JsonSerializer::deserialize(b"[1,1,1,1,1]", 10, 5).is_err());

    // A map charges 1 (map) + 2 per entry (entry + value): {"a":1} = 3.
    assert!(JsonSerializer::deserialize(br#"{"a":1}"#, 10, 3).is_ok());
    assert!(JsonSerializer::deserialize(br#"{"a":1}"#, 10, 2).is_err());
}
