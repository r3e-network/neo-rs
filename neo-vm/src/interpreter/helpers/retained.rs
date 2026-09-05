use super::*;

pub(crate) fn encode_retained_prefix_to_slice(
    stack: &[StackValue],
    buf: &mut [u8],
) -> Result<usize, String> {
    if buf.len() < 4 {
        return Err("retained prefix buffer too small".to_string());
    }

    let len = stack.len();
    if len > MAX_RETAINED_COLLECTION_LEN {
        return Err("collection length exceeds maximum".to_string());
    }

    buf[0..4].copy_from_slice(&(len as u32).to_le_bytes());
    let mut pos = 4;
    for value in stack {
        pos = encode_retained_value_to_slice(value, buf, pos)?;
    }
    Ok(pos)
}

pub(crate) fn encode_retained_value_to_slice(
    value: &StackValue,
    buf: &mut [u8],
    mut pos: usize,
) -> Result<usize, String> {
    match value {
        StackValue::Integer(value) => {
            ensure_retained_capacity(buf, pos, 9)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_INTEGER;
            pos += 1;
            buf[pos..pos + 8].copy_from_slice(&value.to_le_bytes());
            Ok(pos + 8)
        }
        StackValue::BigInteger(bytes) => {
            pos = encode_retained_tag_and_len(
                STACK_VALUE_CODEC_TAG_BIG_INTEGER,
                bytes.len(),
                buf,
                pos,
            )?;
            ensure_retained_capacity(buf, pos, bytes.len())?;
            buf[pos..pos + bytes.len()].copy_from_slice(bytes);
            Ok(pos + bytes.len())
        }
        StackValue::ByteString(bytes) => {
            pos = encode_retained_tag_and_len(
                STACK_VALUE_CODEC_TAG_BYTESTRING,
                bytes.len(),
                buf,
                pos,
            )?;
            ensure_retained_capacity(buf, pos, bytes.len())?;
            buf[pos..pos + bytes.len()].copy_from_slice(bytes);
            Ok(pos + bytes.len())
        }
        StackValue::Boolean(value) => {
            ensure_retained_capacity(buf, pos, 2)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_BOOLEAN;
            buf[pos + 1] = u8::from(*value);
            Ok(pos + 2)
        }
        StackValue::Pointer(value) => {
            ensure_retained_capacity(buf, pos, 9)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_POINTER;
            pos += 1;
            buf[pos..pos + 8].copy_from_slice(&(*value as u64).to_le_bytes());
            Ok(pos + 8)
        }
        StackValue::Array(id, items) => {
            pos = encode_retained_tag_id_len(
                STACK_VALUE_CODEC_TAG_ARRAY,
                *id,
                items.len(),
                buf,
                pos,
            )?;
            for item in items {
                pos = encode_retained_value_to_slice(item, buf, pos)?;
            }
            Ok(pos)
        }
        StackValue::Struct(id, items) => {
            pos = encode_retained_tag_id_len(
                STACK_VALUE_CODEC_TAG_STRUCT,
                *id,
                items.len(),
                buf,
                pos,
            )?;
            for item in items {
                pos = encode_retained_value_to_slice(item, buf, pos)?;
            }
            Ok(pos)
        }
        StackValue::Map(id, entries) => {
            pos = encode_retained_tag_id_len(
                STACK_VALUE_CODEC_TAG_MAP,
                *id,
                entries.len(),
                buf,
                pos,
            )?;
            for (key, value) in entries {
                pos = encode_retained_value_to_slice(key, buf, pos)?;
                pos = encode_retained_value_to_slice(value, buf, pos)?;
            }
            Ok(pos)
        }
        StackValue::Buffer(id, bytes) => {
            pos = encode_retained_tag_id_len(
                STACK_VALUE_CODEC_TAG_BUFFER,
                *id,
                bytes.len(),
                buf,
                pos,
            )?;
            ensure_retained_capacity(buf, pos, bytes.len())?;
            buf[pos..pos + bytes.len()].copy_from_slice(bytes);
            Ok(pos + bytes.len())
        }
        StackValue::Interop(handle) => {
            ensure_retained_capacity(buf, pos, 9)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_INTEROP;
            pos += 1;
            buf[pos..pos + 8].copy_from_slice(&handle.to_le_bytes());
            Ok(pos + 8)
        }
        StackValue::Iterator(handle) => {
            ensure_retained_capacity(buf, pos, 9)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_ITERATOR;
            pos += 1;
            buf[pos..pos + 8].copy_from_slice(&handle.to_le_bytes());
            Ok(pos + 8)
        }
        StackValue::Null => {
            ensure_retained_capacity(buf, pos, 1)?;
            buf[pos] = STACK_VALUE_CODEC_TAG_NULL;
            Ok(pos + 1)
        }
    }
}

fn encode_retained_tag_and_len(
    tag: u8,
    len: usize,
    buf: &mut [u8],
    pos: usize,
) -> Result<usize, String> {
    if len > MAX_RETAINED_COLLECTION_LEN
        && matches!(
            tag,
            STACK_VALUE_CODEC_TAG_ARRAY | STACK_VALUE_CODEC_TAG_STRUCT | STACK_VALUE_CODEC_TAG_MAP
        )
    {
        return Err("collection length exceeds maximum".to_string());
    }
    let len = u32::try_from(len).map_err(|_| "retained prefix length exceeds u32".to_string())?;
    ensure_retained_capacity(buf, pos, 5)?;
    buf[pos] = tag;
    buf[pos + 1..pos + 5].copy_from_slice(&len.to_le_bytes());
    Ok(pos + 5)
}

fn encode_retained_tag_id_len(
    tag: u8,
    id: u64,
    len: usize,
    buf: &mut [u8],
    pos: usize,
) -> Result<usize, String> {
    if len > MAX_RETAINED_COLLECTION_LEN {
        return Err("collection length exceeds maximum".to_string());
    }
    let len = u32::try_from(len).map_err(|_| "retained prefix length exceeds u32".to_string())?;
    ensure_retained_capacity(buf, pos, 13)?;
    buf[pos] = tag;
    buf[pos + 1..pos + 9].copy_from_slice(&id.to_le_bytes());
    buf[pos + 9..pos + 13].copy_from_slice(&len.to_le_bytes());
    Ok(pos + 13)
}

pub(crate) fn ensure_retained_capacity(
    buf: &[u8],
    pos: usize,
    needed: usize,
) -> Result<(), String> {
    if buf.len().saturating_sub(pos) < needed {
        Err("retained prefix buffer too small".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn decode_retained_prefix(bytes: &[u8]) -> Result<Vec<StackValue>, String> {
    let mut stack = Vec::new();
    decode_retained_prefix_into(bytes, &mut stack)?;
    Ok(stack)
}

pub(crate) fn decode_retained_prefix_into(
    bytes: &[u8],
    stack: &mut Vec<StackValue>,
) -> Result<(), String> {
    if bytes.len() < 4 {
        return Err("truncated retained prefix length".to_string());
    }

    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if len > MAX_RETAINED_COLLECTION_LEN {
        return Err("collection length exceeds maximum".to_string());
    }

    stack.clear();
    stack.reserve(len);

    let mut pos = 4;
    for _ in 0..len {
        stack.push(decode_retained_value(bytes, &mut pos, 0)?);
    }
    if pos != bytes.len() {
        return Err("retained prefix has trailing bytes".to_string());
    }
    Ok(())
}

fn decode_retained_value(
    bytes: &[u8],
    pos: &mut usize,
    depth: usize,
) -> Result<StackValue, String> {
    if depth > MAX_RETAINED_DECODE_DEPTH {
        return Err("decode depth exceeds maximum".to_string());
    }
    if *pos >= bytes.len() {
        return Err("truncated retained prefix tag".to_string());
    }

    let tag = bytes[*pos];
    *pos += 1;
    match tag {
        STACK_VALUE_CODEC_TAG_INTEGER => {
            let value = decode_retained_i64(bytes, pos)?;
            Ok(StackValue::Integer(value))
        }
        STACK_VALUE_CODEC_TAG_BIG_INTEGER => {
            let data = decode_retained_bytes(bytes, pos)?;
            Ok(StackValue::BigInteger(data))
        }
        STACK_VALUE_CODEC_TAG_BYTESTRING => {
            let data = decode_retained_bytes(bytes, pos)?;
            Ok(StackValue::ByteString(data))
        }
        STACK_VALUE_CODEC_TAG_BOOLEAN => {
            let value = decode_retained_u8(bytes, pos)?;
            Ok(StackValue::Boolean(value != 0))
        }
        STACK_VALUE_CODEC_TAG_POINTER => {
            let value = decode_retained_u64(bytes, pos)?;
            let pointer = usize::try_from(value).map_err(|_| "pointer out of range".to_string())?;
            Ok(StackValue::Pointer(pointer))
        }
        STACK_VALUE_CODEC_TAG_ARRAY => {
            let id = decode_retained_u64(bytes, pos)?;
            let len = decode_retained_u32(bytes, pos)? as usize;
            if len > MAX_RETAINED_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_retained_value(bytes, pos, depth + 1)?);
            }
            Ok(StackValue::Array(id, items))
        }
        STACK_VALUE_CODEC_TAG_STRUCT => {
            let id = decode_retained_u64(bytes, pos)?;
            let len = decode_retained_u32(bytes, pos)? as usize;
            if len > MAX_RETAINED_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_retained_value(bytes, pos, depth + 1)?);
            }
            Ok(StackValue::Struct(id, items))
        }
        STACK_VALUE_CODEC_TAG_MAP => {
            let id = decode_retained_u64(bytes, pos)?;
            let len = decode_retained_u32(bytes, pos)? as usize;
            if len > MAX_RETAINED_COLLECTION_LEN {
                return Err("collection length exceeds maximum".to_string());
            }
            let mut entries = Vec::with_capacity(len);
            for _ in 0..len {
                let key = decode_retained_value(bytes, pos, depth + 1)?;
                let value = decode_retained_value(bytes, pos, depth + 1)?;
                entries.push((key, value));
            }
            Ok(StackValue::Map(id, entries))
        }
        STACK_VALUE_CODEC_TAG_BUFFER => {
            let id = decode_retained_u64(bytes, pos)?;
            let data = decode_retained_bytes(bytes, pos)?;
            Ok(StackValue::Buffer(id, data))
        }
        STACK_VALUE_CODEC_TAG_INTEROP => {
            let handle = decode_retained_u64(bytes, pos)?;
            Ok(StackValue::Interop(handle))
        }
        STACK_VALUE_CODEC_TAG_ITERATOR => {
            let handle = decode_retained_u64(bytes, pos)?;
            Ok(StackValue::Iterator(handle))
        }
        STACK_VALUE_CODEC_TAG_NULL => Ok(StackValue::Null),
        _ => Err(format!("invalid retained prefix tag 0x{tag:02x}")),
    }
}

fn decode_retained_bytes(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    let len = decode_retained_u32(bytes, pos)? as usize;
    if bytes.len().saturating_sub(*pos) < len {
        return Err("truncated retained prefix bytes".to_string());
    }
    let data = bytes[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(data)
}

fn decode_retained_u8(bytes: &[u8], pos: &mut usize) -> Result<u8, String> {
    ensure_retained_input(bytes, *pos, 1)?;
    let value = bytes[*pos];
    *pos += 1;
    Ok(value)
}

fn decode_retained_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, String> {
    ensure_retained_input(bytes, *pos, 4)?;
    let value = u32::from_le_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
    ]);
    *pos += 4;
    Ok(value)
}

fn decode_retained_u64(bytes: &[u8], pos: &mut usize) -> Result<u64, String> {
    ensure_retained_input(bytes, *pos, 8)?;
    let value = u64::from_le_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
        bytes[*pos + 4],
        bytes[*pos + 5],
        bytes[*pos + 6],
        bytes[*pos + 7],
    ]);
    *pos += 8;
    Ok(value)
}

fn decode_retained_i64(bytes: &[u8], pos: &mut usize) -> Result<i64, String> {
    ensure_retained_input(bytes, *pos, 8)?;
    let value = i64::from_le_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
        bytes[*pos + 4],
        bytes[*pos + 5],
        bytes[*pos + 6],
        bytes[*pos + 7],
    ]);
    *pos += 8;
    Ok(value)
}

fn ensure_retained_input(bytes: &[u8], pos: usize, needed: usize) -> Result<(), String> {
    if bytes.len().saturating_sub(pos) < needed {
        Err("truncated retained prefix".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RETAINED_COLLECTION_LEN, StackValue, decode_retained_prefix,
        encode_retained_prefix_to_slice,
    };
    use alloc::vec;

    #[test]
    fn retained_prefix_codec_round_trips_nested_compound_ids() {
        let shared_buffer = StackValue::Buffer(7, b"value".to_vec());
        let stack = vec![
            shared_buffer.clone(),
            StackValue::Array(
                11,
                vec![
                    shared_buffer.clone(),
                    StackValue::Struct(
                        12,
                        vec![
                            StackValue::Map(
                                13,
                                vec![(shared_buffer.clone(), StackValue::Integer(5))],
                            ),
                            StackValue::Null,
                        ],
                    ),
                ],
            ),
        ];

        let mut buf = vec![0u8; 1024];
        let len = encode_retained_prefix_to_slice(&stack, &mut buf)
            .expect("retained prefix should encode into the scratch buffer");
        let decoded = decode_retained_prefix(&buf[..len])
            .expect("retained prefix should decode from the scratch buffer");

        assert_eq!(decoded, stack);
    }

    #[test]
    fn retained_prefix_codec_rejects_oversized_top_level_stack() {
        let stack = vec![StackValue::Null; MAX_RETAINED_COLLECTION_LEN + 1];
        let mut buf = vec![0u8; 1024];

        let err = encode_retained_prefix_to_slice(&stack, &mut buf)
            .expect_err("oversized retained prefixes must be rejected");

        assert_eq!(err, "collection length exceeds maximum");
    }

    #[test]
    fn retained_prefix_codec_round_trips_block_like_struct_argument_list() {
        let prev_hash = vec![
            0x15, 0x7c, 0xa8, 0xda, 0x91, 0xa2, 0x99, 0x58, 0x6f, 0x5f, 0xaa, 0xc4, 0x26, 0x7c,
            0x7d, 0x77, 0xec, 0x6b, 0xa0, 0x79, 0x3f, 0x8d, 0x9b, 0x7b, 0x5e, 0xaa, 0x6f, 0xa4,
            0xef, 0x1d, 0x4d, 0x1f,
        ];
        let block_like = StackValue::Struct(
            42,
            vec![
                StackValue::ByteString(vec![0xd9; 32]),
                StackValue::Integer(0),
                StackValue::ByteString(prev_hash.clone()),
                StackValue::ByteString(vec![0x72; 32]),
                StackValue::Integer(1),
                StackValue::Integer(2),
                StackValue::Integer(3),
                StackValue::Integer(4),
                StackValue::ByteString(vec![0x6b; 20]),
                StackValue::Integer(1),
            ],
        );
        let stack = vec![block_like, StackValue::ByteString(b"PrevHash".to_vec())];

        let mut buf = vec![0u8; 4096];
        let len = encode_retained_prefix_to_slice(&stack, &mut buf)
            .expect("block-like retained prefix should encode");
        let decoded =
            decode_retained_prefix(&buf[..len]).expect("block-like retained prefix should decode");

        assert_eq!(decoded, stack);
    }
}
