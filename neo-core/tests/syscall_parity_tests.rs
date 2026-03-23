// Syscall parity tests against C# Neo v3.9.1

use neo_core::smart_contract::find_options::FindOptions;

#[test]
fn test_find_options_keys_only_and_values_only_conflict() {
    let options = FindOptions::KeysOnly | FindOptions::ValuesOnly;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be used together"));
}

#[test]
fn test_find_options_pick_field_without_deserialize() {
    let options = FindOptions::PickField0;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("require DeserializeValues"));
}

#[test]
fn test_find_options_pick_field0_and_field1_conflict() {
    let options = FindOptions::PickField0 | FindOptions::PickField1 | FindOptions::DeserializeValues;
    let result = options.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be used together"));
}

#[test]
fn test_find_options_valid_combinations() {
    // Valid: KeysOnly alone
    assert!(FindOptions::KeysOnly.validate().is_ok());

    // Valid: ValuesOnly alone
    assert!(FindOptions::ValuesOnly.validate().is_ok());

    // Valid: DeserializeValues with PickField0
    let options = FindOptions::DeserializeValues | FindOptions::PickField0;
    assert!(options.validate().is_ok());

    // Valid: RemovePrefix with Backwards
    let options = FindOptions::RemovePrefix | FindOptions::Backwards;
    assert!(options.validate().is_ok());
}

