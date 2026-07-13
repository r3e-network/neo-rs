use super::*;
use neo_vm::ExecutionEngineLimits;
use neo_vm::stack_item::Map as MapItem;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm::StackValue, b: &neo_vm::StackValue) -> bool {
    use neo_vm::StackValue::*;
    match (a, b) {
        (Buffer(_, x), Buffer(_, y)) => x == y,
        (Array(_, x), Array(_, y)) | (Struct(_, x), Struct(_, y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(_, x), Map(_, y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

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
fn serialize_zero_integer_uses_empty_payload() {
    let limits = ExecutionEngineLimits::default();
    let serialized = BinarySerializer::serialize(&StackItem::from_int(0), &limits).unwrap();
    assert_eq!(serialized, vec![StackItemType::Integer.to_byte(), 0]);
}

#[test]
fn deserialize_stack_value_reads_storage_payload_without_local_stack_item() {
    let limits = ExecutionEngineLimits::default();
    let item = StackItem::from_struct(vec![
        StackItem::from_int(42i64),
        StackItem::from_byte_string(vec![1, 2, 3]),
        StackItem::from_bool(true),
    ]);
    let serialized = BinarySerializer::serialize(&item, &limits).expect("serialize");

    let value = BinarySerializer::deserialize_stack_value(&serialized).expect("deserialize");

    let expected = StackValue::Struct(
        neo_vm::next_stack_item_id(),
        vec![
            StackValue::BigInteger(vec![42]),
            StackValue::ByteString(vec![1, 2, 3]),
            StackValue::Boolean(true),
        ],
    );
    assert!(
        stack_value_struct_eq(&value, &expected),
        "structural StackValue mismatch: {value:?} vs {expected:?}"
    );
}

#[test]
fn deserialize_stack_value_enforces_item_limits() {
    let payload = vec![neo_vm::NEOVM_STACK_ITEM_TYPE_ARRAY, 3, 0, 0, 0];

    let err = BinarySerializer::deserialize_stack_value_with_limits(&payload, u16::MAX as usize, 3)
        .expect_err("limit error");

    assert_eq!(err.to_string(), "Too many items");
}

#[test]
fn serialize_stack_value_with_limits_matches_stack_item_and_enforces_size() {
    let value = StackValue::Array(
        neo_vm::next_stack_item_id(),
        vec![
            StackValue::ByteString(vec![1, 2, 3]),
            StackValue::BigInteger(BigInt::from(42).to_signed_bytes_le()),
        ],
    );
    let legacy = StackItem::from_array(vec![
        StackItem::from_byte_string(vec![1, 2, 3]),
        StackItem::from_int(42),
    ]);
    let expected = BinarySerializer::serialize_with_limits(&legacy, u16::MAX as usize, 16).unwrap();

    assert_eq!(
        BinarySerializer::serialize_stack_value_with_limits(&value, u16::MAX as usize, 16).unwrap(),
        expected
    );
    let err = BinarySerializer::serialize_stack_value_with_limits(&value, 2, 16)
        .expect_err("serialized byte limit");
    assert_eq!(err.to_string(), "Serialized data exceeds limit");
}

#[test]
fn serialize_stack_value_with_limits_preserves_stack_item_parity_without_runtime_handles() {
    let value = StackValue::Map(
        neo_vm::next_stack_item_id(),
        vec![(
            StackValue::ByteString(b"k".to_vec()),
            StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::Integer(-1),
                    StackValue::BigInteger(vec![0x00]),
                    StackValue::Array(
                        neo_vm::next_stack_item_id(),
                        vec![StackValue::Boolean(true), StackValue::Null],
                    ),
                ],
            ),
        )],
    );
    let legacy = StackItem::from_map({
        let mut map = neo_vm::VmOrderedDictionary::new();
        map.insert(
            StackItem::from_byte_string(b"k".to_vec()),
            StackItem::from_struct(vec![
                StackItem::from_int(-1),
                StackItem::from_int(0),
                StackItem::from_array(vec![StackItem::from_bool(true), StackItem::null()]),
            ]),
        );
        map
    });
    let expected = BinarySerializer::serialize_with_limits(&legacy, u16::MAX as usize, 16).unwrap();

    assert_eq!(
        BinarySerializer::serialize_stack_value_with_limits(&value, u16::MAX as usize, 16).unwrap(),
        expected
    );

    let err = BinarySerializer::serialize_stack_value_with_limits(
        &StackValue::Interop(7),
        u16::MAX as usize,
        16,
    )
    .expect_err("runtime handles are not serializable");
    assert!(err.to_string().contains("Unsupported stack value type"));
}

#[test]
fn serialize_stack_value_with_limits_is_direct_stack_value_serializer() {
    let source = include_str!("../../codec/binary_serializer.rs");
    let start = source
        .find("pub fn serialize_stack_value_with_limits(")
        .expect("stack value serializer exists");
    let end = source[start..]
        .find("/// Serialize a stack item using explicit limits.")
        .map(|offset| start + offset)
        .expect("stack item serializer follows stack value serializer");
    let helper = &source[start..end];

    assert!(!helper.contains("StackItem::try_from"));
    assert!(!helper.contains("serialize_with_limits(&item"));
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
