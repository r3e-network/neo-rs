//! Simplified serialization compatibility tests.

use neo_json::{JToken, OrderedDictionary};

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
fn test_basic_serialization() {
    let token = make_object(vec![
        ("flag".to_string(), Some(JToken::Boolean(true))),
        ("count".to_string(), Some(JToken::Number(7.0))),
        (
            "values".to_string(),
            Some(make_array(vec![
                Some(JToken::Number(1.0)),
                Some(JToken::Number(2.0)),
                None,
            ])),
        ),
    ]);

    let serialized = serde_json::to_string(&token).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert!(parsed["flag"].as_bool().unwrap());
    assert_eq!(parsed["count"].as_f64().unwrap(), 7.0);
    assert_eq!(parsed["values"].as_array().unwrap().len(), 3);
}

#[test]
fn test_roundtrip_serialization() {
    let original = make_object(vec![
        ("name".to_string(), Some(JToken::String("neo".to_string()))),
        (
            "numbers".to_string(),
            Some(make_array(vec![
                Some(JToken::Number(10.0)),
                Some(JToken::Number(20.0)),
            ])),
        ),
    ]);

    let json = serde_json::to_string(&original).unwrap();
    let restored: JToken = serde_json::from_str(&json).unwrap();
    assert_eq!(original, restored);
}
