//! Simplified JArray tests.

use neo_json::{JArray, JToken, OrderedDictionary};

#[test]
fn test_array_creation_and_access() {
    let array = JArray::from_vec(vec![
        Some(JToken::String("first".to_string())),
        Some(JToken::Number(2.0)),
        None,
    ]);

    assert_eq!(array.len(), 3);
    assert_eq!(array.get(0), Some(&JToken::String("first".to_string())));
    assert_eq!(array.get(1), Some(&JToken::Number(2.0)));
    assert_eq!(array.get(2), None);
}

#[test]
fn test_array_mutation() {
    let mut array = JArray::new();
    array.add(Some(JToken::Number(1.0)));
    array.add(Some(JToken::Number(2.0)));
    array.add(None);

    assert_eq!(array.len(), 3);
    let _ = array.set(1, Some(JToken::String("changed".to_string())));
    assert_eq!(array.get(1), Some(&JToken::String("changed".to_string())));
}

#[test]
fn test_array_nested_structures() {
    let mut obj = OrderedDictionary::new();
    obj.insert("id".to_string(), Some(JToken::Number(7.0)));

    let array = JArray::from_vec(vec![
        Some(JToken::from_object(obj)),
        Some(JToken::from_array(vec![Some(JToken::Boolean(true))])),
    ]);

    assert_eq!(array.len(), 2);
}
