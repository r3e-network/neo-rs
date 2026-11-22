use super::*;

/// Implements the APPEND operation.
pub(super) fn append(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let r1 = engine.reference_count();

    let mut item = engine.pop()?;
    let collection = engine.pop()?;
    if !matches!(collection, StackItem::Array(_) | StackItem::Struct(_)) {
        return Err(VmError::invalid_type_simple(
            "Expected Array, Struct, or Map",
        ));
    }

    let mut cloned_items = 0i64;
    if let StackItem::Struct(structure) = item {
        let (clone, cloned) = structure.clone_with_limits_counted(&limits)?;
        item = StackItem::Struct(clone);
        cloned_items = cloned;
    }

    // C# v3.11.2: `array.Add(newItem)`, then, if the array is stack
    // referenced, `AddStackReference(newItem)`.
    // APPEND pops both operands and does not push the array back (Pop 2, Push 0).
    let added = counted(&item);
    let referenced = match collection {
        StackItem::Array(array) => {
            array.push(item)?;
            array.is_stack_referenced()
        }
        StackItem::Struct(structure) => {
            structure.push(item)?;
            structure.is_stack_referenced()
        }
        _ => false,
    };
    let r2 = engine.reference_count();
    if referenced {
        engine.reference_counter().add_stack_reference(&added, 1);
    }
    engine.collect_ref_delta(r1 - r2 + engine.reference_count() - r2);
    engine.collect_cloned_items(cloned_items);

    Ok(())
}

/// Implements the REVERSE operation.
pub(super) fn reverse(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the collection from the stack
    let collection = engine.pop()?;

    // Reverse the collection and price its element count (C# `CollectOpLength(l)`).
    let (item_type, length) = match collection {
        StackItem::Array(array) => {
            array.reverse_items()?;
            (StackItemType::Array, array.len())
        }
        StackItem::Struct(structure) => {
            structure.reverse_items()?;
            (StackItemType::Struct, structure.len())
        }
        StackItem::Buffer(buffer) => {
            buffer.with_data_mut(|data| data.reverse());
            (StackItemType::Buffer, buffer.len())
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Buffer",
            ));
        }
    };

    engine.collect_type(item_type);
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(length as i64);

    Ok(())
}

/// Implements the REMOVE operation.
pub(super) fn remove(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the key and collection from the stack
    let key = engine.pop()?;
    let collection = engine.pop()?;

    // C# v3.11.2 prices the entries that shift down (`CollectOpLength(l)`).
    let mut length = 0i64;
    let item_type = match collection {
        StackItem::Array(array) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid array index"))?;
            if index >= array.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            // C# v3.10.1: `RemoveAt(i)` first, then release the removed item
            // if the array is stack referenced.
            let removed = array.remove(index)?;
            length = array.len() as i64 - index as i64;
            if array.is_stack_referenced() {
                engine.reference_counter().remove_stack_reference(&removed);
            }
            StackItemType::Array
        }
        StackItem::Struct(structure) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct index"))?;
            if index >= structure.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            let removed = structure.remove(index)?;
            length = structure.len() as i64 - index as i64;
            if structure.is_stack_referenced() {
                engine.reference_counter().remove_stack_reference(&removed);
            }
            StackItemType::Struct
        }
        StackItem::Map(map) => {
            // C# v3.10.1: `map.Remove(key)` first; a removed entry releases
            // its key and its value if the map is stack referenced.
            if let Some((index, removed)) = map.remove_entry_with_index(&key)? {
                length = map.len() as i64 - index as i64;
                if map.is_stack_referenced() {
                    let reference_counter = engine.reference_counter();
                    reference_counter.remove_stack_reference(&key);
                    reference_counter.remove_stack_reference(&removed);
                }
            }
            StackItemType::Map
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    };

    engine.collect_type(item_type);
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(length);

    Ok(())
}

/// Implements the CLEARITEMS operation.
pub(super) fn clear_items(
    engine: &mut ExecutionEngine,
    _instruction: &Instruction,
) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the collection from the stack
    let collection = engine.pop()?;

    // Clear the collection. C# v3.10.1: take the sub items, `Clear()`, then
    // release every sub item if the collection is stack referenced.
    let (sub_items, referenced) = match collection {
        StackItem::Array(array) => {
            let sub_items = array.sub_items();
            array.clear()?;
            (sub_items, array.is_stack_referenced())
        }
        StackItem::Struct(structure) => {
            let sub_items = structure.sub_items();
            structure.clear()?;
            (sub_items, structure.is_stack_referenced())
        }
        StackItem::Map(map) => {
            let sub_items = map.sub_items();
            map.clear()?;
            (sub_items, map.is_stack_referenced())
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    };
    if referenced {
        engine.reference_counter().remove_sub_items(sub_items);
    }
    engine.collect_ref_delta(r - engine.reference_count());

    Ok(())
}

/// Implements the POPITEM operation.
pub(super) fn pop_item(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r1 = engine.reference_count();

    // Pop the collection from the stack
    let collection = engine.pop()?;

    // C# v3.10.1: push the last item, `RemoveAt(index)`, then release the
    // removed item if the collection is stack referenced.
    let (item, _index, referenced) = match &collection {
        StackItem::Array(array) => {
            let index = array
                .len()
                .checked_sub(1)
                .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;
            let item = array
                .get(index)
                .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;
            (item, index, array.is_stack_referenced())
        }
        StackItem::Struct(structure) => {
            let index = structure
                .len()
                .checked_sub(1)
                .ok_or_else(|| VmError::invalid_operation_msg("Struct is empty"))?;
            let item = structure.get(index)?;
            (item, index, structure.is_stack_referenced())
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
    };

    let r2 = engine.reference_count();
    engine.push(item.clone())?;
    match &collection {
        StackItem::Array(array) => {
            array.pop()?;
        }
        StackItem::Struct(structure) => {
            structure.pop()?;
        }
        _ => unreachable!("collection was matched above"),
    }
    if referenced {
        engine.reference_counter().remove_stack_reference(&item);
    }
    engine.collect_ref_delta((r1 - r2) + (engine.reference_count() - r2));

    Ok(())
}

/// Implements the HASKEY operation.
pub(super) fn has_key(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();
    let pre_gorgon = engine.limits().pre_gorgon_collection_semantics;

    // Pop the key and collection from the stack
    let key = engine.pop()?;
    let collection = engine.pop()?;

    let result = match &collection {
        StackItem::Array(array) => {
            let index = has_key_collection_index(&key, pre_gorgon, "array")?;
            index < array.len()
        }
        StackItem::Struct(structure) => {
            let index = has_key_collection_index(&key, pre_gorgon, "struct")?;
            index < structure.len()
        }
        StackItem::Map(map) => map.contains_key(&key)?,
        StackItem::ByteString(bytes) => byte_sequence_has_key(
            crate::StackValue::ByteString(bytes.clone()),
            &key,
            pre_gorgon,
        )?,
        StackItem::Buffer(buffer) => {
            byte_sequence_has_key(crate::StackValue::Buffer(buffer.data()), &key, pre_gorgon)?
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, or Buffer",
            ));
        }
    };

    engine.push(StackItem::from_bool(result))?;
    engine.collect_ref_delta(r - engine.reference_count());

    Ok(())
}

/// Implements the KEYS operation.
pub(super) fn keys(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the map from the stack
    let collection = engine.pop()?;

    // Get the keys from the map
    let StackItem::Map(map) = collection else {
        return Err(VmError::invalid_type_simple("Expected Map"));
    };
    let count = map.len();
    let keys: Vec<StackItem> =
        map.with_items(|items| items.iter().map(|(k, _)| k.clone()).collect());
    let array = StackItem::Array(Array::new(keys));
    // C# v3.11.2: `array.StackReferences++`, price the delta and the key count,
    // then `PushItemCounted(array, map.Count + 1)`.
    add_raw_stack_references(&array, 1);
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(count as i64);
    engine.push_item_counted(array, count as i64 + 1)?;

    Ok(())
}

/// Implements the VALUES operation.
pub(super) fn values(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let item = engine.pop_no_ref()?;

    let mut cloned_items = 0i64;
    let mut refs_delta = 0i64;

    // C# v3.11.2: `x.StackReferences--` releases the popped slot without
    // touching the sub items; an unreferenced map additionally drops one
    // counter entry per key (`Dec(map.Count)`).
    let (values, is_referenced) = match &item {
        StackItem::Array(array) => {
            remove_raw_stack_references(&item, 1);
            (array.items(), array.is_stack_referenced())
        }
        StackItem::Struct(structure) => {
            remove_raw_stack_references(&item, 1);
            (structure.items(), structure.is_stack_referenced())
        }
        StackItem::Map(map) => {
            remove_raw_stack_references(&item, 1);
            let referenced = map.is_stack_referenced();
            if !referenced {
                // Decrease refcounter value by number of keys in map.
                engine.reference_counter().dec(map.len() as i64);
            }
            (
                map.with_items(|items| {
                    items
                        .iter()
                        .map(|(_, value)| value.clone())
                        .collect::<Vec<_>>()
                }),
                referenced,
            )
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "VALUES expects an Array, Struct, or Map",
            ));
        }
    };

    let new_array = Array::new(Vec::new());
    if is_referenced {
        // The source is still referenced, so every value (or its struct clone)
        // gains a stack reference of its own.
        let r = engine.reference_count();
        for value in values {
            let (cp_item, cloned) = match value {
                StackItem::Struct(structure) => {
                    let (clone, cloned) = structure.clone_with_limits_counted(&limits)?;
                    (StackItem::Struct(clone), cloned)
                }
                other => (other, 0),
            };
            new_array.push(cp_item.clone())?;
            cloned_items += cloned;
            engine.reference_counter().add_stack_reference(&cp_item, 1);
        }
        refs_delta = engine.reference_count() - r;
    } else {
        // The source is unreferenced, so its values carry no references of
        // their own: a cloned struct releases the original and keeps the
        // counter balanced with the clone's item count.
        for value in values {
            match value {
                StackItem::Struct(ref structure) => {
                    let (clone, cloned) = structure.clone_with_limits_counted(&limits)?;
                    let cp_item = StackItem::Struct(clone);
                    let r = engine.reference_count();
                    engine.reference_counter().remove_stack_reference(&value);
                    refs_delta += r - engine.reference_count();
                    engine.reference_counter().add_stack_reference(&cp_item, 1);
                    new_array.push(cp_item)?;
                    cloned_items += cloned;
                    refs_delta += cloned;
                }
                other => new_array.push(other)?,
            }
        }
    }

    let length = new_array.len();
    let new_item = StackItem::Array(new_array);
    add_raw_stack_references(&new_item, 1);
    engine.push_item_counted(new_item, 0)?;
    engine.collect_ref_delta(refs_delta);
    engine.collect_op_length(length as i64);
    engine.collect_cloned_items(cloned_items);

    Ok(())
}
/// Implements the PICKITEM operation.
pub(super) fn pick_item(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r1 = engine.reference_count();

    let key = engine.pop()?;
    let collection = engine.pop()?;

    let result = match collection {
        StackItem::Array(array) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < array.len() => array
                    .get(idx)
                    .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?,
                _ => {
                    // C# v3.11.2 throws through `ExecuteThrow` so the unwind's
                    // reference delta is part of this opcode's price.
                    let r3 = engine.reference_count();
                    throw_counted(
                        engine,
                        r1 - r3,
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = array.len()
                        ),
                    )?;
                    return Ok(());
                }
            }
        }
        StackItem::Struct(structure) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < structure.len() => structure.get(idx)?,
                _ => {
                    let r3 = engine.reference_count();
                    throw_counted(
                        engine,
                        r1 - r3,
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = structure.len()
                        ),
                    )?;
                    return Ok(());
                }
            }
        }
        StackItem::Map(map) => match map.try_get(&key)? {
            Some(value) => value,
            None => {
                let r3 = engine.reference_count();
                throw_counted(
                    engine,
                    r1 - r3,
                    format!("Key {} not found in Map.", key_text(&key)),
                )?;
                return Ok(());
            }
        },
        StackItem::ByteString(bytes) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < bytes.len() => {
                    pick_byte_sequence_item(crate::StackValue::ByteString(bytes.clone()), idx)?
                }
                _ => {
                    let r3 = engine.reference_count();
                    throw_counted(
                        engine,
                        r1 - r3,
                        format!(
                            "The index of PrimitiveType is out of range, {index}/[0, {count}).",
                            count = bytes.len()
                        ),
                    )?;
                    return Ok(());
                }
            }
        }
        // C# Neo VM PICKITEM on PrimitiveType reads the bytewise GetSpan()
        // representation. Use neo-vm-rs conversion rules so Boolean false
        // remains a one-byte span [0], matching C# Boolean.Memory.
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let value = crate::StackValue::try_from(item)?;
            let length = crate::semantics::collections::size(&value)
                .map_err(VmError::invalid_operation_msg)? as usize;
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < length => pick_byte_sequence_item(value, idx)?,
                _ => {
                    let r3 = engine.reference_count();
                    throw_counted(
                        engine,
                        r1 - r3,
                        format!(
                            "The index of PrimitiveType is out of range, {index}/[0, {length})."
                        ),
                    )?;
                    return Ok(());
                }
            }
        }
        StackItem::Buffer(buffer) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < buffer.len() => {
                    pick_byte_sequence_item(crate::StackValue::Buffer(buffer.data()), idx)?
                }
                _ => {
                    let r3 = engine.reference_count();
                    throw_counted(
                        engine,
                        r1 - r3,
                        format!(
                            "The index of Buffer is out of range, {index}/[0, {count}).",
                            count = buffer.len()
                        ),
                    )?;
                    return Ok(());
                }
            }
        }
        other => {
            return Err(VmError::invalid_type_simple(format!(
                "PICKITEM: unsupported {:?}",
                other.stack_item_type()
            )));
        }
    };

    let r2 = engine.reference_count();
    engine.push(result)?;
    engine.collect_ref_delta(r1 - r2 + engine.reference_count() - r2);

    Ok(())
}
/// Implements the SIZE operation.
pub(super) fn size(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the collection from the stack
    let collection = engine.pop()?;

    // Get the size of the collection.
    // C# Neo VM SIZE accepts CompoundType (Count), PrimitiveType (Size), and Buffer (Size).
    // PrimitiveType subclasses are ByteString, Integer, Boolean — all expose Size.
    let size = match collection {
        StackItem::Array(array) => array.len(),
        StackItem::Struct(structure) => structure.len(),
        StackItem::Map(map) => map.len(),
        StackItem::ByteString(data) => data.len(),
        StackItem::Buffer(data) => data.len(),
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let value = crate::StackValue::try_from(item)?;
            usize::try_from(
                crate::semantics::collections::size(&value)
                    .map_err(VmError::invalid_operation_msg)?,
            )
            .map_err(|_| VmError::invalid_operation_msg("Invalid primitive size"))?
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, Buffer, Integer, or Boolean",
            ));
        }
    };

    // Push the size onto the stack
    engine.push(StackItem::from_int(size))?;
    engine.collect_ref_delta(r - engine.reference_count());

    Ok(())
}
