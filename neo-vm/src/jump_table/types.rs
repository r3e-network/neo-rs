//! Type operations for the Neo Virtual Machine.
//!
//! This module provides the type operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::{Array, StackItem, Struct};
use neo_vm_rs::Instruction;
use neo_vm_rs::StackItemType;
use neo_vm_rs::{OpCode, StackValue};

/// Registers the type operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
    register_jump_handlers![
        jump_table;
        OpCode::CONVERT => convert,
        OpCode::ISTYPE => is_type,
        OpCode::ISNULL => is_null,
    ];
}

/// Implements the CONVERT operation.
fn convert<S>(engine: &mut ExecutionEngine<S>, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

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
fn is_type<S>(engine: &mut ExecutionEngine<S>, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

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
    let result = neo_vm_rs::semantics::conversion::is_type(&probe, item_type.to_byte());

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
fn is_null<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the item on the stack
    let item = context.pop()?;

    let stack_value = match item {
        StackItem::Null => StackValue::Null,
        _ => StackValue::Boolean(true),
    };
    let result = neo_vm_rs::semantics::comparison::is_null(&stack_value);

    context.push(StackItem::from_bool(result))?;

    Ok(())
}
