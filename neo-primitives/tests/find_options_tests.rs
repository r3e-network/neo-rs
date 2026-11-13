use neo_primitives::FindOptions;

#[test]
fn find_options_none_is_zero() {
    assert_eq!(FindOptions::None.bits(), 0);
}

#[test]
fn find_options_individual_bits() {
    assert_eq!(FindOptions::KeysOnly.bits(), 1);
    assert_eq!(FindOptions::RemovePrefix.bits(), 2);
    assert_eq!(FindOptions::ValuesOnly.bits(), 4);
    assert_eq!(FindOptions::DeserializeValues.bits(), 8);
    assert_eq!(FindOptions::PickField0.bits(), 16);
    assert_eq!(FindOptions::PickField1.bits(), 32);
    assert_eq!(FindOptions::Backwards.bits(), 128);
}

#[test]
fn find_options_all_contains_all_flags() {
    let all = FindOptions::All;
    assert!(all.contains(FindOptions::KeysOnly));
    assert!(all.contains(FindOptions::RemovePrefix));
    assert!(all.contains(FindOptions::ValuesOnly));
    assert!(all.contains(FindOptions::DeserializeValues));
    assert!(all.contains(FindOptions::PickField0));
    assert!(all.contains(FindOptions::PickField1));
    assert!(all.contains(FindOptions::Backwards));
}

#[test]
fn find_options_validate_none_succeeds() {
    let result = FindOptions::None.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_keys_only_alone_succeeds() {
    let result = FindOptions::KeysOnly.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_values_only_alone_succeeds() {
    let result = FindOptions::ValuesOnly.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_keys_and_values_fails() {
    let options = FindOptions::KeysOnly | FindOptions::ValuesOnly;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("KeysOnly and ValuesOnly"));
}

#[test]
fn find_options_validate_pick_field0_requires_deserialize() {
    let options = FindOptions::PickField0;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("require DeserializeValues"));
}

#[test]
fn find_options_validate_pick_field1_requires_deserialize() {
    let options = FindOptions::PickField1;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("require DeserializeValues"));
}

#[test]
fn find_options_validate_pick_field0_with_deserialize_succeeds() {
    let options = FindOptions::PickField0 | FindOptions::DeserializeValues;
    let result = options.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_pick_field1_with_deserialize_succeeds() {
    let options = FindOptions::PickField1 | FindOptions::DeserializeValues;
    let result = options.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_pick_field0_and_pick_field1_fails() {
    let options =
        FindOptions::PickField0 | FindOptions::PickField1 | FindOptions::DeserializeValues;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("PickField0 and PickField1"));
}

#[test]
fn find_options_validate_backwards_alone_succeeds() {
    let result = FindOptions::Backwards.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_remove_prefix_alone_succeeds() {
    let result = FindOptions::RemovePrefix.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_deserialize_values_alone_succeeds() {
    let result = FindOptions::DeserializeValues.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_validate_complex_valid_combination() {
    let options = FindOptions::DeserializeValues
        | FindOptions::PickField0
        | FindOptions::RemovePrefix
        | FindOptions::Backwards;
    let result = options.validate();
    assert!(result.is_ok());
}

#[test]
fn find_options_union_operations() {
    let options = FindOptions::KeysOnly | FindOptions::Backwards;
    assert!(options.contains(FindOptions::KeysOnly));
    assert!(options.contains(FindOptions::Backwards));
    assert!(!options.contains(FindOptions::ValuesOnly));
}

#[test]
fn find_options_intersection_operations() {
    let options1 = FindOptions::KeysOnly | FindOptions::Backwards;
    let options2 = FindOptions::Backwards | FindOptions::RemovePrefix;
    let intersection = options1 & options2;
    assert_eq!(intersection, FindOptions::Backwards);
}

#[test]
fn find_options_difference_operations() {
    let options = FindOptions::All;
    let without_backwards = options - FindOptions::Backwards;
    assert!(!without_backwards.contains(FindOptions::Backwards));
    assert!(without_backwards.contains(FindOptions::KeysOnly));
}

#[test]
fn find_options_is_empty() {
    assert!(FindOptions::None.is_empty());
    assert!(!FindOptions::KeysOnly.is_empty());
}

#[test]
fn find_options_clone_and_copy() {
    let options = FindOptions::KeysOnly;
    let cloned = options;
    let copied = options;
    assert_eq!(options, cloned);
    assert_eq!(options, copied);
}

#[test]
fn find_options_debug_format() {
    let options = FindOptions::KeysOnly | FindOptions::Backwards;
    let debug_str = format!("{options:?}");
    assert!(debug_str.contains("KeysOnly"));
    assert!(debug_str.contains("Backwards"));
}

#[test]
fn find_options_equality() {
    assert_eq!(FindOptions::KeysOnly, FindOptions::KeysOnly);
    assert_ne!(FindOptions::KeysOnly, FindOptions::ValuesOnly);
}
