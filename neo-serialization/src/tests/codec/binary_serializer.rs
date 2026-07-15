use super::*;
use neo_vm::ExecutionEngineLimits;
use neo_vm::stack_item::Map as MapItem;

#[test]
fn deserialize_preserves_map_entry_order_for_roundtrip_bytes() {
    let limits = ExecutionEngineLimits::default();

    // Serialize a map with specific insertion order: (3,30), (1,10), (2,20)
    let mut map_items = neo_vm::VmOrderedDictionary::new();
    map_items.insert(StackItem::Integer(3.into()), StackItem::Integer(30.into()));
    map_items.insert(StackItem::Integer(1.into()), StackItem::Integer(10.into()));
    map_items.insert(StackItem::Integer(2.into()), StackItem::Integer(20.into()));

    let map = StackItem::Map(MapItem::new(map_items, None).unwrap());
    let serialized = BinarySerializer::serialize(&map, &limits).unwrap();

    // Deserialize and verify order is preserved
    let deserialized = BinarySerializer::deserialize(&serialized, &limits, None).unwrap();

    if let StackItem::Map(result_map) = deserialized {
        let items = result_map.items();
        assert_eq!(items.len(), 3);

        // Verify insertion order: (3,30), (1,10), (2,20)
        let items_vec: Vec<_> = items.iter().collect();
        assert_eq!(items_vec[0].0, &StackItem::Integer(3.into()));
        assert_eq!(items_vec[0].1, &StackItem::Integer(30.into()));
        assert_eq!(items_vec[1].0, &StackItem::Integer(1.into()));
        assert_eq!(items_vec[1].1, &StackItem::Integer(10.into()));
        assert_eq!(items_vec[2].0, &StackItem::Integer(2.into()));
        assert_eq!(items_vec[2].1, &StackItem::Integer(20.into()));
    } else {
        panic!("Expected Map");
    }
}

#[test]
fn deserialize_rejects_invalid_map_keys_like_csharp() {
    let limits = ExecutionEngineLimits::default();
    let invalid_keys = [
        StackItem::Null,
        StackItem::from_buffer(vec![1]),
        StackItem::from_array(Vec::new()),
        StackItem::from_byte_string(vec![0x41; 65]),
    ];

    for key in invalid_keys {
        let encoded = BinarySerializer::serialize(
            &StackItem::from_map(vec![(key, StackItem::Null)]),
            &limits,
        )
        .expect("unchecked host map can be encoded for the malformed-input fixture");
        assert!(
            BinarySerializer::deserialize(&encoded, &limits, None).is_err(),
            "Neo rejects non-primitive and oversized map keys during reconstruction"
        );
    }

    let boundary = StackItem::from_map(vec![(
        StackItem::from_byte_string(vec![0x41; 64]),
        StackItem::Null,
    )]);
    let encoded = BinarySerializer::serialize(&boundary, &limits).expect("serialize boundary map");
    assert!(BinarySerializer::deserialize(&encoded, &limits, None).is_ok());
}

#[test]
fn serialize_zero_integer_uses_empty_payload() {
    let limits = ExecutionEngineLimits::default();
    let serialized = BinarySerializer::serialize(&StackItem::from_int(0), &limits).unwrap();
    assert_eq!(serialized, vec![StackItemType::Integer.to_byte(), 0]);
}

#[test]
fn deserialize_default_reads_local_stack_item_shape() {
    let limits = ExecutionEngineLimits::default();
    let item = StackItem::from_struct(vec![
        StackItem::from_int(42i64),
        StackItem::from_byte_string(vec![1, 2, 3]),
        StackItem::from_bool(true),
    ]);
    let serialized = BinarySerializer::serialize(&item, &limits).expect("serialize");

    let value = BinarySerializer::deserialize_default(&serialized).expect("deserialize");
    let StackItem::Struct(structure) = value else {
        panic!("expected struct");
    };
    let fields = structure.items();
    assert_eq!(fields[0].as_int().unwrap(), BigInt::from(42));
    assert_eq!(fields[1].as_bytes().unwrap(), vec![1, 2, 3]);
    assert!(fields[2].as_bool().unwrap());
}

#[test]
fn deserialize_with_limits_enforces_item_limits() {
    let payload = vec![neo_vm::NEOVM_STACK_ITEM_TYPE_ARRAY, 3, 0, 0, 0];
    let mut reader = MemoryReader::new(&payload);

    let err = BinarySerializer::deserialize_with_limits(&mut reader, u16::MAX as u32, 3, None)
        .expect_err("limit error");

    assert_eq!(err.to_string(), "Too many items");
}

#[test]
fn serialize_with_limits_enforces_size() {
    let value = StackItem::from_array(vec![
        StackItem::from_byte_string(vec![1, 2, 3]),
        StackItem::from_int(42),
    ]);
    let err =
        BinarySerializer::serialize_with_limits(&value, 2, 16).expect_err("serialized byte limit");
    assert_eq!(err.to_string(), "Serialized data exceeds limit");
}

#[test]
fn deserialize_stack_item_allows_container_when_total_items_equals_limit() {
    let payload = vec![
        StackItemType::Array.to_byte(),
        1,
        StackItemType::Integer.to_byte(),
        1,
        42,
    ];
    let mut reader = MemoryReader::new(&payload);

    let item = BinarySerializer::deserialize_with_limits(&mut reader, u16::MAX as u32, 2, None)
        .expect("C# allows placeholder plus one child when maxItems is 2");

    let StackItem::Array(array) = item else {
        panic!("expected array");
    };
    assert_eq!(array.len(), 1);
    assert_eq!(array.get(0), Some(StackItem::from_i64(42)));
}

#[test]
fn deserialize_stack_item_rejects_container_when_total_items_exceeds_limit() {
    let payload = vec![
        StackItemType::Array.to_byte(),
        2,
        StackItemType::Integer.to_byte(),
        1,
        1,
        StackItemType::Integer.to_byte(),
        1,
        2,
    ];
    let mut reader = MemoryReader::new(&payload);

    let err = BinarySerializer::deserialize_with_limits(&mut reader, u16::MAX as u32, 2, None)
        .expect_err("C# rejects when deserialized item count grows past maxItems");

    assert_eq!(err.to_string(), "Too many items");
}

#[test]
fn nep17_data_array_roundtrips_losslessly() {
    // Reproduces the `data` payload pushed by tx
    // 0x4e2d76756fe4253ed19ae68a99b3557b2dedfa3e8e204fddf61163c9334a7e17
    // (mainnet block 676,050) where N3Trader's onNEP17Payment diverges.
    // data = [Integer(0), [BS(""), BS("")], [GAS, GAS], [Int(100M), Int(100M)],
    //        [Integer(0), Integer(1)], Integer(-1)]
    let limits = ExecutionEngineLimits::default();
    let gas_hash = hex::decode("cf76e28bd0062c4a478ee35561011319f3cfa4d2").expect("gas hash");

    let inner_hashes = StackItem::from_array(vec![
        StackItem::from_byte_string(gas_hash.clone()),
        StackItem::from_byte_string(gas_hash.clone()),
    ]);
    let inner_amounts = StackItem::from_array(vec![
        StackItem::from_int(100_000_000i64),
        StackItem::from_int(100_000_000i64),
    ]);
    let inner_empty = StackItem::from_array(vec![
        StackItem::from_byte_string(Vec::new()),
        StackItem::from_byte_string(Vec::new()),
    ]);
    let inner_zero_one =
        StackItem::from_array(vec![StackItem::from_int(0i64), StackItem::from_int(1i64)]);

    let data = StackItem::from_array(vec![
        StackItem::from_int(0i64),
        inner_empty.clone(),
        inner_hashes.clone(),
        inner_amounts.clone(),
        inner_zero_one.clone(),
        StackItem::from_int(-1i64),
    ]);

    let serialized = BinarySerializer::serialize(&data, &limits).expect("serialize");
    let deserialized =
        BinarySerializer::deserialize(&serialized, &limits, None).expect("deserialize");

    assert_eq!(
        deserialized.stack_item_type(),
        StackItemType::Array,
        "Top-level type must roundtrip as Array"
    );
    let arr = match &deserialized {
        StackItem::Array(a) => a.clone(),
        _ => panic!("Expected Array"),
    };
    assert_eq!(arr.len(), 6, "Array must have 6 elements");
    assert_eq!(arr.get(0).unwrap(), StackItem::from_int(0i64));
    assert_eq!(arr.get(5).unwrap(), StackItem::from_int(-1i64));

    // Re-serialize the deserialized form and confirm bytes match.
    let reserialized = BinarySerializer::serialize(&deserialized, &limits).expect("reserialize");
    assert_eq!(
        serialized, reserialized,
        "Roundtrip must produce identical bytes"
    );
}
