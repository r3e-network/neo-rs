//! Type operations for the Neo Virtual Machine.
//!
//! This module provides the type operation handlers for the Neo VM.

use crate::Instruction;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::run_stats::StackItemType as RunStatsItemType;
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
    // v3.11.2 Convert: r1 = Count; x = Pop(); fromType; toType; r2 = Count;
    // Push(x.ConvertTo(toType)); then collect the combined pop+push ref delta,
    // the converted type, and its length. A failing conversion faults after the
    // pop and before any collection.
    let r1 = engine.reference_count();

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    // Convert the type byte to a StackItemType
    let item_type = StackItemType::from_byte(type_byte)
        .ok_or_else(|| VmError::invalid_instruction_msg(format!("Invalid type: {type_byte}")))?;

    // Pop the item on the stack
    let item = engine.pop()?;
    let r2 = engine.reference_count();
    let from_type = item.stack_item_type();

    // Null remains Null for every target type in the reference VM, and `Any`
    // and same-type conversions are identities (ConvertTo returns the item
    // itself), so the popped item is pushed back unchanged.
    let result = if matches!(item, StackItem::Null)
        || item_type == StackItemType::Any
        || from_type == item_type
    {
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
            (StackItem::Struct(items), StackItemType::Array) => {
                StackItem::Array(Array::new(items.into()))
            }
            (StackItem::Array(items), StackItemType::Struct) => {
                StackItem::Struct(Struct::new(items.into()))
            }

            // Map, Pointer, InteropInterface conversions are only valid to the same type
            (item, target_type) => {
                return Err(VmError::invalid_type_simple(format!(
                    "Cannot convert {:?} to {target_type:?}",
                    item.stack_item_type()
                )));
            }
        }
    };

    // v3.11.2 records the type and length only for the compound and byte-memory
    // cross-conversions, measured on the original item (the converted item
    // carries the same count/size).
    let (converted_type, length) = if (from_type == StackItemType::Array
        && item_type == StackItemType::Struct)
        || (from_type == StackItemType::Struct && item_type == StackItemType::Array)
    {
        let count = match &result {
            StackItem::Array(items) => items.len() as i64,
            StackItem::Struct(items) => items.len() as i64,
            _ => 0,
        };
        (RunStatsItemType::Array, count)
    } else if (from_type == StackItemType::ByteString && item_type == StackItemType::Buffer)
        || (from_type == StackItemType::Buffer && item_type == StackItemType::ByteString)
    {
        let size = match &result {
            StackItem::ByteString(bytes) => bytes.len() as i64,
            StackItem::Buffer(buffer) => buffer.len() as i64,
            _ => 0,
        };
        (RunStatsItemType::ByteString, size)
    } else {
        (RunStatsItemType::Any, 0)
    };

    engine.push(result)?;
    engine.collect_ref_delta((r1 - r2) + (engine.reference_count() - r2));
    engine.collect_type(converted_type);
    engine.collect_op_length(length);
    Ok(())
}

/// Implements the ISTYPE operation.
fn is_type(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // v3.11.2 IsType pops first, validates the operand type after the pop, and
    // collects the pop's ref delta on the success path.
    let r = engine.reference_count();

    // Pop the item on the stack
    let item = engine.pop()?;

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

    let probe = stack_item_type_probe_value(item.stack_item_type());
    let result = crate::semantics::conversion::is_type(&probe, item_type.to_byte());

    engine.push(StackItem::from_bool(result))?;
    engine.collect_ref_delta(r - engine.reference_count());
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
    // v3.11.2 IsNull collects the pop's ref delta on the success path.
    let r = engine.reference_count();

    // Pop the item on the stack
    let item = engine.pop()?;

    let stack_value = match item {
        StackItem::Null => StackValue::Null,
        _ => StackValue::Boolean(true),
    };
    let result = crate::semantics::comparison::is_null(&stack_value);

    engine.push(StackItem::from_bool(result))?;
    engine.collect_ref_delta(r - engine.reference_count());
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
