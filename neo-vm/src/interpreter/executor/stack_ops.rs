use super::super::helpers::pop_integer;
use super::super::opcodes::*;
use super::super::runtime_types::StackValue;
use super::control::Dispatch;
use crate::semantics::stack as stack_rules;
use alloc::{string::String, vec::Vec};

#[inline]
pub(super) fn execute(opcode: u8, stack: &mut Vec<StackValue>) -> Result<Dispatch, String> {
    match opcode {
        DEPTH => stack.push(StackValue::Integer(stack_rules::depth(stack))),
        DROP => stack_rules::drop_top(stack)?,
        DUP => stack_rules::dup(stack)?,
        SWAP => stack_rules::swap(stack)?,
        NIP => stack_rules::nip(stack)?,
        OVER => stack_rules::over(stack)?,
        PICK => {
            let index = pop_integer(stack)?;
            stack_rules::pick(stack, index)?;
        }
        ROT => stack_rules::rot(stack)?,
        ROLL => {
            let index = pop_integer(stack)?;
            stack_rules::roll(stack, index)?;
        }
        REVERSE3 => stack_rules::reverse_top(stack, 3)?,
        REVERSE4 => stack_rules::reverse_top(stack, 4)?,
        REVERSEN => {
            let count = pop_integer(stack)?;
            stack_rules::reverse_n(stack, count)?;
        }
        TUCK => stack_rules::tuck(stack)?,
        XDROP => {
            let index = pop_integer(stack)?;
            stack_rules::xdrop(stack, index)?;
        }
        CLEAR => stack_rules::clear(stack),
        _ => unreachable!("opcode routed to stack_ops: 0x{opcode:02x}"),
    }
    Ok(Dispatch::Fallthrough)
}
