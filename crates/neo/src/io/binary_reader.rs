use super::{IoError, IoResult, MemoryReader};
use std::io::Read;

/// Utility helpers for reading primitives from a `Read` implementation. The
/// C# codebase relies heavily on `BinaryReader`; this module provides a minimal
/// substitute so the translated code can continue to operate on plain byte
/// streams.
pub struct BinaryReader;

impl BinaryReader {
    pub fn read_u8<R: Read>(reader: &mut R) -> IoResult<u8> {
        let mut buf = [0u8; 1];
        reader
            .read_exact(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        Ok(buf[0])
    }

    pub fn read_var_int<R: Read>(reader: &mut R) -> IoResult<u64> {
        let first = Self::read_u8(reader)?;
        let value = match first {
            0xFF => Self::read_u64(reader)?,
            0xFE => Self::read_u32(reader)? as u64,
            0xFD => Self::read_u16(reader)? as u64,
            v => v as u64,
        };
        Ok(value)
    }

    pub fn read_u16<R: Read>(reader: &mut R) -> IoResult<u16> {
        let mut buf = [0u8; 2];
        reader
            .read_exact(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        Ok(u16::from_le_bytes(buf))
    }

    pub fn read_u32<R: Read>(reader: &mut R) -> IoResult<u32> {
        let mut buf = [0u8; 4];
        reader
            .read_exact(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_u64<R: Read>(reader: &mut R) -> IoResult<u64> {
        let mut buf = [0u8; 8];
        reader
            .read_exact(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_var_bytes<R: Read>(reader: &mut R, max: usize) -> IoResult<Vec<u8>> {
        let len = Self::read_var_int(reader)? as usize;
        if len > max {
            return Err(IoError::invalid_data("var bytes length exceeds maximum"));
        }
        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        Ok(buf)
    }

    pub fn read_serializable<T: super::serializable::Serializable, R: Read>(
        reader: &mut R,
    ) -> IoResult<T> {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        let mut memory_reader = MemoryReader::new(&buf);
        T::deserialize(&mut memory_reader)
    }
}
