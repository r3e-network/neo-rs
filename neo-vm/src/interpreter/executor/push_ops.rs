use super::super::opcodes::*;
use super::super::runtime_types::StackValue;
use super::control::Dispatch;
use crate::semantics::numeric;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

#[inline]
pub(super) fn execute(
    opcode: u8,
    script: &[u8],
    ip_ref: &mut usize,
    stack: &mut Vec<StackValue>,
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
        // PUSH OPCODES (0x00-0x20)
        // =============================================================================
        PUSHINT8 => {
            if ip + 2 > script.len() {
                return Err("truncated PUSHINT8 operand".to_string());
            }
            let value = i8::from_le_bytes([script[ip + 1]]) as i64;
            stack.push(StackValue::Integer(value));
            ip += 2;
            finish!(Dispatch::Continue);
        }
        PUSHINT16 => {
            if ip + 3 > script.len() {
                return Err("truncated PUSHINT16 operand".to_string());
            }
            let value = i16::from_le_bytes([script[ip + 1], script[ip + 2]]) as i64;
            stack.push(StackValue::Integer(value));
            ip += 3;
            finish!(Dispatch::Continue);
        }
        PUSHINT32 => {
            if ip + 5 > script.len() {
                return Err("truncated PUSHINT32 operand".to_string());
            }
            let value = i32::from_le_bytes([
                script[ip + 1],
                script[ip + 2],
                script[ip + 3],
                script[ip + 4],
            ]) as i64;
            stack.push(StackValue::Integer(value));
            ip += 5;
            finish!(Dispatch::Continue);
        }
        PUSHINT64 => {
            if ip + 9 > script.len() {
                return Err("truncated PUSHINT64 operand".to_string());
            }
            let value = i64::from_le_bytes([
                script[ip + 1],
                script[ip + 2],
                script[ip + 3],
                script[ip + 4],
                script[ip + 5],
                script[ip + 6],
                script[ip + 7],
                script[ip + 8],
            ]);
            stack.push(StackValue::Integer(value));
            ip += 9;
            finish!(Dispatch::Continue);
        }
        PUSHINT128 => {
            if ip + 17 > script.len() {
                return Err("truncated PUSHINT128 operand".to_string());
            }
            stack.push(StackValue::BigInteger(numeric::trim_le_bytes_slice(
                &script[ip + 1..ip + 17],
            )));
            ip += 17;
            finish!(Dispatch::Continue);
        }
        PUSHINT256 => {
            if ip + 33 > script.len() {
                return Err("truncated PUSHINT256 operand".to_string());
            }
            stack.push(StackValue::BigInteger(numeric::trim_le_bytes_slice(
                &script[ip + 1..ip + 33],
            )));
            ip += 33;
            finish!(Dispatch::Continue);
        }
        PUSHT => {
            stack.push(StackValue::Boolean(true));
            ip += 1;
            finish!(Dispatch::Continue);
        }
        PUSHF => {
            stack.push(StackValue::Boolean(false));
            ip += 1;
            finish!(Dispatch::Continue);
        }
        PUSHNULL => stack.push(StackValue::Null),
        PUSHDATA1 => {
            if ip + 2 > script.len() {
                return Err("truncated PUSHDATA1 length".to_string());
            }
            let len = script[ip + 1] as usize;
            let start = ip + 2;
            let end = start + len;
            if end > script.len() {
                return Err("truncated PUSHDATA1 payload".to_string());
            }
            stack.push(StackValue::ByteString(script[start..end].to_vec()));
            ip = end;
            finish!(Dispatch::Continue);
        }
        PUSHDATA2 => {
            if ip + 3 > script.len() {
                return Err("truncated PUSHDATA2 length".to_string());
            }
            let len = u16::from_le_bytes([script[ip + 1], script[ip + 2]]) as usize;
            let start = ip + 3;
            let end = start + len;
            if end > script.len() {
                return Err("truncated PUSHDATA2 payload".to_string());
            }
            stack.push(StackValue::ByteString(script[start..end].to_vec()));
            ip = end;
            finish!(Dispatch::Continue);
        }
        PUSHDATA4 => {
            if ip + 5 > script.len() {
                return Err("truncated PUSHDATA4 length".to_string());
            }
            let raw_len = u32::from_le_bytes([
                script[ip + 1],
                script[ip + 2],
                script[ip + 3],
                script[ip + 4],
            ]);
            // NeoVM: negative (high bit set) or oversized lengths are invalid
            if raw_len > 0x0010_0000 {
                return Err("PUSHDATA4 length exceeds maximum".to_string());
            }
            let len = raw_len as usize;
            let start = ip + 5;
            let end = start
                .checked_add(len)
                .ok_or_else(|| "PUSHDATA4 length overflow".to_string())?;
            if end > script.len() {
                return Err("truncated PUSHDATA4 payload".to_string());
            }
            stack.push(StackValue::ByteString(script[start..end].to_vec()));
            ip = end;
            finish!(Dispatch::Continue);
        }
        PUSHM1 => stack.push(StackValue::Integer(-1)),
        PUSH0 => stack.push(StackValue::Integer(0)),
        PUSH1..=PUSH16 => stack.push(StackValue::Integer(i64::from(opcode - PUSH0))),
        PUSHA => {
            if ip + 5 > script.len() {
                return Err("truncated PUSHA operand".to_string());
            }
            let offset = i32::from_le_bytes([
                script[ip + 1],
                script[ip + 2],
                script[ip + 3],
                script[ip + 4],
            ]) as i64;
            let target = ip as i64 + offset;
            if target < 0 || target as usize > script.len() {
                return Err("PUSHA target out of bounds".to_string());
            }
            stack.push(StackValue::Pointer(target as usize));
            ip += 5;
            finish!(Dispatch::Continue);
        }
        _ => unreachable!("opcode routed to push_ops: 0x{opcode:02x}"),
    }
    *ip_ref = ip;
    Ok(Dispatch::Fallthrough)
}
