//! `MemoryReader` - matches C# Neo.IO.MemoryReader exactly

use std::io;
use std::str;

use thiserror::Error;

/// Errors that can occur while reading Neo binary data.
#[derive(Debug, Error)]
pub enum IoError {
    /// The stream does not contain enough data or contains malformed data.
    #[error("format error")]
    Format,
    /// The stream contained bytes that are not valid UTF-8.
    #[error("invalid utf-8 data")]
    InvalidUtf8,
    /// The stream contained data that failed a semantic validation.
    #[error("{context}: {value}")]
    InvalidData { context: String, value: String },
    /// Wrapper around lower-level I/O errors when writing.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Result alias for IO operations within Neo.IO.
pub type IoResult<T> = Result<T, IoError>;

/// Memory reader matching C# Neo.IO.MemoryReader.
#[derive(Debug, Clone, Copy)]
pub struct MemoryReader<'a> {
    buffer: &'a [u8],
    position: usize,
}

impl<'a> MemoryReader<'a> {
    /// Creates a new `MemoryReader` (C# constructor)
    #[must_use] 
    pub const fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    /// Gets the current position (C# Position property)
    #[must_use] 
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Sets the reader position (C# Position setter)
    pub fn set_position(&mut self, position: usize) -> IoResult<()> {
        if position > self.buffer.len() {
            Err(IoError::Format)
        } else {
            self.position = position;
            Ok(())
        }
    }

    /// Returns the total length of the backing buffer
    #[must_use] 
    pub const fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true when the backing buffer contains zero bytes.
    #[must_use] 
    pub const fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the number of unread bytes remaining in the buffer.
    #[inline]
    #[must_use] 
    pub const fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.position)
    }
    #[inline]
    const fn ensure_available(&self, required: usize) -> IoResult<()> {
        if self.position.saturating_add(required) > self.buffer.len() {
            Err(IoError::Format)
        } else {
            Ok(())
        }
    }

    #[inline]
    fn read_array<const N: usize>(&mut self) -> IoResult<[u8; N]> {
        self.ensure_available(N)?;
        let end = self.position + N;
        let slice = &self.buffer[self.position..end];
        self.position = end;
        slice.try_into().map_err(|_| IoError::Format)
    }

    #[inline]
    fn read_slice(&mut self, length: usize) -> IoResult<&'a [u8]> {
        self.ensure_available(length)?;
        let start = self.position;
        let end = start + length;
        self.position = end;
        Ok(&self.buffer[start..end])
    }

    /// Reads a sequence of bytes and returns them as an owned vector.
    #[inline]
    pub fn read_bytes(&mut self, length: usize) -> IoResult<Vec<u8>> {
        Ok(self.read_slice(length)?.to_vec())
    }

    /// Peeks at the next byte without advancing position (C# Peek)
    #[inline]
    pub fn peek(&self) -> IoResult<u8> {
        self.ensure_available(1)?;
        Ok(self.buffer[self.position])
    }

    /// Reads a boolean (C# `ReadBoolean`)
    #[inline]
    pub fn read_boolean(&mut self) -> IoResult<bool> {
        match self.read_byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(IoError::Format),
        }
    }

    /// Alias for `read_boolean` for API parity with legacy code.
    #[inline]
    pub fn read_bool(&mut self) -> IoResult<bool> {
        self.read_boolean()
    }

    /// Reads a signed byte (C# `ReadSByte`)
    #[inline]
    pub fn read_sbyte(&mut self) -> IoResult<i8> {
        Ok(self.read_byte()? as i8)
    }

    /// Reads an unsigned byte (C# `ReadByte`)
    #[inline]
    pub fn read_byte(&mut self) -> IoResult<u8> {
        Ok(self.read_array::<1>()?[0])
    }

    /// Alias for `read_byte` (C# `ReadByte` vs `ReadUInt8` naming differences).
    #[inline]
    pub fn read_u8(&mut self) -> IoResult<u8> {
        self.read_byte()
    }

    /// Reads a 16-bit signed integer in little-endian (C# `ReadInt16`)
    #[inline]
    pub fn read_int16(&mut self) -> IoResult<i16> {
        Ok(i16::from_le_bytes(self.read_array::<2>()?))
    }

    /// Alias for `read_int16`.
    #[inline]
    pub fn read_i16(&mut self) -> IoResult<i16> {
        self.read_int16()
    }

    /// Reads a 16-bit signed integer in big-endian (C# `ReadInt16BigEndian`)
    #[inline]
    pub fn read_int16_big_endian(&mut self) -> IoResult<i16> {
        Ok(i16::from_be_bytes(self.read_array::<2>()?))
    }

    /// Reads a 16-bit unsigned integer in little-endian (C# `ReadUInt16`)
    #[inline]
    pub fn read_uint16(&mut self) -> IoResult<u16> {
        Ok(u16::from_le_bytes(self.read_array::<2>()?))
    }

    /// Alias for `read_uint16` to mirror the C# API more closely.
    #[inline]
    pub fn read_u16(&mut self) -> IoResult<u16> {
        self.read_uint16()
    }

    /// Reads a 16-bit unsigned integer in big-endian (C# `ReadUInt16BigEndian`)
    #[inline]
    pub fn read_uint16_big_endian(&mut self) -> IoResult<u16> {
        Ok(u16::from_be_bytes(self.read_array::<2>()?))
    }

    /// Reads a 32-bit signed integer in little-endian (C# `ReadInt32`)
    #[inline]
    pub fn read_int32(&mut self) -> IoResult<i32> {
        Ok(i32::from_le_bytes(self.read_array::<4>()?))
    }

    /// Alias for `read_int32`.
    #[inline]
    pub fn read_i32(&mut self) -> IoResult<i32> {
        self.read_int32()
    }

    /// Reads a 32-bit signed integer in big-endian (C# `ReadInt32BigEndian`)
    #[inline]
    pub fn read_int32_big_endian(&mut self) -> IoResult<i32> {
        Ok(i32::from_be_bytes(self.read_array::<4>()?))
    }

    /// Reads a 32-bit unsigned integer in little-endian (C# `ReadUInt32`)
    #[inline]
    pub fn read_uint32(&mut self) -> IoResult<u32> {
        Ok(u32::from_le_bytes(self.read_array::<4>()?))
    }

    /// Alias for `read_uint32` to mirror the C# API more closely.
    #[inline]
    pub fn read_u32(&mut self) -> IoResult<u32> {
        self.read_uint32()
    }

    /// Reads a 32-bit unsigned integer in big-endian (C# `ReadUInt32BigEndian`)
    #[inline]
    pub fn read_uint32_big_endian(&mut self) -> IoResult<u32> {
        Ok(u32::from_be_bytes(self.read_array::<4>()?))
    }

    /// Reads a 64-bit signed integer in little-endian (C# `ReadInt64`)
    #[inline]
    pub fn read_int64(&mut self) -> IoResult<i64> {
        Ok(i64::from_le_bytes(self.read_array::<8>()?))
    }

    /// Alias for `read_int64`.
    #[inline]
    pub fn read_i64(&mut self) -> IoResult<i64> {
        self.read_int64()
    }

    /// Reads a 64-bit signed integer in big-endian (C# `ReadInt64BigEndian`)
    #[inline]
    pub fn read_int64_big_endian(&mut self) -> IoResult<i64> {
        Ok(i64::from_be_bytes(self.read_array::<8>()?))
    }

    /// Reads a 64-bit unsigned integer in little-endian (C# `ReadUInt64`)
    #[inline]
    pub fn read_uint64(&mut self) -> IoResult<u64> {
        Ok(u64::from_le_bytes(self.read_array::<8>()?))
    }

    /// Alias for `read_uint64` to mirror the C# API more closely.
    #[inline]
    pub fn read_u64(&mut self) -> IoResult<u64> {
        self.read_uint64()
    }

    /// Reads a 64-bit unsigned integer in big-endian (C# `ReadUInt64BigEndian`)
    #[inline]
    pub fn read_uint64_big_endian(&mut self) -> IoResult<u64> {
        Ok(u64::from_be_bytes(self.read_array::<8>()?))
    }

    /// Reads a variable-length integer (C# `ReadVarInt`)
    #[inline]
    pub fn read_var_int(&mut self, max: u64) -> IoResult<u64> {
        let prefix = self.read_byte()?;
        let value = match prefix {
            0xfd => u64::from(self.read_uint16()?),
            0xfe => u64::from(self.read_uint32()?),
            0xff => self.read_uint64()?,
            _ => u64::from(prefix),
        };
        if value > max {
            return Err(IoError::Format);
        }
        Ok(value)
    }

    /// Reads a variable-length integer with no upper bound (alias for C# `ReadVarInt` without `max`).
    #[inline]
    pub fn read_var_uint(&mut self) -> IoResult<u64> {
        self.read_var_int(u64::MAX)
    }

    /// Reads a fixed-length string (C# `ReadFixedString`)
    #[inline]
    pub fn read_fixed_string(&mut self, length: usize) -> IoResult<String> {
        let slice = self.read_slice(length)?;
        let mut end = 0;
        while end < slice.len() && slice[end] != 0 {
            end += 1;
        }
        for &byte in &slice[end..] {
            if byte != 0 {
                return Err(IoError::Format);
            }
        }
        let text = str::from_utf8(&slice[..end]).map_err(|_| IoError::InvalidUtf8)?;
        Ok(text.to_string())
    }

    /// Reads a variable-length string (C# `ReadVarString`)
    #[inline]
    pub fn read_var_string(&mut self, max: usize) -> IoResult<String> {
        let length = self.read_var_int(max as u64)? as usize;
        let data = self.read_slice(length)?;
        let text = str::from_utf8(data).map_err(|_| IoError::InvalidUtf8)?;
        Ok(text.to_string())
    }

    /// Reads memory (C# `ReadMemory`)
    #[inline]
    pub fn read_memory(&mut self, count: usize) -> IoResult<&'a [u8]> {
        self.read_slice(count)
    }

    /// Reads variable-length memory (C# `ReadVarMemory`)
    #[inline]
    pub fn read_var_memory(&mut self, max: usize) -> IoResult<&'a [u8]> {
        let count = self.read_var_int(max as u64)? as usize;
        self.read_slice(count)
    }

    /// Reads variable-length memory and returns an owned buffer (C# `ReadVarBytes`)
    #[inline]
    pub fn read_var_bytes(&mut self, max: usize) -> IoResult<Vec<u8>> {
        Ok(self.read_var_memory(max)?.to_vec())
    }

    /// Alias for `read_var_bytes` with explicit naming used in some ports.
    #[inline]
    pub fn read_var_bytes_max(&mut self, max: usize) -> IoResult<Vec<u8>> {
        self.read_var_bytes(max)
    }

    /// Reads to end (C# `ReadToEnd`)
    #[inline]
    pub fn read_to_end(&mut self) -> IoResult<&'a [u8]> {
        let remaining = self.buffer.len().saturating_sub(self.position);
        self.read_slice(remaining)
    }
}

impl From<str::Utf8Error> for IoError {
    fn from(_: str::Utf8Error) -> Self {
        Self::InvalidUtf8
    }
}

impl IoError {
    /// Helper for creating `InvalidData` errors with a default context.
    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData {
            context: "invalid data".to_string(),
            value: message.into(),
        }
    }

    /// Helper for creating `InvalidData` errors with explicit context/value pairs.
    pub fn invalid_data_with_context(context: impl Into<String>, value: impl Into<String>) -> Self {
        Self::InvalidData {
            context: context.into(),
            value: value.into(),
        }
    }
}
