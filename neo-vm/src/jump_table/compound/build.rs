use super::*;

/// Implements the NEWARRAY0 operation.
pub(super) fn new_array0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let array = collection_stack_item(crate::semantics::collections::new_array(0, max_items))?;
    context.push(array)?;

    Ok(())
}

/// Implements the NEWARRAY operation.
pub(super) fn new_array(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;

    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    let array = collection_stack_item(crate::semantics::collections::new_array(count, max_items))?;
    // C# v3.11.2: `newArray.StackReferences++`, then
    // `PushItemCounted(newArray, n + 1)`, so the counter grows by the array plus
    // its `n` null slots exactly once and the array stays stack referenced.
    add_raw_stack_references(&array, 1);
    engine.push_item_counted(array, count + 1)?;
    engine.collect_type(StackItemType::Any);
    engine.collect_op_length(count);

    Ok(())
}

/// Implements the `NewarrayT` operation.
pub(super) fn new_array_t(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;

    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    let Some(item_type) = StackItemType::from_byte(type_byte) else {
        return Err(VmError::invalid_instruction_msg(format!(
            "Invalid type: {type_byte:#04x}"
        )));
    };

    let array = collection_stack_item(crate::semantics::collections::new_array_t(
        count, type_byte, max_items,
    ))?;
    add_raw_stack_references(&array, 1);
    engine.push_item_counted(array, count + 1)?;
    engine.collect_type(item_type);
    engine.collect_op_length(count);

    Ok(())
}

/// Implements the NEWSTRUCT0 operation.
pub(super) fn new_struct0(
    engine: &mut ExecutionEngine,
    _instruction: &Instruction,
) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let structure = collection_stack_item(crate::semantics::collections::new_struct(0, max_items))?;
    context.push(structure)?;

    Ok(())
}

/// Implements the NEWSTRUCT operation.
pub(super) fn new_struct(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;

    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;

    let structure =
        collection_stack_item(crate::semantics::collections::new_struct(count, max_items))?;
    add_raw_stack_references(&structure, 1);
    engine.push_item_counted(structure, count + 1)?;
    engine.collect_type(StackItemType::Any);
    engine.collect_op_length(count);

    Ok(())
}

/// Implements the NEWMAP operation.
pub(super) fn new_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let map_value = crate::semantics::collections::pack_map(Vec::new());
    let map = collection_stack_item(Ok(map_value))?;
    context.push(map)?;

    Ok(())
}
/// Implements the PACKMAP operation.
pub(super) fn pack_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let r = engine.reference_count();

    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid map size"))?;
    if count < 0 {
        return Err(VmError::invalid_operation_msg("Invalid map size"));
    }
    let count = count as usize;
    let required = count
        .checked_mul(2)
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid map size"))?;
    let stack_len = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?
        .evaluation_stack()
        .len();
    if required > stack_len {
        return Err(VmError::stack_underflow_msg(required, stack_len));
    }

    let map_item = Map::new(crate::VmOrderedDictionary::new());

    for _ in 0..count {
        // C# v3.11.2 pops both operands without releasing their references.
        let key = engine.pop_no_ref()?;
        let value = engine.pop_no_ref()?;
        // A duplicate key releases the key and the value it replaces.
        if let Some(old_value) = map_item.try_get(&key)? {
            let reference_counter = engine.reference_counter();
            reference_counter.remove_stack_reference(&key);
            reference_counter.remove_stack_reference(&old_value);
        }
        map_item.set(key, value)?;
    }

    let map = StackItem::Map(map_item);
    add_raw_stack_references(&map, 1);
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(count as i64);
    engine.push_item_counted(map, 1)?;

    Ok(())
}

/// Implements the PACKSTRUCT operation.
pub(super) fn pack_struct(
    engine: &mut ExecutionEngine,
    _instruction: &Instruction,
) -> VmResult<()> {
    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;
    if count < 0 {
        return Err(VmError::invalid_operation_msg("Invalid struct size"));
    }
    let count = count as usize;
    let stack_len = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?
        .evaluation_stack()
        .len();
    if count > stack_len {
        return Err(VmError::stack_underflow_msg(count, stack_len));
    }

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(engine.pop_no_ref()?);
    }

    let structure = StackItem::Struct(Struct::new(items));
    add_raw_stack_references(&structure, 1);
    engine.push_item_counted(structure, 1)?;
    engine.collect_op_length(count as i64);

    Ok(())
}

/// Implements the PACK operation.
pub(super) fn pack(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Pop the count from the stack
    let count = engine
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;
    if count < 0 {
        return Err(VmError::invalid_operation_msg("Invalid array size"));
    }
    let count = count as usize;
    let stack_len = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?
        .evaluation_stack()
        .len();
    if count > stack_len {
        return Err(VmError::stack_underflow_msg(count, stack_len));
    }

    // Create a new array
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(engine.pop_no_ref()?);
    }

    let array = StackItem::Array(Array::new(items));
    add_raw_stack_references(&array, 1);
    engine.push_item_counted(array, 1)?;
    engine.collect_op_length(count as i64);

    Ok(())
}

/// Implements the UNPACK operation.
pub(super) fn unpack(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Pop the compound from the stack without releasing its reference
    let compound = engine.pop_no_ref()?;

    // C# v3.11.2: `compound.StackReferences--` releases the popped slot without
    // touching the sub items, and `RemoveStackReference(StackItem.Null)` drops
    // the slot itself; every item is re-counted as it is pushed below.
    remove_raw_stack_references(&compound, 1);
    engine
        .reference_counter()
        .remove_stack_reference(&StackItem::Null);

    // Unpack the compound - collect items first to avoid holding locks during push
    let length;
    match &compound {
        StackItem::Map(map) => {
            length = map.len();
            let referenced = map.is_stack_referenced();
            let pairs: Vec<(StackItem, StackItem)> = map.with_items(|items| {
                items
                    .iter()
                    .rev()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            });
            for (key, value) in pairs {
                if referenced {
                    engine.push(value)?;
                    engine.push_item_counted(key, 1)?;
                } else {
                    engine.push_item_counted(value, 0)?;
                    engine.push_item_counted(key, 0)?;
                }
            }
        }
        StackItem::Array(array) => {
            length = array.len();
            let referenced = array.is_stack_referenced();
            let items: Vec<StackItem> =
                array.with_items(|items| items.iter().rev().cloned().collect());
            for item in items {
                if referenced {
                    engine.push(item)?;
                } else {
                    engine.push_item_counted(item, 0)?;
                }
            }
        }
        StackItem::Struct(structure) => {
            length = structure.len();
            let referenced = structure.is_stack_referenced();
            let items: Vec<StackItem> =
                structure.with_items(|items| items.iter().rev().cloned().collect());
            for item in items {
                if referenced {
                    engine.push(item)?;
                } else {
                    engine.push_item_counted(item, 0)?;
                }
            }
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
    }

    engine.push(StackItem::from_int(length))?;
    engine.collect_op_length(length as i64);

    Ok(())
}
