//! Type operations for the Neo Virtual Machine.
//!
//! This module provides the type operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::{Array, StackItem, Struct};

/// Registers the type operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::CONVERT, convert);
    jump_table.register(OpCode::ISTYPE, is_type);
    jump_table.register(OpCode::ISNULL, is_null);
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

    // Pop the item from the stack
    let item = context.pop()?;

    if matches!(item, StackItem::Null) {
        context.push(StackItem::Null)?;
        return Ok(());
    }

    // Convert the item to the specified type
    let result = if item.stack_item_type() == item_type {
        item
    } else {
        match (item, item_type) {
            // Convert to Boolean
            (item, StackItemType::Boolean) => StackItem::from_bool(item.as_bool()?),

            // Convert to Integer
            (item, StackItemType::Integer) => StackItem::from_int(item.as_int()?),

            // Convert to ByteString
            (item, StackItemType::ByteString) => StackItem::from_byte_string(item.as_bytes()?),

            // Convert to Buffer
            (item, StackItemType::Buffer) => StackItem::from_buffer(item.as_bytes()?),

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

    let result = item.stack_item_type() == item_type;

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the ISNULL operation.
fn is_null(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the item on the stack
    let item = context.pop()?;

    let result = matches!(item, StackItem::Null);

    context.push(StackItem::from_bool(result))?;

    Ok(())
}
