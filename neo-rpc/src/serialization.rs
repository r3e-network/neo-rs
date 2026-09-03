//! Shared RPC serialization helpers for Neo wire-compatible payloads.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_io::{BinaryWriter, IoResult, Serializable};

pub(crate) fn serializable_to_bytes<T>(value: &T) -> IoResult<Vec<u8>>
where
    T: Serializable + ?Sized,
{
    let mut writer = BinaryWriter::new();
    value.serialize(&mut writer)?;
    Ok(writer.into_bytes())
}

pub(crate) fn serializable_to_base64<T>(value: &T) -> IoResult<String>
where
    T: Serializable + ?Sized,
{
    serializable_to_bytes(value).map(|bytes| BASE64_STANDARD.encode(bytes))
}
