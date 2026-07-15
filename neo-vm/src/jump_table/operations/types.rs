//! Type operations for the Neo Virtual Machine.
//!
//! This module provides the type operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::{Array, StackItem, Struct};

const VM_INTEGER_MAX_SIZE: usize = 32;

fn integer_memory(value: &crate::stack_item::VmInteger) -> Vec<u8> {
    if value.is_zero() {
        Vec::new()
    } else {
        value.to_signed_bytes_le()
    }
}

fn primitive_memory(item: StackItem) -> VmResult<Vec<u8>> {
    match item {
        StackItem::Boolean(value) => Ok(vec![u8::from(value)]),
        StackItem::Integer(value) => Ok(integer_memory(&value)),
        StackItem::ByteString(bytes) => Ok(bytes),
        StackItem::Buffer(buffer) => Ok(buffer.data()),
        other => Err(VmError::invalid_type_simple(format!(
            "Cannot convert {:?} to a byte sequence",
            other.stack_item_type()
        ))),
    }
}

fn boolean_value(item: StackItem) -> VmResult<bool> {
    match item {
        StackItem::Null => Ok(false),
        StackItem::Boolean(value) => Ok(value),
        StackItem::Integer(value) => Ok(!value.is_zero()),
        StackItem::ByteString(bytes) => {
            if bytes.len() > VM_INTEGER_MAX_SIZE {
                return Err(VmError::invalid_type_simple(
                    "Cannot convert ByteString to Boolean",
                ));
            }
            Ok(bytes.iter().any(|byte| *byte != 0))
        }
        StackItem::Buffer(_)
        | StackItem::Array(_)
        | StackItem::Struct(_)
        | StackItem::Map(_)
        | StackItem::Pointer(_)
        | StackItem::InteropInterface(_) => Ok(true),
    }
}

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
    let context = require_context(engine)?;
    let item = context.pop()?;

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

    if matches!(item, StackItem::Null) {
        context.push(StackItem::Null)?;
        return Ok(());
    }

    // Convert directly between local VM values. Mutable buffers and compound
    // values must retain their local identity when the type is unchanged.
    let result = if item.stack_item_type() == item_type {
        item
    } else {
        match (item, item_type) {
            (item, StackItemType::Boolean) => StackItem::from_bool(boolean_value(item)?),
            (item, StackItemType::Integer) => StackItem::from_int(item.into_int()?),
            (item, StackItemType::ByteString) => {
                StackItem::from_byte_string(primitive_memory(item)?)
            }
            (item, StackItemType::Buffer) => StackItem::from_buffer(primitive_memory(item)?),

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
    let context = require_context(engine)?;
    let item = context.pop()?;

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

    let result = item.stack_item_type() == item_type;

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the ISNULL operation.
fn is_null<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the item on the stack
    let item = context.pop()?;

    context.push(StackItem::from_bool(item.is_null()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;
    use crate::stack_item::Buffer;

    fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");
        let context = engine.current_context_mut().expect("current context");
        for item in items {
            context.push(item).expect("push test item");
        }
        engine
    }

    fn type_instruction(opcode: OpCode, item_type: StackItemType) -> Instruction {
        Instruction::new(opcode, &[item_type.to_byte()])
    }

    fn pop(engine: &mut ExecutionEngine) -> StackItem {
        engine
            .current_context_mut()
            .expect("current context")
            .pop()
            .expect("result item")
    }

    fn convert_item(item: StackItem, item_type: StackItemType) -> StackItem {
        let mut engine = engine_with_stack(vec![item]);
        convert(&mut engine, &type_instruction(OpCode::CONVERT, item_type))
            .expect("CONVERT succeeds");
        pop(&mut engine)
    }

    #[test]
    fn primitive_byte_conversions_match_neovm_memory() {
        assert!(matches!(
            convert_item(StackItem::from_i64(0), StackItemType::ByteString),
            StackItem::ByteString(bytes) if bytes.is_empty()
        ));
        assert!(matches!(
            convert_item(StackItem::from_bool(false), StackItemType::ByteString),
            StackItem::ByteString(bytes) if bytes == vec![0]
        ));
    }

    #[test]
    fn buffer_boolean_conversion_is_true_and_same_type_keeps_identity() {
        let buffer = Buffer::new(Vec::new());
        assert!(matches!(
            convert_item(StackItem::Buffer(buffer.clone()), StackItemType::Boolean),
            StackItem::Boolean(true)
        ));

        match convert_item(StackItem::Buffer(buffer.clone()), StackItemType::Buffer) {
            StackItem::Buffer(result) => assert_eq!(result.id(), buffer.id()),
            other => panic!("expected Buffer, got {other:?}"),
        }
    }

    #[test]
    fn array_to_struct_keeps_child_alias_and_tracks_new_container() {
        let child = Buffer::new(vec![7]);
        let array = StackItem::from_array(vec![StackItem::Buffer(child.clone())]);

        match convert_item(array, StackItemType::Struct) {
            StackItem::Struct(structure) => {
                assert!(structure.reference_counter().is_some());
                match structure.get(0).expect("struct child") {
                    StackItem::Buffer(result) => assert_eq!(result.id(), child.id()),
                    other => panic!("expected Buffer child, got {other:?}"),
                }
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    #[test]
    fn null_conversion_and_type_predicates_use_local_items() {
        assert!(matches!(
            convert_item(StackItem::Null, StackItemType::Map),
            StackItem::Null
        ));

        let mut engine = engine_with_stack(vec![StackItem::from_i64(1)]);
        is_type(
            &mut engine,
            &type_instruction(OpCode::ISTYPE, StackItemType::Integer),
        )
        .expect("ISTYPE succeeds");
        assert!(matches!(pop(&mut engine), StackItem::Boolean(true)));

        let mut engine = engine_with_stack(vec![StackItem::Null]);
        is_null(&mut engine, &Instruction::new(OpCode::ISNULL, &[])).expect("ISNULL succeeds");
        assert!(matches!(pop(&mut engine), StackItem::Boolean(true)));
    }
}
