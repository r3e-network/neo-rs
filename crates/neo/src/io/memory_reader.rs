use super::{serializable::Serializable, IoError, IoResult};
use std::convert::TryInto;

#[derive(Debug, Clone, Copy)]
pub struct MemoryReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> MemoryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn ensure_available(&self, size: usize) -> IoResult<()> {
        if self.offset + size > self.data.len() {
            Err(IoError::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    /// Returns the current offset within the buffer.
    pub fn position(&self) -> usize {
        self.offset
    }

    /// Sets the current offset within the buffer.
    pub fn set_position(&mut self, position: usize) -> IoResult<()> {
        if position > self.data.len() {
            return Err(IoError::invalid_data("position exceeds buffer length"));
        }
        self.offset = position;
        Ok(())
    }

    pub fn read_bytes(&mut self, size: usize) -> IoResult<Vec<u8>> {
        self.ensure_available(size)?;
        let slice = &self.data[self.offset..self.offset + size];
        self.offset += size;
        Ok(slice.to_vec())
    }

    pub fn read_i8(&mut self) -> IoResult<i8> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_sbyte(&mut self) -> IoResult<i8> {
        self.read_i8()
    }

    pub fn read_u8(&mut self) -> IoResult<u8> {
        self.ensure_available(1)?;
        let value = self.data[self.offset];
        self.offset += 1;
        Ok(value)
    }

    pub fn read_byte(&mut self) -> IoResult<u8> {
        self.read_u8()
    }

    pub fn read_bool(&mut self) -> IoResult<bool> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_boolean(&mut self) -> IoResult<bool> {
        self.read_bool()
    }

    pub fn read_u16(&mut self) -> IoResult<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint16(&mut self) -> IoResult<u16> {
        self.read_u16()
    }

    pub fn read_i16(&mut self) -> IoResult<i16> {
        let bytes = self.read_bytes(2)?;
        Ok(i16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_int16(&mut self) -> IoResult<i16> {
        self.read_i16()
    }

    pub fn read_int16_big_endian(&mut self) -> IoResult<i16> {
        let bytes = self.read_bytes(2)?;
        Ok(i16::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint16_big_endian(&mut self) -> IoResult<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u32(&mut self) -> IoResult<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint32(&mut self) -> IoResult<u32> {
        self.read_u32()
    }

    pub fn read_int32(&mut self) -> IoResult<i32> {
        self.read_i32()
    }

    pub fn read_int32_big_endian(&mut self) -> IoResult<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint32_big_endian(&mut self) -> IoResult<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_i32(&mut self) -> IoResult<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u64(&mut self) -> IoResult<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint64(&mut self) -> IoResult<u64> {
        self.read_u64()
    }

    pub fn read_i64(&mut self) -> IoResult<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_int64(&mut self) -> IoResult<i64> {
        self.read_i64()
    }

    pub fn read_int64_big_endian(&mut self) -> IoResult<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_uint64_big_endian(&mut self) -> IoResult<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_var_int(&mut self, max: u64) -> IoResult<u64> {
        let first = self.read_u8()?;
        let value = match first {
            0xFF => self.read_u64()?,
            0xFE => self.read_u32()? as u64,
            0xFD => self.read_u16()? as u64,
            v => v as u64,
        };

        if value > max {
            return Err(IoError::invalid_data(format!(
                "var int exceeds maximum: {value} > {max}"
            )));
        }
        Ok(value)
    }

    pub fn read_var_bytes(&mut self, max: usize) -> IoResult<Vec<u8>> {
        let len = self.read_var_int(max as u64)? as usize;
        if len > max {
            return Err(IoError::invalid_data("var bytes length exceeds maximum"));
        }
        self.read_bytes(len)
    }

    #[inline]
    pub fn read_var_bytes_max(&mut self, max: usize) -> IoResult<Vec<u8>> {
        self.read_var_bytes(max)
    }

    /// Reads a fixed-length, null-terminated UTF-8 string.
    pub fn read_fixed_string(&mut self, length: usize) -> IoResult<String> {
        let bytes = self.read_bytes(length)?;

        // Split at the first null terminator (0x00) if present.
        let (string_bytes, padding) = match bytes.iter().position(|&b| b == 0) {
            Some(pos) => (&bytes[..pos], &bytes[pos..]),
            None => (bytes.as_slice(), &[][..]),
        };

        // Ensure the remaining bytes (if any) are all null to mirror the C# behaviour.
        if padding.iter().any(|&b| b != 0) {
            return Err(IoError::invalid_data(
                "fixed string contains non-null bytes after terminator",
            ));
        }

        String::from_utf8(string_bytes.to_vec())
            .map_err(|e| IoError::invalid_data(format!("invalid UTF-8 string: {}", e)))
    }

    pub fn read_var_string(&mut self, max: usize) -> IoResult<String> {
        let bytes = self.read_var_bytes(max)?;
        String::from_utf8(bytes).map_err(|e| IoError::invalid_data(e.to_string()))
    }

    #[inline]
    pub fn read_var_uint(&mut self) -> IoResult<u64> {
        self.read_var_int(u64::MAX)
    }

    pub fn read_serializable<T: Serializable>(&mut self) -> IoResult<T> {
        T::deserialize(self)
    }

    pub fn read_serializable_vec<T: Serializable>(&mut self) -> IoResult<Vec<T>> {
        let count = self.read_var_uint()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(T::deserialize(self)?);
        }
        Ok(values)
    }
}
