use super::OrderedDictionary;

#[test]
fn insert_updates_without_moving_existing_key() {
    let mut dict = OrderedDictionary::new();

    dict.insert("a", 1);
    dict.insert("b", 2);
    dict.insert("a", 3);

    assert_eq!(
        dict.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(
        dict.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        vec![3, 2]
    );
}

#[test]
fn try_insert_rejects_duplicates_without_moving_existing_key() {
    let mut dict = OrderedDictionary::new();

    assert!(dict.try_insert("a", 1).is_ok());
    assert!(dict.try_insert("b", 2).is_ok());
    assert_eq!(dict.try_insert("a", 3), Err(("a", 3)));

    assert_eq!(
        dict.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(dict.get(&"a"), Some(&1));
}

#[test]
fn remove_preserves_remaining_insertion_order() {
    let mut dict = OrderedDictionary::new();

    dict.insert("a", 1);
    dict.insert("b", 2);
    dict.insert("c", 3);

    assert!(dict.remove(&"b"));
    assert_eq!(
        dict.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        vec!["a", "c"]
    );
    assert_eq!(
        dict.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        vec![1, 3]
    );
}
