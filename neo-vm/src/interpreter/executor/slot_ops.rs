use super::super::helpers::{
    decode_retained_prefix_into, encode_retained_value_to_slice, ensure_retained_capacity,
    pop_item, RETAINED_ARGS_BUF,
};
use super::super::opcodes::*;
use super::super::runtime_types::{CompoundIds, StackValue};
use super::control::Dispatch;
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn execute(
    opcode: u8,
    script: &[u8],
    ip_ref: &mut usize,
    stack: &mut Vec<StackValue>,
    locals: &mut Vec<StackValue>,
    args: &mut Vec<StackValue>,
    static_fields: &mut Vec<StackValue>,
    slots_initialized: &mut bool,
    static_fields_initialized: &mut bool,
    ids: &mut CompoundIds,
    callt_result_pending_store: bool,
) -> Result<Dispatch, String> {
    let mut ip = *ip_ref;
    macro_rules! finish {
        ($dispatch:expr) => {{
            *ip_ref = ip;
            return Ok($dispatch);
        }};
    }

    match opcode {
        // =============================================================================
        // SLOT OPERATIONS (0x56-0x87)
        // =============================================================================
        INITSSLOT => {
            if ip + 2 > script.len() {
                return Err("truncated INITSSLOT operand".to_string());
            }
            let static_count = script[ip + 1] as usize;
            if static_count == 0 {
                return Err("INITSSLOT with 0 items is not allowed".to_string());
            }
            if *static_fields_initialized {
                return Err("static fields already initialized".to_string());
            }
            *static_fields = vec![StackValue::Null; static_count];
            *static_fields_initialized = true;
            ip += 2;
            finish!(Dispatch::Continue);
        }
        INITSLOT => {
            if ip + 3 > script.len() {
                return Err("truncated INITSLOT operands".to_string());
            }
            let local_count = script[ip + 1] as usize;
            let arg_count = script[ip + 2] as usize;
            // NeoVM: INITSLOT with 0 args AND 0 locals is invalid
            if local_count == 0 && arg_count == 0 {
                return Err("INITSLOT with 0 args and 0 locals is not allowed".to_string());
            }
            if *slots_initialized {
                return Err("slots already initialized".to_string());
            }
            // NeoVM keeps arguments and locals in separate slot arrays. Arguments
            // are consumed from the evaluation stack before local slots are created.
            let new_args = if cfg!(target_arch = "riscv32") && arg_count > 0 {
                let buf = unsafe { RETAINED_ARGS_BUF.as_mut_slice() };
                ensure_retained_capacity(buf, 0, 4)?;
                buf[0..4].copy_from_slice(&(arg_count as u32).to_le_bytes());
                let mut pos = 4;
                for _ in 0..arg_count {
                    let value = stack
                        .pop()
                        .ok_or_else(|| "stack underflow for INITSLOT args".to_string())?;
                    pos = encode_retained_value_to_slice(&value, buf, pos)?;
                }
                let mut restored = Vec::with_capacity(arg_count);
                decode_retained_prefix_into(&buf[..pos], &mut restored)?;
                restored
            } else {
                let mut restored = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    restored.push(
                        stack
                            .pop()
                            .ok_or_else(|| "stack underflow for INITSLOT args".to_string())?,
                    );
                }
                restored
            };
            let new_locals = vec![StackValue::Null; local_count];
            *args = new_args;
            *locals = new_locals;
            *slots_initialized = true;
            ip += 3;
            finish!(Dispatch::Continue);
        }
        LDSFLD0..=LDSFLD6 => {
            let index = (opcode - LDSFLD0) as usize;
            if index >= static_fields.len() {
                return Err("invalid static field index".to_string());
            }
            let value = static_fields
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid static field index".to_string())?;
            stack.push(value);
        }
        LDSFLD => {
            if ip + 2 > script.len() {
                return Err("truncated LDSFLD operand".to_string());
            }
            let index = script[ip + 1] as usize;
            if index >= static_fields.len() {
                return Err("invalid static field index".to_string());
            }
            let value = static_fields
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid static field index".to_string())?;
            stack.push(value);
            ip += 2;
            finish!(Dispatch::Continue);
        }
        LDLOC0..=LDLOC6 => {
            let index = (opcode - LDLOC0) as usize;
            let value = locals
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid local index".to_string())?;
            stack.push(value);
        }
        LDLOC => {
            if ip + 2 > script.len() {
                return Err("truncated LDLOC operand".to_string());
            }
            let index = script[ip + 1] as usize;
            let value = locals
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid local index".to_string())?;
            stack.push(value);
            ip += 2;
            finish!(Dispatch::Continue);
        }
        STLOC0..=STLOC6 => {
            let index = (opcode - STLOC0) as usize;
            let value = pop_item(stack)?;
            let value = if callt_result_pending_store
                && cfg!(target_arch = "riscv32")
                && matches!(
                    value,
                    StackValue::Array(..)
                        | StackValue::Struct(..)
                        | StackValue::Map(..)
                        | StackValue::Buffer(..)
                ) {
                ids.deep_clone(&value)
            } else {
                value
            };
            let slot = locals
                .get_mut(index)
                .ok_or_else(|| "invalid local index".to_string())?;
            *slot = value;
        }
        STSFLD0..=STSFLD6 => {
            let index = (opcode - STSFLD0) as usize;
            let value = pop_item(stack)?;
            let value = if callt_result_pending_store
                && cfg!(target_arch = "riscv32")
                && matches!(
                    value,
                    StackValue::Array(..)
                        | StackValue::Struct(..)
                        | StackValue::Map(..)
                        | StackValue::Buffer(..)
                ) {
                ids.deep_clone(&value)
            } else {
                value
            };
            if index >= static_fields.len() {
                return Err("invalid static field index".to_string());
            }
            let slot = static_fields
                .get_mut(index)
                .ok_or_else(|| "invalid static field index".to_string())?;
            *slot = value;
        }
        STSFLD => {
            if ip + 2 > script.len() {
                return Err("truncated STSFLD operand".to_string());
            }
            let index = script[ip + 1] as usize;
            let value = pop_item(stack)?;
            if index >= static_fields.len() {
                return Err("invalid static field index".to_string());
            }
            let slot = static_fields
                .get_mut(index)
                .ok_or_else(|| "invalid static field index".to_string())?;
            *slot = value;
            ip += 2;
            finish!(Dispatch::Continue);
        }
        STLOC => {
            if ip + 2 > script.len() {
                return Err("truncated STLOC operand".to_string());
            }
            let index = script[ip + 1] as usize;
            let value = pop_item(stack)?;
            let value = if callt_result_pending_store
                && cfg!(target_arch = "riscv32")
                && matches!(
                    value,
                    StackValue::Array(..)
                        | StackValue::Struct(..)
                        | StackValue::Map(..)
                        | StackValue::Buffer(..)
                ) {
                ids.deep_clone(&value)
            } else {
                value
            };
            let slot = locals
                .get_mut(index)
                .ok_or_else(|| "invalid local index".to_string())?;
            *slot = value;
            ip += 2;
            finish!(Dispatch::Continue);
        }
        // =============================================================================
        LDARG0..=LDARG6 => {
            let index = (opcode - LDARG0) as usize;
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid argument index".to_string())?;
            stack.push(value);
        }
        LDARG => {
            if ip + 2 > script.len() {
                return Err("truncated LDARG operand".to_string());
            }
            let index = script[ip + 1] as usize;
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| "invalid argument index".to_string())?;
            stack.push(value);
            ip += 2;
            finish!(Dispatch::Continue);
        }
        STARG0..=STARG6 => {
            let index = (opcode - STARG0) as usize;
            let value = pop_item(stack)?;
            let value = if callt_result_pending_store
                && cfg!(target_arch = "riscv32")
                && matches!(
                    value,
                    StackValue::Array(..)
                        | StackValue::Struct(..)
                        | StackValue::Map(..)
                        | StackValue::Buffer(..)
                ) {
                ids.deep_clone(&value)
            } else {
                value
            };
            let slot = args
                .get_mut(index)
                .ok_or_else(|| "invalid argument index".to_string())?;
            *slot = value;
        }
        STARG => {
            if ip + 2 > script.len() {
                return Err("truncated STARG operand".to_string());
            }
            let index = script[ip + 1] as usize;
            let value = pop_item(stack)?;
            let value = if callt_result_pending_store
                && cfg!(target_arch = "riscv32")
                && matches!(
                    value,
                    StackValue::Array(..)
                        | StackValue::Struct(..)
                        | StackValue::Map(..)
                        | StackValue::Buffer(..)
                ) {
                ids.deep_clone(&value)
            } else {
                value
            };
            let slot = args
                .get_mut(index)
                .ok_or_else(|| "invalid argument index".to_string())?;
            *slot = value;
            ip += 2;
            finish!(Dispatch::Continue);
        }
        _ => unreachable!("opcode routed to slot_ops: 0x{opcode:02x}"),
    }
    *ip_ref = ip;
    Ok(Dispatch::Fallthrough)
}
