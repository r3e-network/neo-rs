use super::*;

fn ser(item: &StackItem) -> String {
    let bytes = JsonSerializer::serialize_to_byte_array(item, 1 << 20).expect("serialize");
    String::from_utf8(bytes).expect("ascii/utf8 output")
}

fn de(json: &str) -> neo_error::CoreResult<StackItem> {
    // C# defaults: JToken.Parse depth 10, engine MaxStackSize 2048. Post-Basilisk
    // number handling (the modern chain default); pre-Basilisk is covered by the
    // dedicated `deserialize_number_gates_on_basilisk` test.
    JsonSerializer::deserialize(json.as_bytes(), 10, 2048, true)
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
        JsonSerializer::serialize_to_byte_array(&StackItem::from_int(too_big), 1 << 20).is_err()
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
    assert!(JsonSerializer::deserialize(b"[[[1]]]", 2, 2048, true).is_err());
    assert!(JsonSerializer::deserialize(b"[[[1]]]", 8, 2048, true).is_ok());

    // Item-count limit faults like C# maxStackSize: a wide-but-shallow array
    // would HALT without this guard but must FAULT to match C#.
    // `[1,1,1,1,1]` = 1 (array) + 5 (elements) = 6 items.
    assert!(JsonSerializer::deserialize(b"[1,1,1,1,1]", 10, 6, true).is_ok());
    assert!(JsonSerializer::deserialize(b"[1,1,1,1,1]", 10, 5, true).is_err());

    // A map charges 1 (map) + 2 per entry (entry + value): {"a":1} = 3.
    assert!(JsonSerializer::deserialize(br#"{"a":1}"#, 10, 3, true).is_ok());
    assert!(JsonSerializer::deserialize(br#"{"a":1}"#, 10, 2, true).is_err());
}

#[test]
fn deserialize_enforces_neovm_map_key_size() {
    let key64 = "a".repeat(64);
    let key65 = "a".repeat(65);

    assert!(de(&format!(r#"{{"{key64}":1}}"#)).is_ok());
    assert!(de(&format!(r#"{{"{key65}":1}}"#)).is_err());
}

#[test]
fn deserialize_large_integer_rounds_through_double_like_csharp() {
    // C# JsonSerializer.Deserialize reads every JNumber as a `double` (Neo.Json
    // JNumber.Value) before converting to BigInteger, so integer literals beyond
    // 2^53 lose precision. Reproduce that: <= 2^53 is exact; > 2^53 takes the
    // nearest-double value.
    let de_int = |s: &str| de(s).unwrap().as_int().unwrap();
    assert_eq!(de_int("42"), BigInt::from(42));
    // 2^53 is exactly representable as f64.
    assert_eq!(
        de_int("9007199254740992"),
        BigInt::from(9007199254740992i64)
    );
    // 2^53 + 1 is NOT representable; the nearest double is 2^53, so C# (and now
    // this node) yields 9007199254740992, NOT the exact 9007199254740993.
    assert_eq!(
        de_int("9007199254740993"),
        BigInt::from(9007199254740992i64)
    );
}

#[test]
fn deserialize_number_gates_on_basilisk() {
    // P0 consensus-replay parity: C# JsonSerializer.Deserialize converts a JSON
    // number to a VM Integer differently before vs after HF_Basilisk
    // (JsonSerializer.cs:197-201). Replaying mainnet blocks below the Basilisk
    // height (mainnet < 4_120_000 / testnet < 2_680_000) MUST use the pre-Basilisk
    // path or state diverges from C#.
    let de_int = |s: &str, basilisk: bool| {
        JsonSerializer::deserialize(s.as_bytes(), 10, 2048, basilisk)
            .unwrap()
            .as_int()
            .unwrap()
    };

    // "1e30": serde parses 1e30 to the same f64 C#'s GetDouble() produces.
    // Pre-Basilisk `(BigInteger)num.Value` yields the double's EXACT stored value;
    // post-Basilisk `BigInteger.Parse(num.Value.ToString())` yields decimal 10^30.
    let pre_1e30: BigInt = "1000000000000000019884624838656".parse().unwrap();
    let post_1e30: BigInt = "1000000000000000000000000000000".parse().unwrap(); // 10^30
    // Cross-check pre_1e30 against the double's binary expansion: 1e30 rounds to
    // the double 0x46293E5939A08CEA, whose value is mantissa*2^exp =
    // 7105427357601002 * 2^47, and 7105427357601002 << 47 ==
    // 1000000000000000019884624838656.
    assert_eq!(pre_1e30, BigInt::from(7_105_427_357_601_002u64) << 47);
    assert_ne!(pre_1e30, post_1e30);
    assert_eq!(de_int("1e30", false), pre_1e30);
    assert_eq!(de_int("1e30", true), post_1e30);

    // A small integer is identical in both eras (exact within +/-2^53).
    assert_eq!(de_int("42", false), BigInt::from(42));
    assert_eq!(de_int("42", true), BigInt::from(42));

    // Negative large magnitude also matches the truncated exact double pre-Basilisk.
    assert_eq!(de_int("-1e30", false), -pre_1e30.clone());
    assert_eq!(de_int("-1e30", true), -post_1e30);

    // Fractional numbers fault in BOTH eras (C#'s `num.Value % 1 != 0` check runs
    // before the hardfork gate).
    assert!(JsonSerializer::deserialize(b"123.45", 10, 2048, false).is_err());
    assert!(JsonSerializer::deserialize(b"123.45", 10, 2048, true).is_err());
}
