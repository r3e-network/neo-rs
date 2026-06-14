#![allow(clippy::mutable_key_type)]

//! BinarySerializer - aligns with `Neo.SmartContract.BinarySerializer`.

use neo_error::{CoreError, CoreResult};
use neo_io::var_int::VarInt;
use neo_io::{IoError, MemoryReader};
use neo_vm::StackItem;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackItemType;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use std::collections::{HashSet, VecDeque};
/// Binary serializer helpers for VM stack items.
pub struct BinarySerializer;

#[derive(Debug, Clone, Copy)]
struct ContainerDescriptor {
    item_type: StackItemType,
    element_count: usize,
}

#[derive(Debug)]
enum PendingItem {
    Value(StackItem),
    Container(ContainerDescriptor),
}

#[derive(Debug, Clone, Copy)]
enum StackValueContainerKind {
    Array,
    Struct,
    Map,
}

#[derive(Debug, Clone, Copy)]
struct StackValueContainerDescriptor {
    kind: StackValueContainerKind,
    element_count: usize,
}

#[derive(Debug)]
enum PendingStackValue {
    Value(StackValue),
    Container(StackValueContainerDescriptor),
}

impl BinarySerializer {
    const MAX_INTEGER_SIZE: usize = 32;
    const DEFAULT_MAX_ITEM_SIZE: usize = u16::MAX as usize;

    /// Deserialize a [`StackItem`] from the provided buffer using VM limits.
    pub fn deserialize(
        data: &[u8],
        limits: &ExecutionEngineLimits,
        _reference_counter: Option<ReferenceCounter>,
    ) -> CoreResult<StackItem> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize_with_limits(
            &mut reader,
            limits.max_item_size,
            limits.max_stack_size,
            _reference_counter,
        )
    }

    /// Deserialize a [`StackItem`] from the provided buffer using default VM limits.
    pub fn deserialize_default(data: &[u8]) -> CoreResult<StackItem> {
        Self::deserialize(data, &ExecutionEngineLimits::default(), None)
    }

    /// Deserialize using explicit limits (mirrors the C# overload).
    pub fn deserialize_with_limits(
        reader: &mut MemoryReader<'_>,
        max_size: u32,
        max_items: u32,
        _reference_counter: Option<ReferenceCounter>,
    ) -> CoreResult<StackItem> {
        let mut pending: Vec<PendingItem> = Vec::new();
        let mut remaining = 1usize;
        let mut total_items = 0usize;

        while remaining > 0 {
            remaining -= 1;
            if total_items >= max_items as usize {
                return Err(CoreError::other("Too many items"));
            }

            let item_type_byte = reader.read_byte().map_err(Self::io_error_to_core_error)?;
            let item_type = StackItemType::from_byte(item_type_byte).ok_or_else(|| {
                CoreError::other(format!("Unknown stack item type: 0x{item_type_byte:02x}"))
            })?;

            match item_type {
                StackItemType::Any => {
                    pending.push(PendingItem::Value(StackItem::null()));
                    total_items += 1;
                }
                StackItemType::Boolean => {
                    let value = reader
                        .read_boolean()
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_bool(value)));
                    total_items += 1;
                }
                StackItemType::Integer => {
                    let bytes = reader
                        .read_var_bytes(Self::MAX_INTEGER_SIZE)
                        .map_err(Self::io_error_to_core_error)?;
                    let value = if bytes.is_empty() {
                        BigInt::from(0)
                    } else {
                        BigInt::from_signed_bytes_le(&bytes)
                    };
                    pending.push(PendingItem::Value(StackItem::from_int(value)));
                    total_items += 1;
                }
                StackItemType::ByteString => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_byte_string(bytes)));
                    total_items += 1;
                }
                StackItemType::Buffer => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_buffer(bytes)));
                    total_items += 1;
                }
                StackItemType::Array | StackItemType::Struct => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    if count > (max_items as usize).saturating_sub(total_items) {
                        return Err(CoreError::other("Too many items"));
                    }
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining += count;
                    total_items += count + 1;
                }
                StackItemType::Map => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    if count > (max_items as usize).saturating_sub(total_items) {
                        return Err(CoreError::other("Too many items"));
                    }
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining += count * 2;
                    total_items += count * 2 + 1;
                }
                _ => return Err(CoreError::other("Unsupported stack item type")),
            }

            if pending.len() > max_items as usize {
                return Err(CoreError::other("Too many items"));
            }
        }

        let mut constructed: Vec<StackItem> = Vec::new();
        while let Some(item) = pending.pop() {
            match item {
                PendingItem::Value(stack_item) => constructed.push(stack_item),
                PendingItem::Container(container) => {
                    let result = match container.item_type {
                        StackItemType::Array => {
                            let mut elements = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                elements.push(constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized array data")
                                })?);
                            }
                            StackItem::from_array(elements)
                        }
                        StackItemType::Struct => {
                            let mut elements = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                elements.push(constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized struct data")
                                })?);
                            }
                            StackItem::from_struct(elements)
                        }
                        StackItemType::Map => {
                            let mut entries = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                let key = constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized map key")
                                })?;
                                let value = constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized map value")
                                })?;
                                entries.push((key, value));
                            }
                            let mut dict = neo_vm_rs::VmOrderedDictionary::new();
                            for (key, value) in entries {
                                dict.insert(key, value);
                            }
                            StackItem::from_map(dict)
                        }
                        _ => return Err(CoreError::other("Invalid container descriptor")),
                    };
                    constructed.push(result);
                }
            }
        }

        constructed
            .pop()
            .ok_or_else(|| CoreError::other("Empty serialization payload"))
    }

    /// Deserialize a binary-serialized stack item into the shared `neo-vm-rs` value type.
    ///
    /// This is intended for callers that need to inspect persisted stack payloads
    /// but do not need local VM object identity or reference counting.
    pub fn deserialize_stack_value(data: &[u8]) -> CoreResult<StackValue> {
        Self::deserialize_stack_value_with_limits(
            data,
            Self::DEFAULT_MAX_ITEM_SIZE,
            neo_vm_rs::DEFAULT_MAX_STACK_DEPTH,
        )
    }

    /// Deserialize a binary-serialized stack item into `neo_vm_rs::StackValue` with explicit limits.
    pub fn deserialize_stack_value_with_limits(
        data: &[u8],
        max_size: usize,
        max_items: usize,
    ) -> CoreResult<StackValue> {
        let mut reader = MemoryReader::new(data);
        let mut pending: Vec<PendingStackValue> = Vec::new();
        let mut remaining = 1usize;
        let mut total_items = 0usize;

        while remaining > 0 {
            remaining -= 1;
            if total_items >= max_items {
                return Err(CoreError::other("Too many items"));
            }
            total_items += 1;

            let item_type = reader.read_byte().map_err(Self::io_error_to_core_error)?;
            match item_type {
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY => {
                    pending.push(PendingStackValue::Value(StackValue::Null));
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN => {
                    let value = reader
                        .read_boolean()
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingStackValue::Value(StackValue::Boolean(value)));
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER => {
                    let bytes = reader
                        .read_var_bytes(Self::MAX_INTEGER_SIZE)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingStackValue::Value(StackValue::BigInteger(bytes)));
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING => {
                    let bytes = reader
                        .read_var_bytes(max_size)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingStackValue::Value(StackValue::ByteString(bytes)));
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BUFFER => {
                    let bytes = reader
                        .read_var_bytes(max_size)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingStackValue::Value(StackValue::Buffer(bytes)));
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY
                | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    if count > max_items.saturating_sub(total_items) {
                        return Err(CoreError::other("Too many items"));
                    }
                    let kind = if item_type == neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY {
                        StackValueContainerKind::Array
                    } else {
                        StackValueContainerKind::Struct
                    };
                    pending.push(PendingStackValue::Container(
                        StackValueContainerDescriptor {
                            kind,
                            element_count: count,
                        },
                    ));
                    remaining = remaining
                        .checked_add(count)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                }
                neo_vm_rs::NEOVM_STACK_ITEM_TYPE_MAP => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    let child_count = count
                        .checked_mul(2)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                    if child_count > max_items.saturating_sub(total_items) {
                        return Err(CoreError::other("Too many items"));
                    }
                    pending.push(PendingStackValue::Container(
                        StackValueContainerDescriptor {
                            kind: StackValueContainerKind::Map,
                            element_count: count,
                        },
                    ));
                    remaining = remaining
                        .checked_add(child_count)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                }
                _ => return Err(CoreError::other("Unsupported stack item type")),
            }

            if pending.len() > max_items {
                return Err(CoreError::other("Too many items"));
            }
        }

        let mut constructed: Vec<StackValue> = Vec::new();
        while let Some(item) = pending.pop() {
            match item {
                PendingStackValue::Value(value) => constructed.push(value),
                PendingStackValue::Container(container) => match container.kind {
                    StackValueContainerKind::Array | StackValueContainerKind::Struct => {
                        let mut elements = Vec::with_capacity(container.element_count);
                        for _ in 0..container.element_count {
                            elements.push(constructed.pop().ok_or_else(|| {
                                CoreError::other("Invalid serialized array data")
                            })?);
                        }
                        if matches!(container.kind, StackValueContainerKind::Array) {
                            constructed.push(StackValue::Array(elements));
                        } else {
                            constructed.push(StackValue::Struct(elements));
                        }
                    }
                    StackValueContainerKind::Map => {
                        let mut entries = Vec::with_capacity(container.element_count);
                        for _ in 0..container.element_count {
                            let key = constructed
                                .pop()
                                .ok_or_else(|| CoreError::other("Invalid serialized map key"))?;
                            let value = constructed
                                .pop()
                                .ok_or_else(|| CoreError::other("Invalid serialized map value"))?;
                            entries.push((key, value));
                        }
                        constructed.push(StackValue::Map(entries));
                    }
                },
            }
        }

        constructed
            .pop()
            .ok_or_else(|| CoreError::other("Empty serialization payload"))
    }

    /// Serialize a stack item with VM limits.
    pub fn serialize(item: &StackItem, limits: &ExecutionEngineLimits) -> CoreResult<Vec<u8>> {
        Self::serialize_with_limits(
            item,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
    }

    /// Serialize a shared `neo-vm-rs` stack value with VM limits.
    ///
    /// This keeps callers that only need ABI/storage value projection from
    /// depending on local VM stack item constructors.
    pub fn serialize_stack_value(
        value: &StackValue,
        limits: &ExecutionEngineLimits,
    ) -> CoreResult<Vec<u8>> {
        let item =
            StackItem::try_from(value.clone()).map_err(|err| CoreError::other(err.to_string()))?;
        Self::serialize(&item, limits)
    }

    /// Serialize a stack item with default VM limits.
    pub fn serialize_default(item: &StackItem) -> CoreResult<Vec<u8>> {
        Self::serialize(item, &ExecutionEngineLimits::default())
    }

    /// Serialize a shared `neo-vm-rs` stack value with default VM limits.
    pub fn serialize_stack_value_default(value: &StackValue) -> CoreResult<Vec<u8>> {
        Self::serialize_stack_value(value, &ExecutionEngineLimits::default())
    }

    /// Serialize a stack item using explicit limits.
    pub fn serialize_with_limits(
        item: &StackItem,
        max_size: usize,
        max_items: usize,
    ) -> CoreResult<Vec<u8>> {
        let mut writer = Vec::new();
        Self::serialize_into(item, &mut writer, max_size, max_items)?;
        Ok(writer)
    }

    /// Serialize into the provided writer buffer.
    pub fn serialize_into(
        item: &StackItem,
        writer: &mut Vec<u8>,
        max_size: usize,
        max_items: usize,
    ) -> CoreResult<()> {
        let mut processed: HashSet<(usize, StackItemType)> = HashSet::new();
        let mut queue: VecDeque<StackItem> = VecDeque::new();
        queue.push_back(item.clone());
        let mut remaining = max_items as isize;

        while let Some(current) = queue.pop_back() {
            remaining -= 1;
            if remaining < 0 {
                return Err(CoreError::other("Too many items"));
            }

            match &current {
                StackItem::Null => {
                    writer.push(StackItemType::Any.to_byte());
                }
                StackItem::Boolean(value) => {
                    writer.push(StackItemType::Boolean.to_byte());
                    writer.push(if *value { 1 } else { 0 });
                }
                StackItem::Integer(integer) => {
                    writer.push(StackItemType::Integer.to_byte());
                    let bytes = if integer.is_zero() {
                        Vec::new()
                    } else {
                        integer.to_signed_bytes_le()
                    };
                    VarInt::write_var_bytes(&bytes, writer);
                }
                StackItem::ByteString(bytes) => {
                    writer.push(StackItemType::ByteString.to_byte());
                    VarInt::write_var_bytes(bytes, writer);
                }
                StackItem::Buffer(buffer) => {
                    writer.push(StackItemType::Buffer.to_byte());
                    VarInt::write_var_bytes(&buffer.data(), writer);
                }
                StackItem::Array(array) => {
                    writer.push(StackItemType::Array.to_byte());
                    let identity = (array.id(), StackItemType::Array);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing array",
                        ));
                    }
                    VarInt::write_var_int(array.len() as u64, writer);
                    for element in array.iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Struct(struct_item) => {
                    writer.push(StackItemType::Struct.to_byte());
                    let identity = (struct_item.id(), StackItemType::Struct);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing struct",
                        ));
                    }
                    VarInt::write_var_int(struct_item.len() as u64, writer);
                    for element in struct_item.iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Map(map) => {
                    writer.push(StackItemType::Map.to_byte());
                    let identity = (map.id(), StackItemType::Map);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing map",
                        ));
                    }
                    VarInt::write_var_int(map.len() as u64, writer);
                    for (key, value) in map.iter().rev() {
                        queue.push_back(value.clone());
                        queue.push_back(key.clone());
                    }
                }
                _ => return Err(CoreError::other("Unsupported stack item type")),
            }

            if writer.len() > max_size {
                return Err(CoreError::other("Serialized data exceeds limit"));
            }
        }

        Ok(())
    }

    fn io_error_to_core_error(error: IoError) -> CoreError {
        CoreError::other(match error {
            IoError::Format => "format error".to_string(),
            IoError::InvalidUtf8 => "invalid utf-8 data".to_string(),
            IoError::InvalidData { context, value } => format!("{context}: {value}"),
            IoError::Io(inner) => inner.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm::stack_item::Map as MapItem;
    use neo_vm_rs::ExecutionEngineLimits;

    #[test]
    fn deserialize_preserves_map_entry_order_for_roundtrip_bytes() {
        let limits = ExecutionEngineLimits::default();

        // Serialize a map with specific insertion order: (3,30), (1,10), (2,20)
        let mut map_items = neo_vm_rs::VmOrderedDictionary::new();
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

        assert_eq!(
            value,
            StackValue::Struct(vec![
                StackValue::BigInteger(vec![42]),
                StackValue::ByteString(vec![1, 2, 3]),
                StackValue::Boolean(true),
            ])
        );
    }

    #[test]
    fn deserialize_stack_value_enforces_item_limits() {
        let payload = vec![neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY, 3, 0, 0, 0];

        let err =
            BinarySerializer::deserialize_stack_value_with_limits(&payload, u16::MAX as usize, 3)
                .expect_err("limit error");

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
        let reserialized =
            BinarySerializer::serialize(&deserialized, &limits).expect("reserialize");
        assert_eq!(
            serialized, reserialized,
            "Roundtrip must produce identical bytes"
        );
    }
}
