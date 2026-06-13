//! Simplified JSONPath compatibility tests.

use neo_serialization::json::{JPathToken, JToken, OrderedDictionary};

fn make_object(entries: Vec<(String, Option<JToken>)>) -> JToken {
    let mut dict = OrderedDictionary::new();
    for (key, value) in entries {
        dict.insert(key, value);
    }
    JToken::from_object(dict)
}

fn make_array(items: Vec<Option<JToken>>) -> JToken {
    JToken::from_array(items)
}

#[test]
fn test_json_path_simple_properties() {
    let json = make_object(vec![
        ("name".to_string(), Some(JToken::String("Neo".to_string()))),
        ("version".to_string(), Some(JToken::Number(3.0))),
    ]);

    let tokens = JPathToken::parse("$.name").unwrap();
    let results = JPathToken::evaluate(&tokens, &json).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], &JToken::String("Neo".to_string()));

    let tokens = JPathToken::parse("$.version").unwrap();
    let results = JPathToken::evaluate(&tokens, &json).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], &JToken::Number(3.0));
}

#[test]
fn test_json_path_array_index() {
    let array = make_array(vec![
        Some(JToken::String("first".to_string())),
        Some(JToken::String("second".to_string())),
        Some(JToken::Number(42.0)),
    ]);
    let json = make_object(vec![("items".to_string(), Some(array))]);

    let tokens = JPathToken::parse("$.items[0]").unwrap();
    let results = JPathToken::evaluate(&tokens, &json).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], &JToken::String("first".to_string()));

    let tokens = JPathToken::parse("$.items[2]").unwrap();
    let results = JPathToken::evaluate(&tokens, &json).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], &JToken::Number(42.0));
}

#[test]
fn test_json_path_wildcard() {
    let array = make_array(vec![
        Some(JToken::String("a".to_string())),
        Some(JToken::String("b".to_string())),
        Some(JToken::String("c".to_string())),
    ]);
    let json = make_object(vec![("letters".to_string(), Some(array))]);

    let tokens = JPathToken::parse("$.letters[*]").unwrap();
    let results = JPathToken::evaluate(&tokens, &json).unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results
            .iter()
            .any(|value| **value == JToken::String("a".to_string()))
    );
    assert!(
        results
            .iter()
            .any(|value| **value == JToken::String("b".to_string()))
    );
    assert!(
        results
            .iter()
            .any(|value| **value == JToken::String("c".to_string()))
    );
}

// ---------------------------------------------------------------------------
// Differential pinning tests against C# Neo.Json UT_JPath.cs.
//
// The Oracle filter (an on-chain, untrusted JSONPath string) is evaluated here,
// so any divergence from C# Neo.Json's JPathToken is a CONSENSUS FORK. These
// vectors are transcribed verbatim from
// neo_csharp/tests/Neo.Json.UnitTests/UT_JPath.cs and assert byte-identical
// output (via `to_string_formatted(false)`, which mirrors C# `JToken.ToString()`).
// ---------------------------------------------------------------------------

/// The exact UT_JPath.cs fixture (`json`), built via `JToken::parse`.
const STORE_JSON: &str = r#"{"store":{"book":[{"category":"reference","author":"Nigel Rees","title":"Sayings of the Century","price":8.95},{"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99},{"category":"fiction","author":"Herman Melville","title":"Moby Dick","isbn":"0-553-21311-3","price":8.99},{"category":"fiction","author":"J. R. R. Tolkien","title":"The Lord of the Rings","isbn":"0-395-19395-8","price":null}],"bicycle":{"color":"red","price":19.95}},"expensive":10,"data":null}"#;

fn store() -> JToken {
    JToken::parse(STORE_JSON, 64).unwrap()
}

/// Evaluate `path` against `root` and render the resulting array exactly as C#
/// `JToken.ToString()` would (`ToByteArray(false)` semantics).
fn jpath_str(root: &JToken, path: &str) -> String {
    JToken::from(root.json_path(path).unwrap())
        .to_string_formatted(false)
        .unwrap()
}

#[test]
fn ut_jpath_success_vectors() {
    let json = store();

    // Recursive descent + wildcard equivalence (UT_JPath.cs:74-75).
    let authors = r#"["Nigel Rees","Evelyn Waugh","Herman Melville","J. R. R. Tolkien"]"#;
    assert_eq!(jpath_str(&json, "$.store.book[*].author"), authors);
    assert_eq!(jpath_str(&json, "$..author"), authors);

    // Dot + Asterisk over an object (UT_JPath.cs:76).
    assert_eq!(
        jpath_str(&json, "$.store.*"),
        r#"[[{"category":"reference","author":"Nigel Rees","title":"Sayings of the Century","price":8.95},{"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99},{"category":"fiction","author":"Herman Melville","title":"Moby Dick","isbn":"0-553-21311-3","price":8.99},{"category":"fiction","author":"J. R. R. Tolkien","title":"The Lord of the Rings","isbn":"0-395-19395-8","price":null}],{"color":"red","price":19.95}]"#
    );

    // Recursive descent collecting nulls (UT_JPath.cs:77).
    assert_eq!(jpath_str(&json, "$.store..price"), r#"[19.95,8.95,12.99,8.99,null]"#);

    // Indexing, including negative index normalization (UT_JPath.cs:78-79).
    let moby = r#"[{"category":"fiction","author":"Herman Melville","title":"Moby Dick","isbn":"0-553-21311-3","price":8.99}]"#;
    assert_eq!(jpath_str(&json, "$..book[2]"), moby);
    assert_eq!(jpath_str(&json, "$..book[-2]"), moby);

    // Numeric union (UT_JPath.cs:80).
    let first_two = r#"[{"category":"reference","author":"Nigel Rees","title":"Sayings of the Century","price":8.95},{"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99}]"#;
    assert_eq!(jpath_str(&json, "$..book[0,1]"), first_two);

    // Slices (UT_JPath.cs:81-84).
    assert_eq!(jpath_str(&json, "$..book[:2]"), first_two);
    assert_eq!(
        jpath_str(&json, "$..book[1:2]"),
        r#"[{"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99}]"#
    );
    let last_two = r#"[{"category":"fiction","author":"Herman Melville","title":"Moby Dick","isbn":"0-553-21311-3","price":8.99},{"category":"fiction","author":"J. R. R. Tolkien","title":"The Lord of the Rings","isbn":"0-395-19395-8","price":null}]"#;
    assert_eq!(jpath_str(&json, "$..book[-2:]"), last_two);
    assert_eq!(jpath_str(&json, "$..book[2:]"), last_two);

    // Empty expression returns [root] (UT_JPath.cs:85).
    assert_eq!(
        jpath_str(&json, ""),
        format!("[{}]", json.to_string_formatted(false).unwrap())
    );

    // `$.*` flattens root's children, INCLUDING the trailing null (UT_JPath.cs:86).
    assert_eq!(
        jpath_str(&json, "$.*"),
        r#"[{"book":[{"category":"reference","author":"Nigel Rees","title":"Sayings of the Century","price":8.95},{"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99},{"category":"fiction","author":"Herman Melville","title":"Moby Dick","isbn":"0-553-21311-3","price":8.99},{"category":"fiction","author":"J. R. R. Tolkien","title":"The Lord of the Rings","isbn":"0-395-19395-8","price":null}],"bicycle":{"color":"red","price":19.95}},10,null]"#
    );

    // Non-existent field yields [] (UT_JPath.cs:87).
    assert_eq!(jpath_str(&json, "$..invalidfield"), "[]");
}

#[test]
fn ut_jpath_quoted_key_vectors() {
    // Quoted-key parsing — guards against the parse_string off-by-one regression.
    let json2 = JToken::parse(r#"{"a":{"b":7}}"#, 64).unwrap();
    assert_eq!(jpath_str(&json2, "$['a']"), r#"[{"b":7}]"#);
    assert_eq!(jpath_str(&json2, "$['a']['b']"), "[7]");
    // String union over the same key twice.
    assert_eq!(jpath_str(&json2, "$['a','a']"), r#"[{"b":7},{"b":7}]"#);
}

#[test]
fn ut_jpath_max_depth_bound() {
    // 7 descents exceed maxDepth=6 → error. This is the key consensus DoS bound
    // (UT_JPath.cs:91-94, TestMaxDepth).
    let json = store();
    assert!(json.json_path("$..book[*].author").is_err());
}

#[test]
fn ut_jpath_invalid_format_vectors() {
    // Every one of these must be rejected (UT_JPath.cs:97-171, TestInvalidFormat).
    let json = store();
    for bad in [
        "$..*",
        "..book",
        "$..",
        "@#$%^&*()",
        "$.store.book[",
        "$.store.book)]",
        "$.store.book=>2",
        "$.store.'book'",
        "$.store.[book]",
        "$..*..author",
        "$.store.book..[0]",
        "$..@.book",
        "$.store.book.length()",
    ] {
        assert!(
            json.json_path(bad).is_err(),
            "expected `{bad}` to be rejected"
        );
    }
}

#[test]
fn ut_jpath_oom_bound() {
    // maxObjects guard (UT_JPath.cs:64-69, TestOOM): `$` + ("[0" + ",0"*64 + "]")*6.
    let inner = format!("[0{}]", ",0".repeat(64));
    let filter = format!("${}", inner.repeat(6));
    let json = JToken::parse("[[[[[[{}]]]]]]", 64).unwrap();
    assert!(json.json_path(&filter).is_err());
}
