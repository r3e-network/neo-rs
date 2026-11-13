//! Type operations for the Neo Virtual Machine.
//!
//! This module provides the type operation handlers for the Neo VM.

use crate::Instruction;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::stack_item::{Array, StackItem, Struct};
use crate::{OpCode, StackValue};

/// Registers the type operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    register_jump_handlers![
        jump_table;
        OpCode::CONVERT => convert,
        OpCode::ISTYPE => is_type,
        OpCode::ISNULL => is_null,
    ];
}

/// Implements the CONVERT operation.
fn convert(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    // Convert the type byte to a StackItemType
    let item_type = StackItemType::from_byte(type_byte)
        .ok_or_else(|| VmError::invalid_instruction_msg(format!("Invalid type: {type_byte}")))?;
    if item_type == StackItemType::Any {
        return Err(VmError::invalid_instruction_msg(format!(
            "Invalid type: {type_byte}"
        )));
    }

    // Null remains Null for every target type in the reference VM. A same-type
    // conversion likewise already has the required item on top of the stack.
    let item = {
        let mut stack = context.evaluation_stack_mut();
        let item = stack.peek(0)?;
        if matches!(item, StackItem::Null) || item.stack_item_type() == item_type {
            return Ok(());
        }
        stack.pop()?
    };

    // Convert the item to the specified type
    let result = {
        match (item, item_type) {
            // Keep primitive conversion rules centralized on StackItem; its
            // byte-target path delegates the exact safe subset to neo-vm-rs.
            (
                item,
                target_type @ (StackItemType::Boolean
                | StackItemType::Integer
                | StackItemType::ByteString
                | StackItemType::Buffer),
            ) => item.convert_to(target_type)?,

            // Convert to Array/Struct
            (StackItem::Struct(items), StackItemType::Array) => StackItem::Array(Array::new(
                items.into(),
                Some(context.reference_counter().clone()),
            )?),
            (StackItem::Array(items), StackItemType::Struct) => StackItem::Struct(Struct::new(
                items.into(),
                Some(context.reference_counter().clone()),
            )?),

            // Map, Pointer, InteropInterface conversions are only valid to the same type
            (item, target_type) => {
                return Err(VmError::invalid_type_simple(format!(
                    "Cannot convert {:?} to {:?}",
                    item.stack_item_type(),
                    target_type
                )));
            }
        }
    };

    context.push(result)?;

    Ok(())
}

/// Implements the ISTYPE operation.
fn is_type(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    // Convert the type byte to a StackItemType
    let item_type = StackItemType::from_byte(type_byte)
        .ok_or_else(|| VmError::invalid_instruction_msg(format!("Invalid type: {type_byte}")))?;
    if item_type == StackItemType::Any {
        return Err(VmError::invalid_instruction_msg(format!(
            "Invalid type: {type_byte}"
        )));
    }

    // Pop the item on the stack
    let item = context.pop()?;

    let probe = stack_item_type_probe_value(item.stack_item_type());
    let result = crate::semantics::conversion::is_type(&probe, item_type.to_byte());

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

fn stack_item_type_probe_value(item_type: StackItemType) -> StackValue {
    match item_type {
        StackItemType::Any => StackValue::Null,
        StackItemType::Pointer => StackValue::Pointer(0),
        StackItemType::Boolean => StackValue::Boolean(false),
        StackItemType::Integer => StackValue::Integer(0),
        StackItemType::ByteString => StackValue::ByteString(Vec::new()),
        StackItemType::Buffer => StackValue::Buffer(Vec::new()),
        StackItemType::Array => StackValue::Array(Vec::new()),
        StackItemType::Struct => StackValue::Struct(Vec::new()),
        StackItemType::Map => StackValue::Map(Vec::new()),
        StackItemType::InteropInterface => StackValue::Interop(0),
    }
}

/// Implements the ISNULL operation.
fn is_null(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the item on the stack
    let item = context.pop()?;

    let stack_value = match item {
        StackItem::Null => StackValue::Null,
        _ => StackValue::Boolean(true),
    };
    let result = crate::semantics::comparison::is_null(&stack_value);

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");
        let context = engine.current_context_mut().expect("current context");
        for item in items {
            context.push(item).expect("push test item");
        }
        engine
    }

    fn conversion(target: StackItemType) -> Instruction {
        Instruction::new(OpCode::CONVERT, &[target.to_byte()])
    }

    fn converted(item: StackItem, target: StackItemType) -> VmResult<StackItem> {
        let mut engine = engine_with_stack(vec![item]);
        convert(&mut engine, &conversion(target))?;
        engine.current_context_mut().expect("current context").pop()
    }

    #[test]
    fn convert_same_type_preserves_buffer_and_array_identity() {
        let buffer = StackItem::from_buffer(vec![1, 2, 3]);
        let buffer_id = match &buffer {
            StackItem::Buffer(value) => value.id(),
            _ => unreachable!(),
        };
        let result = converted(buffer, StackItemType::Buffer).expect("buffer conversion");
        assert_eq!(result.stack_item_type(), StackItemType::Buffer);
        match result {
            StackItem::Buffer(value) => assert_eq!(value.id(), buffer_id),
            _ => unreachable!(),
        }

        let array = StackItem::from_array(vec![StackItem::from_i64(42)]);
        let array_id = match &array {
            StackItem::Array(value) => value.id(),
            _ => unreachable!(),
        };
        let result = converted(array, StackItemType::Array).expect("array conversion");
        assert_eq!(result.stack_item_type(), StackItemType::Array);
        match result {
            StackItem::Array(value) => assert_eq!(value.id(), array_id),
            _ => unreachable!(),
        }
    }

    #[test]
    fn convert_null_remains_null_for_every_target_type() {
        for target in [
            StackItemType::Boolean,
            StackItemType::Integer,
            StackItemType::ByteString,
            StackItemType::Buffer,
            StackItemType::Array,
            StackItemType::Struct,
            StackItemType::Map,
            StackItemType::Pointer,
            StackItemType::InteropInterface,
        ] {
            assert_eq!(
                converted(StackItem::Null, target).expect("Null conversion must succeed"),
                StackItem::Null,
                "Null conversion changed for {target:?}"
            );
        }
    }

    #[test]
    fn convert_real_conversion_and_empty_stack_errors_are_unchanged() {
        let result = converted(StackItem::from_i64(42), StackItemType::ByteString)
            .expect("integer conversion");
        assert_eq!(result, StackItem::ByteString(vec![42]));

        let mut engine = engine_with_stack(Vec::new());
        assert!(convert(&mut engine, &conversion(StackItemType::Boolean)).is_err());
    }
}
