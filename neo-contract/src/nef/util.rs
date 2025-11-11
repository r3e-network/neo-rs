use alloc::{string::String, vec, vec::Vec};

use neo_base::encoding::{write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::{MAX_SCRIPT_SIZE, METHOD_NAME_MAX, TOKENS_MAX};

pub(crate) fn validate_compiler(compiler: &str) -> Result<(), DecodeError> {
    if compiler.len() > super::COMPILER_FIELD_SIZE {
        return Err(DecodeError::InvalidValue("Nef.compiler"));
    }
    Ok(())
}

pub(crate) fn validate_source(source: &str) -> Result<(), DecodeError> {
    if source.len() > super::SOURCE_URL_MAX {
        return Err(DecodeError::InvalidValue("Nef.source"));
    }
    Ok(())
}

pub(crate) fn validate_tokens_len(len: usize) -> Result<(), DecodeError> {
    if len > TOKENS_MAX {
        return Err(DecodeError::LengthOutOfRange {
            len: len as u64,
            max: TOKENS_MAX as u64,
        });
    }
    Ok(())
}

pub(crate) fn validate_script(script: &[u8]) -> Result<(), DecodeError> {
    if script.len() as u64 > MAX_SCRIPT_SIZE {
        return Err(DecodeError::LengthOutOfRange {
            len: script.len() as u64,
            max: MAX_SCRIPT_SIZE,
        });
    }
    Ok(())
}

pub(crate) fn validate_method_name(name: &str) -> Result<(), DecodeError> {
    if name.is_empty() || name.len() > METHOD_NAME_MAX {
        return Err(DecodeError::InvalidValue("MethodToken.method length"));
    }
    if name.starts_with('_') {
        return Err(DecodeError::InvalidValue(
            "MethodToken.method leading underscore",
        ));
    }
    Ok(())
}

pub(crate) fn read_limited_string<R: NeoRead>(
    reader: &mut R,
    max_len: usize,
    field: &'static str,
) -> Result<String, DecodeError> {
    let bytes = reader.read_var_bytes(max_len as u64)?;
    let text = String::from_utf8(bytes).map_err(|_| DecodeError::InvalidValue(field))?;
    Ok(text)
}

pub(crate) fn write_fixed_string<W: NeoWrite>(writer: &mut W, value: &str, size: usize) {
    let mut buffer = vec![0u8; size];
    let bytes = value.as_bytes();
    let len = bytes.len().min(size);
    buffer[..len].copy_from_slice(&bytes[..len]);
    writer.write_bytes(&buffer);
}

pub(crate) fn read_array<R, T>(reader: &mut R) -> Result<Vec<T>, DecodeError>
where
    R: NeoRead,
    T: NeoDecode,
{
    let len = reader.read_varint()? as usize;
    let mut items = Vec::with_capacity(len);
    for _ in 0..len {
        items.push(T::neo_decode(reader)?);
    }
    Ok(items)
}

pub(crate) fn write_array<W, T>(writer: &mut W, values: &[T])
where
    W: NeoWrite,
    T: NeoEncode,
{
    write_varint(writer, values.len() as u64);
    for value in values {
        value.neo_encode(writer);
    }
}
