//! Simplified JObject tests.

use neo_json::{JObject, JToken, OrderedDictionary};

#[test]
fn test_object_creation_and_access() {
    let mut dict = OrderedDictionary::new();
    dict.insert("name".to_string(), Some(JToken::String("neo".to_string())));
    dict.insert("height".to_string(), Some(JToken::Number(1.0)));
    dict.insert("missing".to_string(), None);

    let object = JObject::from_properties(dict);
    assert_eq!(object.len(), 3);
    assert_eq!(object.get("name"), Some(&JToken::String("neo".to_string())));
    assert!(object.get("missing").is_none());
}
