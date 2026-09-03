//! Simplified JSONPath compatibility tests.

use neo_json::{JPathToken, JToken, OrderedDictionary};

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
    assert!(results
        .iter()
        .any(|value| **value == JToken::String("a".to_string())));
    assert!(results
        .iter()
        .any(|value| **value == JToken::String("b".to_string())));
    assert!(results
        .iter()
        .any(|value| **value == JToken::String("c".to_string())));
}
