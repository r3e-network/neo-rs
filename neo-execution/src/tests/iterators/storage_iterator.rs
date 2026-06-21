use super::*;

fn iterator_for_value(value: Vec<u8>, options: FindOptions) -> StorageIterator {
    StorageIterator::new(
        vec![(
            StorageKey::new(7, vec![0x01]),
            StorageItem::from_bytes(value),
        )],
        0,
        options,
    )
}

#[test]
fn value_before_next_faults_like_csharp_enumerator_current() {
    let iterator = iterator_for_value(vec![0x01], FindOptions::ValuesOnly);

    assert!(
        iterator.value().is_err(),
        "C# StorageIterator.Value reads enumerator.Current and faults before MoveNext"
    );
}

#[test]
fn deserialize_values_propagates_invalid_storage_payload() {
    let mut iterator = iterator_for_value(
        vec![0xff],
        FindOptions::ValuesOnly | FindOptions::DeserializeValues,
    );

    assert!(iterator.next());
    assert!(
        iterator.value().is_err(),
        "C# BinarySerializer.Deserialize failures are not converted to raw bytes"
    );
}

#[test]
fn pick_field_requires_deserialized_array_like_csharp() {
    let serialized_integer =
        BinarySerializer::serialize(&StackItem::from_i64(1), &ExecutionEngineLimits::default())
            .expect("integer serializes");
    let mut iterator = iterator_for_value(
        serialized_integer,
        FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField0,
    );

    assert!(iterator.next());
    assert!(
        iterator.value().is_err(),
        "C# casts deserialized values to Array before PickField0/PickField1"
    );
}

#[test]
fn pick_field_out_of_range_faults_like_csharp_array_indexer() {
    let serialized_array = BinarySerializer::serialize(
        &StackItem::from_array(vec![StackItem::from_i64(1)]),
        &ExecutionEngineLimits::default(),
    )
    .expect("array serializes");
    let mut iterator = iterator_for_value(
        serialized_array,
        FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField1,
    );

    assert!(iterator.next());
    assert!(
        iterator.value().is_err(),
        "C# Array indexer faults when PickField1 is requested for a one-item array"
    );
}

#[test]
fn value_uses_stack_value_projection_until_vm_return() {
    let source = include_str!("../../iterators/storage_iterator.rs");
    let start = source.find("fn value(&self)").expect("value method exists");
    let end = source[start..]
        .find("fn dispose")
        .map(|offset| start + offset)
        .expect("dispose method follows value");
    let value_method = &source[start..end];

    assert!(value_method.contains("deserialize_stack_value_with_limits"));
    assert!(value_method.contains("StackValue::Array"));
    assert!(value_method.contains("StackValue::Struct"));
    assert!(value_method.contains("stack_value_to_stack_item("));
    assert!(source.contains("StackItem::try_from(value)"));
    assert!(!value_method.contains("BinarySerializer::deserialize("));
    assert!(!value_method.contains("StackItem::from_struct"));
}
