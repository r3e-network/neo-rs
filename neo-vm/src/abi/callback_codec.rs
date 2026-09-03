extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use super::cursor::Cursor;
use crate::StackValue;

const TAG_OK_STACK: u8 = 0;
const TAG_ERR: u8 = 1;
const TAG_OK_EMPTY: u8 = 2;
const TAG_OK_INTEGER: u8 = 3;
const TAG_OK_BOOLEAN: u8 = 4;
const TAG_OK_NULL: u8 = 5;
const TAG_OK_BYTESTRING: u8 = 6;
const TAG_OK_BIGINTEGER: u8 = 7;
const TAG_OK_INTEROP: u8 = 8;
const TAG_OK_ITERATOR: u8 = 9;
const TAG_OK_POINTER: u8 = 10;
const TAG_OK_BUFFER: u8 = 11;

#[inline]
pub fn encode_stack_result(result: &Result<Vec<StackValue>, String>) -> Vec<u8> {
    let capacity = match result {
        Ok(stack) => 1 + 4 + stack.len() * 16,
        Err(msg) => 1 + 4 + msg.len(),
    };
    let mut out = Vec::with_capacity(capacity);
    match result {
        Ok(stack) => match stack.as_slice() {
            [] => out.push(TAG_OK_EMPTY),
            [StackValue::Integer(value)] => {
                out.push(TAG_OK_INTEGER);
                out.extend_from_slice(&value.to_le_bytes());
            }
            [StackValue::Boolean(value)] => {
                out.push(TAG_OK_BOOLEAN);
                out.push(u8::from(*value));
            }
            [StackValue::Null] => out.push(TAG_OK_NULL),
            [StackValue::ByteString(bytes)] => {
                out.push(TAG_OK_BYTESTRING);
                encode_bytes(bytes, &mut out);
            }
            [StackValue::BigInteger(bytes)] => {
                out.push(TAG_OK_BIGINTEGER);
                encode_bytes(bytes, &mut out);
            }
            [StackValue::Interop(handle)] => {
                out.push(TAG_OK_INTEROP);
                out.extend_from_slice(&handle.to_le_bytes());
            }
            [StackValue::Iterator(handle)] => {
                out.push(TAG_OK_ITERATOR);
                out.extend_from_slice(&handle.to_le_bytes());
            }
            [StackValue::Pointer(value)] => {
                out.push(TAG_OK_POINTER);
                out.extend_from_slice(&value.to_le_bytes());
            }
            [StackValue::Buffer(bytes)] => {
                out.push(TAG_OK_BUFFER);
                encode_bytes(bytes, &mut out);
            }
            _ => {
                out.push(TAG_OK_STACK);
                encode_u32(stack.len() as u32, &mut out);
                for item in stack {
                    encode_stack_value(item, &mut out);
                }
            }
        },
        Err(message) => {
            out.push(TAG_ERR);
            encode_bytes(message.as_bytes(), &mut out);
        }
    }
    out
}

const MAX_DECODE_DEPTH: usize = 64;
const MAX_COLLECTION_LEN: usize = 4096;

pub fn decode_stack_result_into(
    bytes: &[u8],
    stack: &mut Vec<StackValue>,
) -> Result<Result<(), String>, String> {
    let mut cursor = Cursor::new(bytes);
    let tag = cursor.read_u8()?;
    let value = match tag {
        TAG_OK_STACK => {
            let len = cursor.read_u32()? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            stack.clear();
            stack.reserve(len);
            for _ in 0..len {
                stack.push(decode_stack_value_depth(&mut cursor, 0)?);
            }
            Ok(())
        }
        TAG_ERR => {
            let message = cursor.read_bytes()?.to_vec();
            Err(String::from_utf8(message)
                .map_err(|_| "invalid utf-8 error payload".to_string())?)
        }
        TAG_OK_EMPTY => {
            stack.clear();
            Ok(())
        }
        TAG_OK_INTEGER => {
            stack.clear();
            stack.push(StackValue::Integer(cursor.read_i64()?));
            Ok(())
        }
        TAG_OK_BOOLEAN => {
            stack.clear();
            stack.push(StackValue::Boolean(cursor.read_u8()? != 0));
            Ok(())
        }
        TAG_OK_NULL => {
            stack.clear();
            stack.push(StackValue::Null);
            Ok(())
        }
        TAG_OK_BYTESTRING => {
            stack.clear();
            stack.push(StackValue::ByteString(cursor.read_bytes()?.to_vec()));
            Ok(())
        }
        TAG_OK_BIGINTEGER => {
            stack.clear();
            stack.push(StackValue::BigInteger(cursor.read_bytes()?.to_vec()));
            Ok(())
        }
        TAG_OK_INTEROP => {
            stack.clear();
            stack.push(StackValue::Interop(cursor.read_u64()?));
            Ok(())
        }
        TAG_OK_ITERATOR => {
            stack.clear();
            stack.push(StackValue::Iterator(cursor.read_u64()?));
            Ok(())
        }
        TAG_OK_POINTER => {
            stack.clear();
            stack.push(StackValue::Pointer(cursor.read_i64()?));
            Ok(())
        }
        TAG_OK_BUFFER => {
            stack.clear();
            stack.push(StackValue::Buffer(cursor.read_bytes()?.to_vec()));
            Ok(())
        }
        _ => return Err("invalid stack result tag".to_string()),
    };
    cursor.expect_eof("trailing bytes in payload")?;
    Ok(value)
}

pub fn decode_stack_result(bytes: &[u8]) -> Result<Result<Vec<StackValue>, String>, String> {
    let mut cursor = Cursor::new(bytes);
    let tag = cursor.read_u8()?;
    let value = match tag {
        TAG_OK_STACK => {
            let len = cursor.read_u32()? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut stack = Vec::with_capacity(len);
            for _ in 0..len {
                stack.push(decode_stack_value_depth(&mut cursor, 0)?);
            }
            Ok(stack)
        }
        TAG_ERR => {
            let message = cursor.read_bytes()?.to_vec();
            Err(String::from_utf8(message)
                .map_err(|_| "invalid utf-8 error payload".to_string())?)
        }
        TAG_OK_EMPTY => Ok(Vec::new()),
        TAG_OK_INTEGER => Ok(vec![StackValue::Integer(cursor.read_i64()?)]),
        TAG_OK_BOOLEAN => Ok(vec![StackValue::Boolean(cursor.read_u8()? != 0)]),
        TAG_OK_NULL => Ok(vec![StackValue::Null]),
        TAG_OK_BYTESTRING => Ok(vec![StackValue::ByteString(cursor.read_bytes()?.to_vec())]),
        TAG_OK_BIGINTEGER => Ok(vec![StackValue::BigInteger(cursor.read_bytes()?.to_vec())]),
        TAG_OK_INTEROP => Ok(vec![StackValue::Interop(cursor.read_u64()?)]),
        TAG_OK_ITERATOR => Ok(vec![StackValue::Iterator(cursor.read_u64()?)]),
        TAG_OK_POINTER => Ok(vec![StackValue::Pointer(cursor.read_i64()?)]),
        TAG_OK_BUFFER => Ok(vec![StackValue::Buffer(cursor.read_bytes()?.to_vec())]),
        _ => return Err("invalid stack result tag".to_string()),
    };
    cursor.expect_eof("trailing bytes in payload")?;
    Ok(value)
}

#[inline]
fn encode_stack_value(value: &StackValue, out: &mut Vec<u8>) {
    match value {
        StackValue::Integer(value) => {
            out.push(0);
            out.extend_from_slice(&value.to_le_bytes());
        }
        StackValue::BigInteger(bytes) => {
            out.push(1);
            encode_bytes(bytes, out);
        }
        StackValue::ByteString(bytes) => {
            out.push(2);
            encode_bytes(bytes, out);
        }
        StackValue::Boolean(value) => {
            out.push(3);
            out.push(u8::from(*value));
        }
        StackValue::Array(items) => {
            out.push(4);
            encode_u32(items.len() as u32, out);
            for item in items {
                encode_stack_value(item, out);
            }
        }
        StackValue::Struct(items) => {
            out.push(5);
            encode_u32(items.len() as u32, out);
            for item in items {
                encode_stack_value(item, out);
            }
        }
        StackValue::Map(entries) => {
            out.push(6);
            encode_u32(entries.len() as u32, out);
            for (key, value) in entries {
                encode_stack_value(key, out);
                encode_stack_value(value, out);
            }
        }
        StackValue::Interop(handle) => {
            out.push(7);
            out.extend_from_slice(&handle.to_le_bytes());
        }
        StackValue::Iterator(handle) => {
            out.push(8);
            out.extend_from_slice(&handle.to_le_bytes());
        }
        StackValue::Null => out.push(9),
        StackValue::Buffer(bytes) => {
            out.push(11);
            encode_bytes(bytes, out);
        }
        StackValue::Pointer(value) => {
            out.push(10);
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

#[inline]
fn decode_stack_value_depth(cursor: &mut Cursor<'_>, depth: usize) -> Result<StackValue, String> {
    if depth > MAX_DECODE_DEPTH {
        return Err("decode depth exceeds maximum".to_string());
    }
    match cursor.read_u8()? {
        0 => Ok(StackValue::Integer(cursor.read_i64()?)),
        1 => Ok(StackValue::BigInteger(cursor.read_bytes()?.to_vec())),
        2 => Ok(StackValue::ByteString(cursor.read_bytes()?.to_vec())),
        3 => Ok(StackValue::Boolean(cursor.read_u8()? != 0)),
        4 => {
            let len = cursor.read_u32()? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_stack_value_depth(cursor, depth + 1)?);
            }
            Ok(StackValue::Array(items))
        }
        5 => {
            let len = cursor.read_u32()? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_stack_value_depth(cursor, depth + 1)?);
            }
            Ok(StackValue::Struct(items))
        }
        6 => {
            let len = cursor.read_u32()? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                let key = decode_stack_value_depth(cursor, depth + 1)?;
                let value = decode_stack_value_depth(cursor, depth + 1)?;
                items.push((key, value));
            }
            Ok(StackValue::Map(items))
        }
        7 => Ok(StackValue::Interop(cursor.read_u64()?)),
        8 => Ok(StackValue::Iterator(cursor.read_u64()?)),
        9 => Ok(StackValue::Null),
        10 => Ok(StackValue::Pointer(cursor.read_i64()?)),
        11 => Ok(StackValue::Buffer(cursor.read_bytes()?.to_vec())),
        _ => Err("invalid stack value tag".to_string()),
    }
}

#[inline]
fn encode_u32(value: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&value.to_le_bytes());
    out.extend_from_slice(&buf);
}

#[inline]
fn encode_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    let mut len_buf = [0u8; 4];
    len_buf.copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&len_buf);
    out.extend_from_slice(bytes);
}
