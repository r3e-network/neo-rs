//! Fast binary codec for StackValue serialization
//!
//! Custom binary format optimized for Neo RISC-V VM stack operations.
//! Replaces postcard with zero-copy, type-tagged encoding.

extern crate alloc;
use super::stack_value::{
    STACK_VALUE_CODEC_TAG_ARRAY, STACK_VALUE_CODEC_TAG_BIG_INTEGER, STACK_VALUE_CODEC_TAG_BOOLEAN,
    STACK_VALUE_CODEC_TAG_BUFFER, STACK_VALUE_CODEC_TAG_BYTESTRING, STACK_VALUE_CODEC_TAG_INTEGER,
    STACK_VALUE_CODEC_TAG_INTEROP, STACK_VALUE_CODEC_TAG_ITERATOR, STACK_VALUE_CODEC_TAG_MAP,
    STACK_VALUE_CODEC_TAG_NULL, STACK_VALUE_CODEC_TAG_POINTER, STACK_VALUE_CODEC_TAG_STRUCT,
    StackValue,
};
use alloc::vec::Vec;

/// Encode stack to binary format
#[inline]
pub fn encode_stack(stack: &[StackValue]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(stack.len() * 32);
    encode_stack_into(stack, &mut VecSink(&mut buf)).expect("Vec sink should not fail");

    buf
}

/// Encode stack into a pre-allocated slice (for no_std guest)
#[inline]
pub fn encode_stack_to_slice<'a>(
    stack: &[StackValue],
    buf: &'a mut [u8],
) -> Result<&'a mut [u8], &'static str> {
    let mut sink = SliceSink { buf, pos: 0 };
    encode_stack_into(stack, &mut sink)?;
    Ok(&mut sink.buf[..sink.pos])
}

const MAX_DECODE_DEPTH: usize = 64;
const MAX_COLLECTION_LEN: usize = 4096;

/// Decode stack from binary format
#[inline]
pub fn decode_stack(bytes: &[u8]) -> Result<Vec<StackValue>, &'static str> {
    let mut pos = 0;
    let len = read_u32(bytes, &mut pos, "truncated stack length")? as usize;
    if len > MAX_COLLECTION_LEN {
        return Err("collection length exceeds maximum");
    }
    let mut stack = Vec::with_capacity(len);

    for _ in 0..len {
        let (value, consumed) = decode_value_depth(&bytes[pos..], 0)?;
        stack.push(value);
        pos += consumed;
    }

    Ok(stack)
}

trait FastEncodeSink {
    fn write_u8(&mut self, value: u8, error: &'static str) -> Result<(), &'static str>;
    fn write_bytes(&mut self, bytes: &[u8], error: &'static str) -> Result<(), &'static str>;
}

struct VecSink<'a>(&'a mut Vec<u8>);

impl FastEncodeSink for VecSink<'_> {
    fn write_u8(&mut self, value: u8, _error: &'static str) -> Result<(), &'static str> {
        self.0.push(value);
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8], _error: &'static str) -> Result<(), &'static str> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

struct SliceSink<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl FastEncodeSink for SliceSink<'_> {
    fn write_u8(&mut self, value: u8, error: &'static str) -> Result<(), &'static str> {
        self.write_bytes(&[value], error)
    }

    fn write_bytes(&mut self, bytes: &[u8], error: &'static str) -> Result<(), &'static str> {
        let end = self.pos.checked_add(bytes.len()).ok_or(error)?;
        if self.buf.len() < end {
            return Err(error);
        }
        self.buf[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }
}

fn encode_stack_into<S: FastEncodeSink>(
    stack: &[StackValue],
    sink: &mut S,
) -> Result<(), &'static str> {
    write_u32(
        sink,
        stack.len() as u32,
        "buffer too small for stack length",
    )?;
    for item in stack {
        encode_value_into(item, sink)?;
    }
    Ok(())
}

fn write_u32<S: FastEncodeSink>(
    sink: &mut S,
    value: u32,
    error: &'static str,
) -> Result<(), &'static str> {
    sink.write_bytes(&value.to_le_bytes(), error)
}

fn write_i64<S: FastEncodeSink>(
    sink: &mut S,
    value: i64,
    error: &'static str,
) -> Result<(), &'static str> {
    sink.write_bytes(&value.to_le_bytes(), error)
}

fn write_u64<S: FastEncodeSink>(
    sink: &mut S,
    value: u64,
    error: &'static str,
) -> Result<(), &'static str> {
    sink.write_bytes(&value.to_le_bytes(), error)
}

fn write_len_prefixed_bytes<S: FastEncodeSink>(
    sink: &mut S,
    bytes: &[u8],
) -> Result<(), &'static str> {
    write_u32(sink, bytes.len() as u32, "buffer too small")?;
    sink.write_bytes(bytes, "buffer too small")
}

#[inline]
fn encode_value_into<S: FastEncodeSink>(
    value: &StackValue,
    sink: &mut S,
) -> Result<(), &'static str> {
    match value {
        StackValue::Integer(i) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_INTEGER, "buffer too small")?;
            write_i64(sink, *i, "buffer too small")
        }
        StackValue::BigInteger(b) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_BIG_INTEGER, "buffer too small")?;
            write_len_prefixed_bytes(sink, b)
        }
        StackValue::ByteString(b) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_BYTESTRING, "buffer too small")?;
            write_len_prefixed_bytes(sink, b)
        }
        StackValue::Buffer(b) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_BUFFER, "buffer too small")?;
            write_len_prefixed_bytes(sink, b)
        }
        StackValue::Boolean(b) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_BOOLEAN, "buffer too small")?;
            sink.write_u8(u8::from(*b), "buffer too small")
        }
        StackValue::Array(items) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_ARRAY, "buffer too small")?;
            write_u32(sink, items.len() as u32, "buffer too small")?;
            for item in items {
                encode_value_into(item, sink)?;
            }
            Ok(())
        }
        StackValue::Struct(items) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_STRUCT, "buffer too small")?;
            write_u32(sink, items.len() as u32, "buffer too small")?;
            for item in items {
                encode_value_into(item, sink)?;
            }
            Ok(())
        }
        StackValue::Map(pairs) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_MAP, "buffer too small")?;
            write_u32(sink, pairs.len() as u32, "buffer too small")?;
            for (k, v) in pairs {
                encode_value_into(k, sink)?;
                encode_value_into(v, sink)?;
            }
            Ok(())
        }
        StackValue::Interop(h) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_INTEROP, "buffer too small")?;
            write_u64(sink, *h, "buffer too small")
        }
        StackValue::Iterator(h) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_ITERATOR, "buffer too small")?;
            write_u64(sink, *h, "buffer too small")
        }
        StackValue::Null => sink.write_u8(STACK_VALUE_CODEC_TAG_NULL, "buffer too small"),
        StackValue::Pointer(p) => {
            sink.write_u8(STACK_VALUE_CODEC_TAG_POINTER, "buffer too small")?;
            write_i64(sink, *p, "buffer too small")
        }
    }
}

fn read_exact<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    len: usize,
    error: &'static str,
) -> Result<&'a [u8], &'static str> {
    let end = pos.checked_add(len).ok_or(error)?;
    if bytes.len() < end {
        return Err(error);
    }
    let data = &bytes[*pos..end];
    *pos = end;
    Ok(data)
}

fn read_u32(bytes: &[u8], pos: &mut usize, error: &'static str) -> Result<u32, &'static str> {
    let data = read_exact(bytes, pos, 4, error)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64(bytes: &[u8], pos: &mut usize, error: &'static str) -> Result<u64, &'static str> {
    let data = read_exact(bytes, pos, 8, error)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

fn read_i64(bytes: &[u8], pos: &mut usize, error: &'static str) -> Result<i64, &'static str> {
    Ok(read_u64(bytes, pos, error)? as i64)
}

fn read_len_prefixed_bytes(
    bytes: &[u8],
    pos: &mut usize,
    len_error: &'static str,
    data_error: &'static str,
) -> Result<Vec<u8>, &'static str> {
    let len = read_u32(bytes, pos, len_error)? as usize;
    Ok(read_exact(bytes, pos, len, data_error)?.to_vec())
}

#[inline]
fn decode_value_depth(bytes: &[u8], depth: usize) -> Result<(StackValue, usize), &'static str> {
    if depth > MAX_DECODE_DEPTH {
        return Err("decode depth exceeds maximum");
    }
    if bytes.is_empty() {
        return Err("empty buffer");
    }

    let tag = bytes[0];
    let mut pos = 1;

    let value = match tag {
        STACK_VALUE_CODEC_TAG_INTEGER => {
            let val = read_i64(bytes, &mut pos, "truncated integer")?;
            StackValue::Integer(val)
        }
        STACK_VALUE_CODEC_TAG_BIG_INTEGER => {
            let data = read_len_prefixed_bytes(
                bytes,
                &mut pos,
                "truncated biginteger length",
                "truncated biginteger data",
            )?;
            StackValue::BigInteger(data)
        }
        STACK_VALUE_CODEC_TAG_BYTESTRING => {
            let data = read_len_prefixed_bytes(
                bytes,
                &mut pos,
                "truncated bytestring length",
                "truncated bytestring data",
            )?;
            StackValue::ByteString(data)
        }
        STACK_VALUE_CODEC_TAG_BUFFER => {
            let data = read_len_prefixed_bytes(
                bytes,
                &mut pos,
                "truncated buffer length",
                "truncated buffer data",
            )?;
            StackValue::Buffer(data)
        }
        STACK_VALUE_CODEC_TAG_BOOLEAN => {
            let val = read_exact(bytes, &mut pos, 1, "truncated boolean")?[0] != 0;
            StackValue::Boolean(val)
        }
        STACK_VALUE_CODEC_TAG_ARRAY => {
            let len = read_u32(bytes, &mut pos, "truncated array length")? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum");
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                let (item, consumed) = decode_value_depth(&bytes[pos..], depth + 1)?;
                items.push(item);
                pos += consumed;
            }
            StackValue::Array(items)
        }
        STACK_VALUE_CODEC_TAG_STRUCT => {
            let len = read_u32(bytes, &mut pos, "truncated struct length")? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum");
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                let (item, consumed) = decode_value_depth(&bytes[pos..], depth + 1)?;
                items.push(item);
                pos += consumed;
            }
            StackValue::Struct(items)
        }
        STACK_VALUE_CODEC_TAG_MAP => {
            let len = read_u32(bytes, &mut pos, "truncated map length")? as usize;
            if len > MAX_COLLECTION_LEN {
                return Err("collection length exceeds maximum");
            }
            let mut pairs = Vec::with_capacity(len);
            for _ in 0..len {
                let (k, k_consumed) = decode_value_depth(&bytes[pos..], depth + 1)?;
                pos += k_consumed;
                let (v, v_consumed) = decode_value_depth(&bytes[pos..], depth + 1)?;
                pos += v_consumed;
                pairs.push((k, v));
            }
            StackValue::Map(pairs)
        }
        STACK_VALUE_CODEC_TAG_INTEROP => {
            let val = read_u64(bytes, &mut pos, "truncated interop")?;
            StackValue::Interop(val)
        }
        STACK_VALUE_CODEC_TAG_ITERATOR => {
            let val = read_u64(bytes, &mut pos, "truncated iterator")?;
            StackValue::Iterator(val)
        }
        STACK_VALUE_CODEC_TAG_NULL => StackValue::Null,
        STACK_VALUE_CODEC_TAG_POINTER => {
            let val = read_i64(bytes, &mut pos, "truncated pointer")?;
            StackValue::Pointer(val)
        }
        _ => return Err("invalid tag"),
    };

    Ok((value, pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn roundtrip_integer() {
        let stack = vec![StackValue::Integer(42), StackValue::Integer(-100)];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn roundtrip_bytestring() {
        let stack = vec![StackValue::ByteString(vec![1, 2, 3, 4])];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn roundtrip_buffer() {
        let stack = vec![StackValue::Buffer(vec![0, 0, 0, 0])];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn roundtrip_buffer_empty() {
        let stack = vec![StackValue::Buffer(vec![])];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn roundtrip_array() {
        let stack = vec![StackValue::Array(vec![
            StackValue::Integer(1),
            StackValue::Boolean(true),
            StackValue::Null,
        ])];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn roundtrip_map() {
        let stack = vec![StackValue::Map(vec![
            (StackValue::Integer(1), StackValue::ByteString(vec![0xAA])),
            (StackValue::Boolean(false), StackValue::Null),
        ])];
        let encoded = encode_stack(&stack);
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(stack, decoded);
    }

    #[test]
    fn decode_rejects_excessive_nesting() {
        // Build a payload: stack length = 1, then 65 nested arrays
        // (STACK_VALUE_CODEC_TAG_ARRAY, len=1) to exceed MAX_DECODE_DEPTH (64).
        // decode_value_depth starts at depth=0, each Array recurses with depth+1,
        // so the 65th nested array will attempt depth=65 which exceeds the limit.
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes()); // stack length = 1
        for _ in 0..65 {
            payload.push(STACK_VALUE_CODEC_TAG_ARRAY);
            payload.extend_from_slice(&1u32.to_le_bytes()); // array length = 1
        }
        // Innermost value (won't be reached due to depth limit)
        payload.push(STACK_VALUE_CODEC_TAG_NULL);

        let result = decode_stack(&payload);
        assert!(result.is_err(), "excessive nesting must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.contains("depth"),
            "error should mention depth, got: {err}"
        );
    }

    #[test]
    fn decode_rejects_excessive_collection_length() {
        // Build a payload: stack length = 1, then an array with length 5000,
        // which exceeds MAX_COLLECTION_LEN (4096).
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes()); // stack length = 1
        payload.push(STACK_VALUE_CODEC_TAG_ARRAY);
        payload.extend_from_slice(&5000u32.to_le_bytes()); // array length = 5000

        let result = decode_stack(&payload);
        assert!(
            result.is_err(),
            "excessive collection length must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("length"),
            "error should mention length, got: {err}"
        );
    }
}
