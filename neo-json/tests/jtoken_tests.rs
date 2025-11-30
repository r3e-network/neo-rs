//! Simplified JToken compatibility tests.

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
fn test_basic_token_construction() {
    assert_eq!(JToken::Null, JToken::Null);
    assert_eq!(JToken::Boolean(true), JToken::Boolean(true));
    assert_eq!(JToken::Number(42.0), JToken::Number(42.0));
    assert_eq!(
        JToken::String("neo".to_string()),
        JToken::String("neo".to_string())
    );
}

#[test]
fn test_array_behaviour() {
    let array = make_array(vec![
        Some(JToken::Null),
        Some(JToken::Boolean(true)),
        Some(JToken::Number(5.0)),
        None,
    ]);

    if let JToken::Array(inner) = &array {
        assert_eq!(inner.len(), 4);
        assert_eq!(inner.get(0), Some(&JToken::Null));
        assert_eq!(inner.get(1), Some(&JToken::Boolean(true)));
        assert_eq!(inner.get(2), Some(&JToken::Number(5.0)));
        assert_eq!(inner.get(3), None);
    } else {
        panic!("expected array");
    }

    assert_eq!(array.get_index(0).unwrap(), Some(&JToken::Null));
    assert!(array.get_index(10).is_err());
}

#[test]
fn test_object_behaviour() {
    let token = make_object(vec![
        ("flag".to_string(), Some(JToken::Boolean(true))),
        ("count".to_string(), Some(JToken::Number(3.0))),
        ("missing".to_string(), None),
    ]);

    if let JToken::Object(obj) = token {
        assert_eq!(obj.len(), 3);
        assert_eq!(obj.get("flag"), Some(&JToken::Boolean(true)));
        assert_eq!(obj.get("missing"), None);
        assert_eq!(obj.get("other"), None);
    } else {
        panic!("expected object");
    }
}

#[test]
fn test_equality() {
    let left = make_array(vec![Some(JToken::Number(1.0)), Some(JToken::Number(2.0))]);
    let right = make_array(vec![Some(JToken::Number(1.0)), Some(JToken::Number(2.0))]);
    assert_eq!(left, right);

    let object_a = make_object(vec![("value".to_string(), Some(JToken::Number(10.0)))]);
    let object_b = make_object(vec![("value".to_string(), Some(JToken::Number(10.0)))]);
    assert_eq!(object_a, object_b);
}
