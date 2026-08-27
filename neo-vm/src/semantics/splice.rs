//! Pure NeoVM splice opcode semantics.

use alloc::string::{String, ToString};

use crate::{
    concat_splice_values, slice_splice_value, stack_value_span_bytes, StackValue, MAX_ITEM_SIZE,
};

pub fn cat_values(left: &StackValue, right: &StackValue) -> Result<StackValue, String> {
    let result = concat_splice_values(left, right)
        .ok_or_else(|| "CAT: operands must expose byte memory".to_string())?;

    if let StackValue::Buffer(bytes) = &result {
        if bytes.len() > MAX_ITEM_SIZE {
            return Err("CAT: result exceeds max item size".to_string());
        }
    }

    Ok(result)
}

pub fn substr_value(value: &StackValue, index: i64, count: i64) -> Result<StackValue, String> {
    let bytes = stack_value_span_bytes(value)
        .ok_or_else(|| "SUBSTR: value does not expose byte memory".to_string())?;
    let (index, count) = checked_non_negative_range("SUBSTR", index, count)?;
    let end = checked_end("SUBSTR", index, count, bytes.len())?;

    slice_splice_value(value, index, end - index)
        .ok_or_else(|| "SUBSTR: range out of bounds".to_string())
}

pub fn left_value(value: &StackValue, count: i64) -> Result<StackValue, String> {
    let len = stack_value_span_bytes(value)
        .ok_or_else(|| "LEFT: value does not expose byte memory".to_string())?
        .len();
    let count = checked_non_negative_count("LEFT", count)?;
    if count > len {
        return Err("LEFT: count exceeds length".to_string());
    }

    slice_splice_value(value, 0, count).ok_or_else(|| "LEFT: count exceeds length".to_string())
}

pub fn right_value(value: &StackValue, count: i64) -> Result<StackValue, String> {
    let len = stack_value_span_bytes(value)
        .ok_or_else(|| "RIGHT: value does not expose byte memory".to_string())?
        .len();
    let count = checked_non_negative_count("RIGHT", count)?;
    if count > len {
        return Err("RIGHT: count exceeds length".to_string());
    }

    slice_splice_value(value, len - count, count)
        .ok_or_else(|| "RIGHT: count exceeds length".to_string())
}

pub fn memcpy_bytes(
    destination: &mut [u8],
    destination_index: i64,
    source: &StackValue,
    source_index: i64,
    count: i64,
) -> Result<(), String> {
    let source_bytes = stack_value_span_bytes(source)
        .ok_or_else(|| "MEMCPY: source does not expose byte memory".to_string())?;

    if count < 0 || source_index < 0 || destination_index < 0 {
        return Err("MEMCPY: negative argument".to_string());
    }

    #[allow(clippy::cast_sign_loss)]
    let (count, source_index, destination_index) = (
        count as usize,
        source_index as usize,
        destination_index as usize,
    );

    let source_end = source_index
        .checked_add(count)
        .ok_or_else(|| "MEMCPY: source range out of bounds".to_string())?;
    if source_end > source_bytes.len() {
        return Err("MEMCPY: source range out of bounds".to_string());
    }

    let destination_end = destination_index
        .checked_add(count)
        .ok_or_else(|| "MEMCPY: destination range out of bounds".to_string())?;
    if destination_end > destination.len() {
        return Err("MEMCPY: destination range out of bounds".to_string());
    }

    destination[destination_index..destination_end]
        .copy_from_slice(&source_bytes[source_index..source_end]);
    Ok(())
}

fn checked_non_negative_range(
    opcode: &str,
    index: i64,
    count: i64,
) -> Result<(usize, usize), String> {
    if index < 0 || count < 0 {
        return Err(alloc::format!("{opcode}: negative index or count"));
    }

    #[allow(clippy::cast_sign_loss)]
    Ok((index as usize, count as usize))
}

fn checked_non_negative_count(opcode: &str, count: i64) -> Result<usize, String> {
    if count < 0 {
        return Err(alloc::format!("{opcode}: negative count"));
    }

    #[allow(clippy::cast_sign_loss)]
    Ok(count as usize)
}

fn checked_end(opcode: &str, index: usize, count: usize, len: usize) -> Result<usize, String> {
    let end = index
        .checked_add(count)
        .ok_or_else(|| alloc::format!("{opcode}: range out of bounds"))?;
    if end > len {
        return Err(alloc::format!("{opcode}: range out of bounds"));
    }
    Ok(end)
}
