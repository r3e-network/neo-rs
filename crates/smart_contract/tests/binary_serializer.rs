use neo_smart_contract::binary_serializer::BinarySerializer;
use neo_smart_contract::Error;
use neo_vm::stack_item::map::Map;
use neo_vm::stack_item::InteropInterface;
use neo_vm::{ExecutionEngineLimits, StackItem, StackItemType};
use std::collections::BTreeMap;

#[test]
fn serialize_primitives_and_containers() -> Result<(), Error> {
    let limits = ExecutionEngineLimits::default();

    let bytes = BinarySerializer::serialize(vec![0u8; 5], limits)?;
    assert_eq!(bytes, vec![0x28, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00]);

    let bytes = BinarySerializer::serialize(true, limits)?;
    assert_eq!(bytes, vec![0x20, 0x01]);

    let bytes = BinarySerializer::serialize(1i32, limits)?;
    assert_eq!(bytes, vec![0x21, 0x01, 0x01]);

    let array_item = StackItem::from_array(vec![StackItem::from_int(1)]);
    let bytes = BinarySerializer::serialize(array_item.clone(), limits)?;
    assert_eq!(bytes, vec![0x40, 0x01, 0x21, 0x01, 0x01]);

    let struct_item = StackItem::from_struct(vec![StackItem::from_int(1)]);
    let bytes = BinarySerializer::serialize(struct_item.clone(), limits)?;
    assert_eq!(bytes, vec![0x41, 0x01, 0x21, 0x01, 0x01]);

    let mut map_data = BTreeMap::new();
    map_data.insert(StackItem::from_int(2), StackItem::from_int(1));
    let map_item = StackItem::Map(Map::new(map_data, None));
    let bytes = BinarySerializer::serialize(map_item.clone(), limits)?;
    assert_eq!(bytes, vec![0x48, 0x01, 0x21, 0x01, 0x02, 0x21, 0x01, 0x01]);

    Ok(())
}

#[test]
fn serialize_rejects_unsupported_items() {
    #[derive(Debug)]
    struct DummyInterop;

    impl InteropInterface for DummyInterop {
        fn interface_type(&self) -> &str {
            "Dummy"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let limits = ExecutionEngineLimits::default();
    let item = StackItem::from_interface(DummyInterop);
    let err = BinarySerializer::serialize(item, limits).expect_err("interop items are unsupported");
    assert!(matches!(err, Error::InvalidOperation(_)));
}

#[test]
fn deserialize_round_trip_matches_original() -> Result<(), Error> {
    let limits = ExecutionEngineLimits::default();

    let original = StackItem::from_byte_string(vec![0u8; 5]);
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    let original = StackItem::from_bool(true);
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    let original = StackItem::from_int(1);
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    let original = StackItem::from_array(vec![StackItem::from_int(1)]);
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    let original = StackItem::from_struct(vec![StackItem::from_int(1)]);
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    let mut map_data = BTreeMap::new();
    map_data.insert(StackItem::from_int(2), StackItem::from_int(1));
    let original = StackItem::Map(Map::new(map_data, None));
    let bytes = BinarySerializer::serialize(original.clone(), limits)?;
    let restored = BinarySerializer::deserialize(&bytes, limits, None)?;
    assert_eq!(original, restored);

    Ok(())
}

#[test]
fn deserialize_invalid_type_raises_format_error() {
    let limits = ExecutionEngineLimits::default();
    let mut bytes = BinarySerializer::serialize(1i32, limits).expect("serialize integer");
    bytes[0] = StackItemType::Array.to_byte();

    let err = BinarySerializer::deserialize(&bytes, limits, None).expect_err("should fail");
    assert!(matches!(err, Error::SerializationError(_)));
}
